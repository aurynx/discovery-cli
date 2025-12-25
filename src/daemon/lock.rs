#![allow(unsafe_code)]

use anyhow::{Context, Result, anyhow};
#[cfg(not(target_os = "macos"))]
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Maximum number of retry attempts when lock is held by another process
const MAX_LOCK_RETRIES: usize = 3;

/// IPC Request for health check
#[derive(Debug, Serialize)]
struct PingRequest {
    action: String,
}

/// IPC Response for health check
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum PingResponse {
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "error")]
    Error { message: String },
}

/// Atomic daemon lock using advisory file locking (flock)
///
/// This struct ensures that only ONE daemon process can run per cache file,
/// even under high concurrency (e.g., 100 concurrent PHP requests).
///
/// # Atomicity Guarantee
///
/// The lock is acquired using `try_lock_exclusive()` which is atomic at the OS level.
/// Unlike the "check PID file exists, then write PID" pattern, this eliminates the
/// TOCTOU (Time-of-Check to Time-of-Use) race condition window.
///
/// # Lock Lifetime
///
/// The lock is held for the entire daemon lifetime. When the daemon exits (gracefully
/// or via crash), the OS automatically releases the lock. The `Drop` implementation
/// ensures cleanup on graceful shutdown.
///
/// # Stale Lock Detection
///
/// If another process holds the lock, we verify it's healthy by:
/// 1. Reading PID from lock file
/// 2. Checking if process exists (via `kill(0)`)
/// 3. Sending IPC ping to Unix socket
/// 4. If any check fails → consider lock stale and retry
///
/// # Usage
///
/// ```rust,ignore
/// # use aurynx::daemon::lock::DaemonLock;
/// # use std::path::PathBuf;
/// # let lock_path = PathBuf::from("/tmp/lock");
/// # let socket_path = PathBuf::from("/tmp/socket");
/// let lock = DaemonLock::acquire(&lock_path, &socket_path, false)?;
/// // Lock is now held exclusively
/// // ... run daemon ...
/// // Lock released automatically on Drop
/// # Ok::<(), anyhow::Error>(())
/// ```
#[derive(Debug)]
pub struct DaemonLock {
    /// Lock file handle (kept open to hold flock)
    file: File,
    /// Path to lock file
    path: PathBuf,
    /// Current process PID (for verification)
    pid: u32,
}

