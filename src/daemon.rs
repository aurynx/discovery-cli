#![allow(clippy::unwrap_used, clippy::expect_used)] // Allow unwrap/expect for RwLock poisoning and signal setup

mod lock;

use crate::cache_strategy::{CacheStrategy, detect_cache_strategy};
use crate::error::{AurynxError, Result};
use crate::incremental::{FileEntry, MANIFEST_FILE, Manifest, perform_incremental_scan};
use crate::metadata::PhpClassMetadata;
use crate::scanner;
use crate::writer::write_php_cache;
use anyhow::Context;
use lock::DaemonLock;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tracing::{debug, info, warn};

/// Exit codes

#[allow(dead_code)]
const EXIT_SUCCESS: i32 = 0;
#[allow(dead_code)]
const EXIT_SIGNAL_ERROR: i32 = 2;
#[allow(dead_code)]
const EXIT_RUNTIME_ERROR: i32 = 3;
/// IPC Protocol: Plain text commands, plain text responses
/// NO JSON! Direct PHP code delivery for zero overhead.
///
/// Commands:
/// - "getCode" or "getCacheCode" -> Returns PHP code directly
/// - "getFilePath" -> Returns file path as plain text
/// - "ping" -> Returns "PONG"
/// - "stats" -> Returns "total:N strategy:X uptime:Y"
///
/// CRITICAL: This is a performance-critical path. DO NOT add JSON serialization.
/// PHP library expects raw PHP code, not JSON-wrapped data.

pub struct DaemonConfig {
    pub paths: Vec<PathBuf>,
    pub output_path: PathBuf,
    pub socket_path: PathBuf,
    pub pid_file: PathBuf,
    pub ignore_patterns: Vec<String>,
    pub verbose: bool,
    pub is_tty: bool,
    pub force: bool,
    pub write_to_disk: bool,
    pub pretty: bool,
    pub format: String,

    // Configurable limits
    pub max_file_size: u64,       // Maximum PHP file size in bytes
    pub max_request_size: usize,  // Maximum IPC request size in bytes
    pub max_cache_entries: usize, // Maximum number of cached classes
}

pub struct Daemon {
    cache: Arc<RwLock<HashMap<String, PhpClassMetadata>>>,
    manifest: Arc<RwLock<Manifest>>,
    config: DaemonConfig,
    strategy: CacheStrategy,
    start_time: Instant,
    shutdown_rx: Option<UnboundedReceiver<()>>,
    /// Daemon lock held for entire lifetime (prevents concurrent instances)
    _lock: DaemonLock,
}

