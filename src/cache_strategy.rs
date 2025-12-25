use std::path::Path;
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStrategy {
    /// File-based cache (tmpfs/RAMDisk)
    File,
    /// PHP Stream Wrapper (in-memory via IPC)
    StreamWrapper,
}

/// Detect optimal cache strategy based on OS and filesystem
pub fn detect_cache_strategy(cache_dir: &Path) -> CacheStrategy {
    #[cfg(target_os = "windows")]
    {
        // Windows: check for RAMDisk
        if is_ramdisk(cache_dir) {
            info!(path = ?cache_dir, strategy = "File", "Detected RAMDisk, using file-based cache");
            return CacheStrategy::File;
        } else {
            info!(
                strategy = "StreamWrapper",
                "Windows without RAMDisk, using stream wrapper (zero SSD wear)"
            );
            return CacheStrategy::StreamWrapper;
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Unix: check for tmpfs
        if is_tmpfs(cache_dir) {
            info!(path = ?cache_dir, strategy = "File", "Detected tmpfs, using file-based cache");
            CacheStrategy::File
        } else {
            info!(
                strategy = "StreamWrapper",
                "tmpfs not detected, using stream wrapper"
            );
            CacheStrategy::StreamWrapper
        }
    }
}

#[cfg(target_os = "windows")]
fn is_ramdisk(path: &Path) -> bool {
    use std::process::Command;

    // Get drive letter
    let drive = match path.to_str().and_then(|s| s.chars().next()) {
        Some(c) if c.is_ascii_alphabetic() => c,
        _ => return false,
    };

    // Check via wmic
    let output = Command::new("wmic")
        .args([
            "logicaldisk",
            "where",
            &format!("DeviceID='{}:'", drive),
            "get",
            "MediaType,VolumeName",
        ])
        .output();

    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();

        // MediaType 11 = Removable media (often used for RAMDisk)
        // Or VolumeName contains "ram", "ramdisk", "imdisk"
        if stdout.contains("11")
            || stdout.contains("ram")
            || stdout.contains("imdisk")
            || stdout.contains("ramdisk")
        {
            return true;
        }
    }

    false
}

#[cfg(not(target_os = "windows"))]
fn is_tmpfs(path: &Path) -> bool {
    use std::process::Command;

    // Try df command
    let output = Command::new("df").arg("-T").arg(path).output();

    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);

        if stdout.to_lowercase().contains("tmpfs") {
            return true;
        }
    }

    // Fallback: check /proc/mounts (Linux)
    #[cfg(target_os = "linux")]
    {
        if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
            for line in mounts.lines() {
                if line.contains("tmpfs") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let mount_point = parts[1];
                        // Check if our path is under this tmpfs mount
                        if path.starts_with(mount_point) {
                            return true;
                        }
                    }
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_strategy_temp_dir() {
        let temp = std::env::temp_dir();
        let strategy = detect_cache_strategy(&temp);

        // Should detect some strategy
        assert!(matches!(
            strategy,
            CacheStrategy::File | CacheStrategy::StreamWrapper
        ));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_tmpfs_detection() {
        // /tmp is usually tmpfs on Linux/macOS
        let tmp = PathBuf::from("/tmp");
        let is_tmp_tmpfs = is_tmpfs(&tmp);

        // This might be true or false depending on system, just test it doesn't panic
        println!("Is /tmp tmpfs: {}", is_tmp_tmpfs);
    }
}