impl DaemonLock {
    /// Acquire exclusive daemon lock with atomic guarantees
    ///
    /// # Arguments
    ///
    /// * `lock_path` - Path to lock file (e.g., `/tmp/aurynx-discovery-{hash}.lock`)
    /// * `socket_path` - Path to Unix socket for health checks
    /// * `force` - If true, forcefully break existing lock (dangerous!)
    ///
    /// # Returns
    ///
    /// * `Ok(DaemonLock)` - Lock acquired successfully
    /// * `Err(AlreadyRunning)` - Another healthy daemon is running
    /// * `Err(...)` - Other errors (permissions, I/O, etc.)
    ///
    /// # Atomicity
    ///
    /// This method uses `try_lock_exclusive()` which is atomic:
    /// - If lock is free → acquired in single syscall
    /// - If lock is held → returns immediately without waiting
    /// - No race condition window between check and acquire
    pub fn acquire(lock_path: &Path, socket_path: &Path, force: bool) -> Result<Self> {
        let pid = std::process::id();

        info!(
            lock_path = ?lock_path,
            pid = pid,
            force = force,
            "Attempting to acquire daemon lock"
        );

        // Open lock file (create if not exists)
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);

        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // O_EXLOCK: Atomic open + lock
            // O_NONBLOCK: Fail immediately if locked
            options.custom_flags(libc::O_EXLOCK | libc::O_NONBLOCK);
        }

        let mut file = match options.open(lock_path) {
            Ok(f) => f,
            Err(e) => {
                // Check if error is due to lock held
                // On macOS with O_EXLOCK | O_NONBLOCK, it returns EWOULDBLOCK (35)
                let is_locked = if cfg!(target_os = "macos") {
                    e.kind() == std::io::ErrorKind::WouldBlock || e.raw_os_error() == Some(35)
                } else {
                    false
                };

                if !is_locked {
                    // If not macOS, or other error, we proceed to try_lock_exclusive below
                    // But wait, if open failed, we can't proceed.
                    // If it's not macOS, open() shouldn't fail due to lock unless we used O_EXLOCK.
                    // So this branch is mainly for macOS lock contention.
                    if cfg!(target_os = "macos") {
                        // Fallthrough to handle locked state
                    } else {
                        return Err(anyhow!("Failed to open lock file: {e}"));
                    }
                }

                // If we are here, it's locked (on macOS) or open failed (shouldn't happen for non-macOS here)
                // Actually, for non-macOS, we just open() normally.
                // So let's restructure this.

                // Lock is held by another process
                debug!(error = ?e, "Lock is held by another process");

                if force {
                    warn!("Force flag set - attempting to kill existing process");
                    // ... force logic ...
                    // For macOS, we need to retry open()
                    if let Ok(old_pid) = Self::read_pid(lock_path)
                        && Self::is_process_running(old_pid) {
                            // kill...
                            #[cfg(unix)]
                            unsafe {
                                libc::kill(old_pid as i32, libc::SIGTERM);
                                std::thread::sleep(std::time::Duration::from_millis(200));
                                if Self::is_process_running(old_pid) {
                                    libc::kill(old_pid as i32, libc::SIGKILL);
                                    std::thread::sleep(std::time::Duration::from_millis(100));
                                }
                            }
                        }

                    // Retry open
                    match options.open(lock_path) {
                        Ok(f) => {
                            info!("Successfully acquired lock after force action");
                            let mut f = f;
                            Self::write_pid(&mut f, pid)?;
                            return Ok(Self {
                                file: f,
                                path: lock_path.to_path_buf(),
                                pid,
                            });
                        },
                        Err(e) => {
                            return Err(anyhow!(
                                "Failed to acquire lock even with --force flag: {e}"
                            ));
                        },
                    }
                }

                // Verify if the lock holder is healthy with retry logic
                Self::verify_lock_holder_with_retry(lock_path, socket_path)?;

                // If we reached here, the lock holder is healthy
                return Err(anyhow!(
                    "Daemon already running (lock held by healthy process)"
                ));
            },
        };

        // On non-macOS, we need to lock explicitly using fs2
        #[cfg(not(target_os = "macos"))]
        {
            if let Err(e) = file.try_lock_exclusive() {
                // Lock is held by another process
                debug!(error = ?e, "Lock is held by another process");

                if force {
                    warn!("Force flag set - attempting to kill existing process");

                    // Try to read PID from the file
                    if let Ok(old_pid) = Self::read_pid(lock_path) {
                        if Self::is_process_running(old_pid) {
                            info!(pid = old_pid, "Killing existing daemon process");
                            #[cfg(unix)]
                            unsafe {
                                libc::kill(old_pid as i32, libc::SIGTERM);
                                std::thread::sleep(std::time::Duration::from_millis(200));
                                if Self::is_process_running(old_pid) {
                                    warn!(pid = old_pid, "Process didn't exit, sending SIGKILL");
                                    libc::kill(old_pid as i32, libc::SIGKILL);
                                    std::thread::sleep(std::time::Duration::from_millis(100));
                                }
                            }
                        }
                    }

                    // Retry lock acquisition
                    if file.try_lock_exclusive().is_ok() {
                        info!("Successfully acquired lock after force action");
                        Self::write_pid(&mut file, pid)?;
                        return Ok(Self {
                            file,
                            path: lock_path.to_path_buf(),
                            pid,
                        });
                    }

                    return Err(anyhow!(
                        "Failed to acquire lock even with --force flag: {}",
                        e
                    ));
                }

                // Verify if the lock holder is healthy with retry logic
                Self::verify_lock_holder_with_retry(lock_path, socket_path)?;

                // If we reached here, the lock holder is healthy
                return Err(anyhow!(
                    "Daemon already running (lock held by healthy process)"
                ));
            }
        }

        // Verify that the file we locked is still the one at lock_path
        // This prevents the race where we opened the file, then someone else removed it,
        // and we locked the unlinked file.
        let file_meta = file.metadata()?;
        let inode = file_meta.ino();

        match std::fs::metadata(lock_path) {
            Ok(path_meta) => {
                if inode != path_meta.ino() {
                    return Err(anyhow!(
                        "Lock file replaced during acquisition (race condition)"
                    ));
                }
            },
            Err(_) => {
                return Err(anyhow!(
                    "Lock file removed during acquisition (race condition)"
                ));
            },
        }

        // Lock acquired! Write our PID and return
        info!(pid = pid, inode = inode, "Lock acquired successfully");
        Self::write_pid(&mut file, pid)?;

        Ok(Self {
            file,
            path: lock_path.to_path_buf(),
            pid,
        })
    }

    /// Write PID to lock file (overwrite existing content)
    fn write_pid(file: &mut File, pid: u32) -> Result<()> {
        use std::io::Seek;

        // Truncate file and write PID
        file.set_len(0)?;
        file.seek(std::io::SeekFrom::Start(0))?;
        write!(file, "{pid}")?;
        file.sync_all()?;

        debug!(pid = pid, "PID written to lock file");
        Ok(())
    }

    /// Read PID from lock file
    fn read_pid(lock_path: &Path) -> Result<u32> {
        let mut file = File::open(lock_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        let pid = content
            .trim()
            .parse::<u32>()
            .with_context(|| format!("Invalid PID in lock file: {content}"))?;

        Ok(pid)
    }

    /// Check if a process with given PID is running
    #[cfg(unix)]
    fn is_process_running(pid: u32) -> bool {
        // Use kill(pid, 0) - sends null signal to check process existence
        // 0 = success, -1 = error. If error is EPERM, process exists but we can't signal it.
        unsafe {
            let ret = libc::kill(pid as i32, 0);
            if ret == 0 {
                return true;
            }
            let err = std::io::Error::last_os_error().raw_os_error();
            err == Some(libc::EPERM)
        }
    }

    #[cfg(windows)]
    fn is_process_running(pid: u32) -> bool {
        use std::process::Command;

        Command::new("tasklist")
            .arg("/FI")
            .arg(format!("PID eq {}", pid))
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }

    /// Send IPC ping to verify daemon is healthy
    fn ping_daemon(socket_path: &Path, timeout: Duration) -> Result<()> {
        debug!(socket = ?socket_path, "Attempting IPC ping");

        // Connect to Unix socket with timeout
        let mut stream = UnixStream::connect(socket_path)
            .with_context(|| format!("Failed to connect to socket: {socket_path:?}"))?;

        stream
            .set_read_timeout(Some(timeout))
            .context("Failed to set read timeout")?;
        stream
            .set_write_timeout(Some(timeout))
            .context("Failed to set write timeout")?;

        // Send ping request
        let request = PingRequest {
            action: "ping".to_string(),
        };
        let request_json = serde_json::to_string(&request)?;
        stream.write_all(request_json.as_bytes())?;
        stream.write_all(b"\n")?;

        // Read response
        let mut response_data = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = stream.read(&mut buf)?;
            if n == 0 {
                break;
            }
            response_data.extend_from_slice(&buf[..n]);
            if response_data.ends_with(b"\n") {
                break;
            }
        }

        // Parse response
        let response: PingResponse =
            serde_json::from_slice(&response_data).context("Failed to parse ping response")?;

        match response {
            PingResponse::Pong => {
                debug!("IPC ping successful - daemon is healthy");
                Ok(())
            },
            PingResponse::Error { message } => Err(anyhow!("Daemon returned error: {message}")),
        }
    }

    /// Verify lock holder is healthy with exponential backoff retry
    ///
    /// Retry logic:
    /// - Attempt 1: immediate
    /// - Attempt 2: 100ms delay
    /// - Attempt 3: 300ms delay
    /// - Attempt 4: 1000ms delay
    ///
    /// If all retries fail → consider lock stale and allow cleanup
    fn verify_lock_holder_with_retry(lock_path: &Path, socket_path: &Path) -> Result<()> {
        let mut last_error = None;

        for attempt in 0..MAX_LOCK_RETRIES {
            if attempt > 0 {
                let delay_ms = match attempt {
                    1 => 100,
                    2 => 300,
                    _ => 1000,
                };
                debug!(
                    attempt = attempt,
                    delay_ms = delay_ms,
                    "Retrying lock holder verification"
                );
                std::thread::sleep(Duration::from_millis(delay_ms));
            }

            match Self::verify_lock_holder(lock_path, socket_path) {
                Ok(()) => {
                    // Lock holder is healthy
                    return Ok(());
                },
                Err(e) => {
                    warn!(
                        attempt = attempt,
                        error = ?e,
                        "Lock holder verification failed"
                    );
                    last_error = Some(e);
                },
            }
        }

        // All retries failed - lock is stale
        let error = last_error.unwrap();
        Err(anyhow!(
            "Lock holder appears stale after {MAX_LOCK_RETRIES} retries: {error}"
        ))
    }

    /// Verify that the lock holder is a healthy daemon
    ///
    /// Checks:
    /// 1. Read PID from lock file
    /// 2. Check if process exists
    /// 3. Send IPC ping to socket
    ///
    /// If any check fails → lock is stale
    fn verify_lock_holder(lock_path: &Path, socket_path: &Path) -> Result<()> {
        // Step 1: Read PID from lock file
        let pid = Self::read_pid(lock_path)
            .context("Failed to read PID from lock file (possibly stale)")?;

        debug!(pid = pid, "Found PID in lock file");

        // Step 2: Check if process exists
        if !Self::is_process_running(pid) {
            return Err(anyhow!("Process {pid} not running (lock is stale)"));
        }

        debug!(pid = pid, "Process is running");

        // Step 3: Send IPC ping to verify daemon is responsive
        Self::ping_daemon(socket_path, Duration::from_secs(2))
            .context("Daemon not responding to IPC ping (lock is stale)")?;

        info!(pid = pid, "Lock holder verified as healthy daemon");
        Ok(())
    }

    /// Get lock file path from cache file path (deterministic hash)
    ///
    /// Uses xxh3 hash of cache path to generate unique lock file name:
    /// `/tmp/aurynx-discovery-{hash}.lock`
    ///
    /// This ensures that different cache files get different locks,
    /// allowing multiple independent daemons.
    pub fn path_from_cache(cache_path: &Path) -> PathBuf {
        let hash = xxhash_rust::xxh3::xxh3_64(cache_path.as_os_str().as_bytes());
        std::env::temp_dir().join(format!("aurynx-discovery-{hash:x}.lock"))
    }

    /// Verify that lock is still held by current process
    ///
    /// This is a paranoid check to detect lock file tampering.
    /// Should not fail under normal circumstances.
    pub fn verify_current_process(&self) -> Result<()> {
        let pid_in_file =
            Self::read_pid(&self.path).context("Failed to read PID from lock file (lock lost?)")?;

        if pid_in_file != self.pid {
            return Err(anyhow!(
                "Lock file PID mismatch! Expected {}, found {}. Lock was tampered with!",
                self.pid,
                pid_in_file
            ));
        }

        Ok(())
    }
}