impl Daemon {
    pub fn new(config: DaemonConfig) -> Result<Self> {
        let mut strategy = detect_cache_strategy(&config.output_path);

        // Override strategy if write_to_disk is enabled
        if config.write_to_disk {
            info!("Forcing File strategy due to --write-to-disk flag");
            strategy = CacheStrategy::File;
        }

        // Acquire daemon lock atomically (prevents race conditions)
        let lock_path = DaemonLock::path_from_cache(&config.output_path);
        let lock = DaemonLock::acquire(&lock_path, &config.socket_path, config.force)
            .context("Failed to acquire daemon lock")?;

        info!(
            lock_path = ?lock_path,
            pid = std::process::id(),
            force = config.force,
            "Daemon lock acquired successfully"
        );

        Ok(Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            manifest: Arc::new(RwLock::new(Manifest::default())),
            config,
            strategy,
            start_time: Instant::now(),
            shutdown_rx: None,
            _lock: lock,
        })
    }

    /// Log debug message (verbose mode)
    fn log(&self, message: &str) {
        if self.config.verbose {
            debug!(emoji = "🔮", "{}", message);
        }
    }

    /// Log info message
    fn log_info(&self, message: &str) {
        info!(emoji = "✨", "{}", message);
    }

    /// Log warning
    fn log_warn(&self, message: &str) {
        warn!(emoji = "⚠️", "{}", message);
    }

    /// Log crafting action (debug level)
    fn log_craft(&self, message: &str) {
        debug!(emoji = "🔮", "Crafting {}", message);
    }

    /// Cleanup orphaned files (socket, PID file)
    fn cleanup_files(&self) -> Result<()> {
        if self.config.socket_path.exists() {
            if let Err(e) = std::fs::remove_file(&self.config.socket_path) {
                self.log_warn(&format!("Failed to remove socket file: {e}"));
            } else {
                self.log_info(&format!("Cleaned up socket: {:?}", self.config.socket_path));
            }
        }

        if self.config.pid_file.exists() {
            if let Err(e) = std::fs::remove_file(&self.config.pid_file) {
                self.log_warn(&format!("Failed to remove PID file: {e}"));
            } else {
                self.log_info(&format!("Cleaned up PID: {:?}", self.config.pid_file));
            }
        }

        Ok(())
    }

    pub fn run(&mut self) -> Result<()> {
        // Canonicalize paths to resolve symlinks (important for macOS /tmp -> /private/tmp)
        // This ensures that paths in cache match paths from notify events
        let canonical_paths: Vec<PathBuf> = self
            .config
            .paths
            .iter()
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
            .collect();
        self.config.paths = canonical_paths;

        // Lock already acquired in new()
        // The atomic lock prevents race conditions even with 100+ concurrent requests

        // Setup panic hook for cleanup (prevents resource leaks on panic)
        let socket_path = self.config.socket_path.clone();
        let pid_file = self.config.pid_file.clone();

        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Attempt cleanup on panic
            let _ = std::fs::remove_file(&socket_path);
            let _ = std::fs::remove_file(&pid_file);
            warn!("Daemon panicked, cleaned up resources: {:?}", info);
            default_hook(info);
        }));

        // Write PID file (critical for PHP integration)
        if let Err(e) = std::fs::write(&self.config.pid_file, std::process::id().to_string()) {
            self.log_warn(&format!("Failed to write PID file: {e}"));
        } else {
            self.log_info(&format!("PID file created: {:?}", self.config.pid_file));
        }

        // Verify lock is still held by current process (paranoid check)
        self._lock
            .verify_current_process()
            .context("Lock verification failed - this should never happen")?;

        let pid = std::process::id();
        self.log_info(&format!("Daemon starting with PID {pid}"));

        // Setup signal handlers
        let (shutdown_tx, shutdown_rx) = unbounded_channel();
        self.shutdown_rx = Some(shutdown_rx);

        // Spawn signal handler thread
        let is_tty = self.config.is_tty;
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                Self::signal_handler(shutdown_tx, is_tty).await;
            });
        });

        // Initial scan
        self.log_craft("initial metadata scan...");
        self.scan_initial()?;
        let class_count = self.cache.read().unwrap().len();
        self.log_info(&format!(
            "Metadata crafted: {class_count} classes discovered"
        ));

        // Write initial cache file (for File strategy)
        if self.strategy == CacheStrategy::File {
            self.log_info("Attempting to write cache file...");
            match self.write_cache_file() {
                Ok(()) => self.log_info(&format!("Cache crafted at {:?}", self.config.output_path)),
                Err(e) => self.log_warn(&format!("Failed to write cache: {e}")),
            }
        }

        // Setup file watcher
        let (tx, rx) = channel();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(tx)?;

        for path in &self.config.paths {
            watcher.watch(path, RecursiveMode::Recursive)?;
            self.log_info(&format!("Watching crafted: {path:?}"));
        }

        // Setup Unix socket server (for IPC)
        #[cfg(unix)]
        let socket_listener = self.setup_unix_socket()?;

        info!(
            "🪄 Daemon ready! Strategy: {:?}, Socket: {:?}, Output: {:?}, Verbose: {}",
            self.strategy, self.config.socket_path, self.config.output_path, self.config.verbose
        );

        if self.config.is_tty {
            println!("   Press Ctrl+C to stop gracefully\n");
        }

        let mut last_write = Instant::now();
        let mut dirty = false;
        let mut pending_changes: Vec<PathBuf> = Vec::new();

        let result = loop {
            // Check for shutdown signal (non-blocking)
            if let Some(ref mut rx) = self.shutdown_rx
                && rx.try_recv().is_ok() {
                    self.log_info("Shutdown signal received, cleaning up...");
                    break Ok(());
                }

            // Collect file system events (adaptive batching)
            let batch_start = Instant::now();
            let base_debounce = Duration::from_millis(50);

            // Collect first event
            match rx.recv_timeout(base_debounce) {
                Ok(Ok(event)) => match self.collect_event_paths(event) {
                    Ok(paths) => pending_changes.extend(paths),
                    Err(e) => {
                        self.log_warn(&format!("Error collecting event paths: {e}"));
                    },
                },
                Ok(Err(e)) => {
                    self.log_warn(&format!("Watch error: {e}"));
                },
                Err(RecvTimeoutError::Timeout) => {
                    // Continue collecting events if we already have some
                    if !pending_changes.is_empty()
                        && batch_start.elapsed() < Duration::from_millis(300)
                    {
                        continue;
                    }
                },
                Err(RecvTimeoutError::Disconnected) => {
                    self.log_info("Watcher disconnected, shutting down");
                    break Ok(());
                },
            }

            // Continue collecting more events with adaptive debounce
            let adaptive_debounce = if pending_changes.len() > 100 {
                Duration::from_millis(1000) // Longer debounce for mass changes
            } else {
                Duration::from_millis(300) // Normal debounce
            };

            let collect_deadline = Instant::now() + adaptive_debounce;
            while Instant::now() < collect_deadline {
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(Ok(event)) => match self.collect_event_paths(event) {
                        Ok(paths) => pending_changes.extend(paths),
                        Err(e) => {
                            self.log_warn(&format!("Error collecting event paths: {e}"));
                        },
                    },
                    Ok(Err(e)) => {
                        self.log_warn(&format!("Watch error: {e}"));
                    },
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        self.log_info("Watcher disconnected, shutting down");
                        break;
                    },
                }
            }

            // Process batch if we have pending changes
            if !pending_changes.is_empty() {
                // Remove duplicates
                pending_changes.sort();
                pending_changes.dedup();

                if self.config.verbose {
                    if pending_changes.len() > 10 {
                        self.log_craft(&format!("batch: {} files", pending_changes.len()));
                    } else {
                        for path in &pending_changes {
                            self.log_craft(&format!("metadata for: {}", path.display()));
                        }
                    }
                }

                // Process batch in parallel
                match self.batch_rescan_files(&pending_changes) {
                    Ok(()) => dirty = true,
                    Err(e) => {
                        self.log_warn(&format!("Error in batch rescan: {e}"));
                    },
                }

                pending_changes.clear();
            }

            // Check for IPC requests (non-blocking)
            #[cfg(unix)]
            if let Err(e) = self.check_ipc_requests(&socket_listener) {
                self.log_warn(&format!("IPC error: {e}"));
                // Continue despite IPC errors
            }

            // Periodic flush (only for File strategy)
            if self.strategy == CacheStrategy::File && dirty
                && last_write.elapsed() >= Duration::from_millis(300) {
                    if let Err(e) = self.write_cache_file() {
                        self.log_warn(&format!("Failed to write cache: {e}"));
                    } else {
                        let count = self.cache.read().unwrap().len();
                        self.log(&format!("Cache recrafted: {count} classes"));
                    }
                    dirty = false;
                    last_write = Instant::now();
                }
        };

        // Graceful cleanup
        self.log_craft("graceful shutdown...");

        // Final cache flush if dirty
        if self.strategy == CacheStrategy::File && dirty {
            if let Err(e) = self.write_cache_file() {
                self.log_warn(&format!("Failed to write final cache: {e}"));
            } else {
                let count = self.cache.read().unwrap().len();
                self.log_info(&format!("Final cache crafted: {count} classes"));
            }
        }

        // Cleanup files
        self.cleanup_files()?;

        info!("Daemon stopped gracefully");
        if self.config.is_tty {
            println!("\n🪄 Daemon stopped gracefully\n");
        }

        result
    }

    /// Async signal handler
    async fn signal_handler(shutdown_tx: tokio::sync::mpsc::UnboundedSender<()>, is_tty: bool) {
        use tokio::signal;

        #[cfg(unix)]
        {
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to setup SIGTERM handler");
            let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
                .expect("Failed to setup SIGINT handler");
            let mut sighup = signal::unix::signal(signal::unix::SignalKind::hangup())
                .expect("Failed to setup SIGHUP handler");

            tokio::select! {
                _ = sigterm.recv() => {
                    info!(signal = "SIGTERM", "Received SIGTERM");
                    if is_tty {
                        println!("\n✨ Received SIGTERM");
                    }
                }
                _ = sigint.recv() => {
                    info!(signal = "SIGINT", "Received SIGINT (Ctrl+C)");
                    if is_tty {
                        println!("\n✨ Received SIGINT (Ctrl+C)");
                    }
                }
                _ = sighup.recv() => {
                    info!(signal = "SIGHUP", "Received SIGHUP");
                    if is_tty {
                        println!("\n✨ Received SIGHUP");
                    }
                }
            }
        }

        #[cfg(windows)]
        {
            signal::ctrl_c()
                .await
                .expect("Failed to setup Ctrl+C handler");
            info!(signal = "CTRL_C", "Received Ctrl+C");
            if is_tty {
                println!("\n✨ Received Ctrl+C");
            }
        }

        // Send shutdown signal
        let _ = shutdown_tx.send(());
    }

    fn scan_initial(&mut self) -> Result<()> {
        let manifest_path = if let Some(parent) = self.config.output_path.parent() {
            parent.join(MANIFEST_FILE)
        } else {
            PathBuf::from(MANIFEST_FILE)
        };

        let (metadata, new_manifest) = perform_incremental_scan(
            &manifest_path,
            &self.config.paths,
            &self.config.ignore_patterns,
            self.config.max_file_size,
        )?;

        // Update manifest
        *self.manifest.write().unwrap() = new_manifest;

        // Update cache
        let mut cache = self.cache.write().unwrap();
        for m in metadata {
            cache.insert(m.fqcn.clone(), m);
        }

        Ok(())
    }

    /// Collect paths from event for batch processing
    fn collect_event_paths(&self, event: notify::Event) -> Result<Vec<PathBuf>> {
        use notify::EventKind;

        let mut paths = Vec::new();

        match event.kind {
            EventKind::Modify(_) | EventKind::Create(_) => {
                for path in event.paths {
                    if path.extension().and_then(|s| s.to_str()) == Some("php") {
                        paths.push(path);
                    }
                }
            },
            EventKind::Remove(_) => {
                // Handle removals separately
                for path in event.paths {
                    let mut cache = self.cache.write().unwrap();
                    cache.retain(|_, m| m.file != path);
                }
            },
            _ => {},
        }

        Ok(paths)
    }

    /// Process multiple files in parallel
    fn batch_rescan_files(&mut self, paths: &[PathBuf]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }

        // Use scan_files_with_limit which handles parallel processing internally
        let max_file_size = self.config.max_file_size;
        let all_metadata = scanner::scan_files_with_limit(paths, max_file_size);

        // Update cache with results
        let mut cache = self.cache.write().unwrap();
        let mut manifest = self.manifest.write().unwrap();

        for metadata in all_metadata {
            let path = metadata.file.clone();
            let path_str = path.to_string_lossy().to_string();

            // Remove old entries for this file
            cache.retain(|_, m| m.file != path);

            // Update manifest - get parsed classes for this file
            let parsed_metadata = vec![metadata.clone()];
            let mtime = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .map(|t| {
                    t.duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                })
                .unwrap_or(0);

            manifest.files.insert(
                path_str,
                FileEntry {
                    mtime,
                    classes: parsed_metadata.clone(),
                },
            );

            // Security: check cache size limit
            if cache.len() >= self.config.max_cache_entries {
                self.log_warn(&format!(
                    "Cache limit reached ({} entries), skipping new entries",
                    self.config.max_cache_entries
                ));
                continue;
            }

            // Add new entries (with limit check)
            for m in parsed_metadata {
                if cache.len() >= self.config.max_cache_entries {
                    self.log_warn("Cache limit reached, stopping scan");
                    break;
                }
                cache.insert(m.fqcn.clone(), m);
            }
        }

        Ok(())
    }

    fn write_cache_file(&self) -> Result<()> {
        let cache = self.cache.read().unwrap();
        let metadata: Vec<_> = cache.values().cloned().collect();

        // Atomic write cache
        let temp = self.config.output_path.with_extension("tmp");

        match self.config.format.as_str() {
            "json" => crate::writer::write_json_cache(&metadata, &temp, self.config.pretty)?,
            _ => write_php_cache(&metadata, &temp, self.config.pretty)?,
        }

        std::fs::rename(temp, &self.config.output_path)?;

        // Write manifest
        if let Some(parent) = self.config.output_path.parent() {
            let manifest_path = parent.join(MANIFEST_FILE);
            let manifest = self.manifest.read().unwrap();
            manifest.save(&manifest_path)?;
        }

        Ok(())
    }

    #[cfg(unix)]
    fn setup_unix_socket(&self) -> Result<std::os::unix::net::UnixListener> {
        use std::os::unix::fs::PermissionsExt;

        // Remove old socket if exists
        let _ = std::fs::remove_file(&self.config.socket_path);

        let listener =
            std::os::unix::net::UnixListener::bind(&self.config.socket_path).map_err(|e| {
                AurynxError::io_error(
                    format!(
                        "Failed to bind Unix socket: {}",
                        self.config.socket_path.display()
                    ),
                    e,
                )
            })?;

        // Set non-blocking mode
        listener
            .set_nonblocking(true)
            .map_err(|e| AurynxError::io_error("Failed to set socket non-blocking", e))?;

        // Set strict permissions: 0600 (owner read/write only)
        let mut perms = std::fs::metadata(&self.config.socket_path)
            .map_err(|e| AurynxError::io_error("Failed to read socket metadata", e))?
            .permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&self.config.socket_path, perms)
            .map_err(|e| AurynxError::io_error("Failed to set socket permissions", e))?;

        Ok(listener)
    }

    #[cfg(unix)]
    fn check_ipc_requests(&self, listener: &std::os::unix::net::UnixListener) -> Result<()> {
        // Try to accept connection (non-blocking)
        match listener.accept() {
            Ok((stream, _addr)) => {
                // Set blocking mode for the connection
                stream
                    .set_nonblocking(false)
                    .map_err(|e| AurynxError::io_error("Failed to set stream blocking", e))?;

                // Set read timeout
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .map_err(|e| AurynxError::io_error("Failed to set read timeout", e))?;

                // Clone stream for reading (BufReader needs ownership)
                let stream_clone = stream
                    .try_clone()
                    .map_err(|e| AurynxError::io_error("Failed to clone stream", e))?;
                let reader = BufReader::new(stream_clone);
                let mut writer = stream;

                for line in reader.lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(e) => {
                            warn!(error = %e, "IPC read error");
                            break;
                        },
                    };

                    // Security: limit request size
                    if line.len() > self.config.max_request_size {
                        let error_msg = format!(
                            "ERROR: Request too large: {} bytes (max: {})\n",
                            line.len(),
                            self.config.max_request_size
                        );
                        let _ = writer.write_all(error_msg.as_bytes());
                        let _ = writer.flush();
                        continue;
                    }

                    // Plain text protocol - NO JSON!
                    // Direct command processing for zero overhead
                    let trimmed = line.trim();

                    match trimmed {
                        "getCode" | "getCacheCode" | "getPhpCode" => {
                            // Return raw PHP code directly (CRITICAL: No JSON wrapper!)
                            match self.generate_php_code() {
                                Ok(code) => {
                                    if let Err(e) = writer.write_all(code.as_bytes()) {
                                        warn!(error = %e, "IPC write error");
                                        break;
                                    }
                                    if let Err(e) = writer.flush() {
                                        warn!(error = %e, "IPC flush error");
                                        break;
                                    }
                                },
                                Err(e) => {
                                    let error_msg =
                                        format!("ERROR: Failed to generate PHP code: {e}\n");
                                    let _ = writer.write_all(error_msg.as_bytes());
                                    let _ = writer.flush();
                                },
                            }
                        },
                        "getFilePath" => {
                            // Return file path as plain text
                            if self.strategy == CacheStrategy::File {
                                let path = self.config.output_path.to_string_lossy();
                                let _ = writer.write_all(path.as_bytes());
                                let _ = writer.write_all(b"\n");
                                let _ = writer.flush();
                            } else {
                                let _ = writer.write_all(b"ERROR: File strategy not available\n");
                                let _ = writer.flush();
                            }
                        },
                        "ping" => {
                            let _ = writer.write_all(b"PONG\n");
                            let _ = writer.flush();
                        },
                        "stats" => {
                            // Return plain text stats
                            let cache = self.cache.read().unwrap();
                            let stats = format!(
                                "total:{} strategy:{:?} uptime:{}\n",
                                cache.len(),
                                self.strategy,
                                self.start_time.elapsed().as_secs()
                            );
                            let _ = writer.write_all(stats.as_bytes());
                            let _ = writer.flush();
                        },
                        _ => {
                            // Unknown command - send error as plain text
                            let error_msg = format!("ERROR: Unknown command: {trimmed}\n");
                            let _ = writer.write_all(error_msg.as_bytes());
                            let _ = writer.flush();
                        },
                    }
                }
            },
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No connections, this is fine
            },
            Err(e) => {
                warn!(error = %e, "IPC socket error");
                // Don't crash on socket errors
            },
        }

        Ok(())
    }

    fn generate_php_code(&self) -> Result<String> {
        let cache = self.cache.read().unwrap();
        let metadata: Vec<_> = cache.values().cloned().collect();

        // Use existing writer to generate PHP code
        let temp_file = tempfile::NamedTempFile::new()?;
        write_php_cache(&metadata, temp_file.path(), self.config.pretty)?;

        let code = std::fs::read_to_string(temp_file.path())?;
        Ok(code)
    }
}