impl Drop for DaemonLock {
    fn drop(&mut self) {
        // Unlock file (releases flock)
        if let Err(e) = self.file.unlock() {
            warn!(error = ?e, path = ?self.path, "Failed to unlock file");
        }

        // Delete lock file
        if let Err(e) = std::fs::remove_file(&self.path) {
            warn!(error = ?e, path = ?self.path, "Failed to remove lock file");
        } else {
            info!(path = ?self.path, pid = self.pid, "Lock released and file removed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_lock_path_from_cache() {
        let cache1 = PathBuf::from("/tmp/cache1.php");
        let cache2 = PathBuf::from("/tmp/cache2.php");

        let lock1 = DaemonLock::path_from_cache(&cache1);
        let lock2 = DaemonLock::path_from_cache(&cache2);

        // Different caches should have different lock files
        assert_ne!(lock1, lock2);

        // Same cache should always produce same lock file
        let lock1_again = DaemonLock::path_from_cache(&cache1);
        assert_eq!(lock1, lock1_again);

        // Should be in temp dir
        assert!(lock1.starts_with(std::env::temp_dir()));
        assert!(lock1.to_string_lossy().contains("aurynx-discovery"));
        assert!(lock1.extension().unwrap() == "lock");
    }

    #[test]
    fn test_write_and_read_pid() {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("test.lock");

        // Create file and write PID
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        let test_pid = 12345u32;
        DaemonLock::write_pid(&mut file, test_pid).unwrap();

        // Read PID back
        let read_pid = DaemonLock::read_pid(&lock_path).unwrap();
        assert_eq!(test_pid, read_pid);
    }

    #[test]
    fn test_is_process_running() {
        // Current process should be running
        let current_pid = std::process::id();
        assert!(DaemonLock::is_process_running(current_pid));

        // Very high PID should not exist
        assert!(!DaemonLock::is_process_running(999999));
    }

    #[test]
    fn test_acquire_lock_success() {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("test.lock");
        let socket_path = temp_dir.path().join("test.sock");

        // Should successfully acquire lock
        let lock = DaemonLock::acquire(&lock_path, &socket_path, false);
        assert!(lock.is_ok());

        let lock = lock.unwrap();
        assert_eq!(lock.pid, std::process::id());

        // Verify PID written to file
        let pid_in_file = DaemonLock::read_pid(&lock_path).unwrap();
        assert_eq!(pid_in_file, std::process::id());

        // Lock should be released on drop
        drop(lock);
        assert!(!lock_path.exists());
    }

    #[test]
    fn test_acquire_lock_twice_fails() {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("test.lock");
        let socket_path = temp_dir.path().join("test.sock");

        // First lock succeeds
        let _lock1 = DaemonLock::acquire(&lock_path, &socket_path, false).unwrap();

        // Second lock should fail (lock held)
        let lock2 = DaemonLock::acquire(&lock_path, &socket_path, false);
        assert!(lock2.is_err());

        // Error should mention "already running" or "stale"
        let err_msg = lock2.unwrap_err().to_string();
        assert!(
            err_msg.contains("already running") || err_msg.contains("stale"),
            "Error message was: {}",
            err_msg
        );
    }

    #[test]
    fn test_force_flag_breaks_lock() {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("test.lock");
        let socket_path = temp_dir.path().join("test.sock");

        // First lock succeeds
        let lock1 = DaemonLock::acquire(&lock_path, &socket_path, false).unwrap();
        let first_pid = lock1.pid;

        // Drop first lock to release file handle (force still needs clean file descriptor)
        drop(lock1);

        // Recreate lock file with stale PID
        std::fs::write(&lock_path, format!("{}", first_pid)).unwrap();

        // Lock file exists but no lock held - force should work
        let lock2 = DaemonLock::acquire(&lock_path, &socket_path, true);
        assert!(lock2.is_ok(), "Force flag should allow reacquiring lock");

        // PID should be current process
        let pid_in_file = DaemonLock::read_pid(&lock_path).unwrap();
        assert_eq!(pid_in_file, std::process::id());
    }

    #[test]
    fn test_verify_current_process() {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("test.lock");
        let socket_path = temp_dir.path().join("test.sock");

        let lock = DaemonLock::acquire(&lock_path, &socket_path, false).unwrap();

        // Verification should succeed
        assert!(lock.verify_current_process().is_ok());

        // Manually tamper with lock file
        fs::write(&lock_path, "99999").unwrap();

        // Verification should now fail
        assert!(lock.verify_current_process().is_err());
    }

    #[test]
    fn test_force_kill_external_process() {
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::Duration;

        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("test_ext.lock");
        let socket_path = temp_dir.path().join("test_ext.sock");
        let ready_path = temp_dir.path().join("ready");
        let script_path = temp_dir.path().join("locker.py");

        // Python script to hold the lock
        let script = format!(
            r#"
import fcntl
import time
import sys
import os

lock_file = "{}"
ready_file = "{}"

# Open and lock
f = open(lock_file, 'w')
# Write PID
f.write(str(os.getpid()))
f.flush()

# Lock
try:
    fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
except IOError:
    sys.exit(1)

# Signal ready
with open(ready_file, 'w') as rf:
    rf.write("ready")

# Sleep forever (until killed)
while True:
    time.sleep(1)
"#,
            lock_path.display(),
            ready_path.display()
        );

        fs::write(&script_path, script).unwrap();

        // Start python process
        let mut child = Command::new("python3")
            .arg(&script_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn python3");

        // Wait for ready signal
        let mut attempts = 0;
        while !ready_path.exists() {
            if attempts > 50 {
                let _ = child.kill();
                panic!("Python script failed to start/lock");
            }
            thread::sleep(Duration::from_millis(100));
            attempts += 1;
        }

        // Verify we CANNOT acquire lock without force
        let lock_result = DaemonLock::acquire(&lock_path, &socket_path, false);
        assert!(
            lock_result.is_err(),
            "Should not acquire lock when held by python"
        );

        // Verify we CAN acquire lock WITH force
        // This should kill the python process
        let lock_result = DaemonLock::acquire(&lock_path, &socket_path, true);
        assert!(lock_result.is_ok(), "Should acquire lock with force");

        // Cleanup
        let _ = child.kill(); // Just in case
    }
}
