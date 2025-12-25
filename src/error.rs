use std::fmt;
use std::io;
use std::path::PathBuf;

/// Custom error type for Aurynx library operations
#[derive(Debug)]
pub enum AurynxError {
    /// IO errors (file read/write, socket operations)
    Io { context: String, source: io::Error },

    /// Configuration errors (invalid config, validation failures)
    Config { message: String },

    /// Parser errors (tree-sitter failures, syntax errors)
    Parse { file: PathBuf, message: String },

    /// File size limit exceeded
    FileSizeLimit {
        file: PathBuf,
        size: u64,
        limit: u64,
    },

    /// Daemon lock errors
    LockAcquisition { lock_path: PathBuf, reason: String },

    /// Daemon already running
    DaemonAlreadyRunning { pid: u32, socket_path: PathBuf },

    /// Invalid IPC request
    InvalidRequest { message: String },

    /// JSON serialization/deserialization errors
    Json {
        context: String,
        source: serde_json::Error,
    },

    /// Tree-sitter language errors
    TreeSitter { message: String },

    /// Watcher errors (notify library)
    Watcher {
        context: String,
        source: notify::Error,
    },

    /// Generic error with context (for migration from anyhow)
    Other { message: String },
}

impl fmt::Display for AurynxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { context, source } => {
                write!(f, "{context}: {source}")
            }
            Self::Config { message } => {
                write!(f, "Configuration error: {message}")
            }
            Self::Parse { file, message } => {
                write!(f, "Parse error in {}: {}", file.display(), message)
            }
            Self::FileSizeLimit { file, size, limit } => {
                write!(
                    f,
                    "File {} ({:.2}MB) exceeds size limit of {:.2}MB",
                    file.display(),
                    *size as f64 / 1024.0 / 1024.0,
                    *limit as f64 / 1024.0 / 1024.0
                )
            }
            Self::LockAcquisition { lock_path, reason } => {
                write!(
                    f,
                    "Failed to acquire daemon lock at {}: {}",
                    lock_path.display(),
                    reason
                )
            }
            Self::DaemonAlreadyRunning { pid, socket_path } => {
                write!(
                    f,
                    "Daemon already running with PID {} (socket: {})",
                    pid,
                    socket_path.display()
                )
            }
            Self::InvalidRequest { message } => {
                write!(f, "Invalid IPC request: {message}")
            }
            Self::Json { context, source } => {
                write!(f, "JSON error in {context}: {source}")
            }
            Self::TreeSitter { message } => {
                write!(f, "Tree-sitter error: {message}")
            }
            Self::Watcher { context, source } => {
                write!(f, "File watcher error in {context}: {source}")
            }
            Self::Other { message } => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for AurynxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::Watcher { source, .. } => Some(source),
            _ => None,
        }
    }
}

// Conversions from standard error types
impl From<io::Error> for AurynxError {
    fn from(err: io::Error) -> Self {
        Self::Io {
            context: "IO operation failed".to_string(),
            source: err,
        }
    }
}

impl From<serde_json::Error> for AurynxError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json {
            context: "JSON operation failed".to_string(),
            source: err,
        }
    }
}

impl From<notify::Error> for AurynxError {
    fn from(err: notify::Error) -> Self {
        Self::Watcher {
            context: "File watcher operation failed".to_string(),
            source: err,
        }
    }
}

impl From<anyhow::Error> for AurynxError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other {
            message: err.to_string(),
        }
    }
}

// Helper methods for creating errors with context
impl AurynxError {
    pub fn io_error(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub fn config_error(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    pub fn parse_error(file: PathBuf, message: impl Into<String>) -> Self {
        Self::Parse {
            file,
            message: message.into(),
        }
    }

    #[must_use] 
    pub const fn file_size_error(file: PathBuf, size: u64, limit: u64) -> Self {
        Self::FileSizeLimit { file, size, limit }
    }

    pub fn lock_error(lock_path: PathBuf, reason: impl Into<String>) -> Self {
        Self::LockAcquisition {
            lock_path,
            reason: reason.into(),
        }
    }

    #[must_use] 
    pub const fn daemon_running_error(pid: u32, socket_path: PathBuf) -> Self {
        Self::DaemonAlreadyRunning { pid, socket_path }
    }

    pub fn invalid_request_error(message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            message: message.into(),
        }
    }

    pub fn json_error(context: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Json {
            context: context.into(),
            source,
        }
    }

    pub fn tree_sitter_error(message: impl Into<String>) -> Self {
        Self::TreeSitter {
            message: message.into(),
        }
    }

    pub fn watcher_error(context: impl Into<String>, source: notify::Error) -> Self {
        Self::Watcher {
            context: context.into(),
            source,
        }
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::Other {
            message: message.into(),
        }
    }
}

/// Result type alias for Aurynx operations
pub type Result<T> = std::result::Result<T, AurynxError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn test_error_display() {
        let err = AurynxError::config_error("Invalid log level");
        assert_eq!(err.to_string(), "Configuration error: Invalid log level");

        let err = AurynxError::file_size_error(
            PathBuf::from("test.php"),
            15 * 1024 * 1024,
            10 * 1024 * 1024,
        );
        assert!(err.to_string().contains("exceeds size limit"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let aurynx_err = AurynxError::from(io_err);

        assert!(matches!(aurynx_err, AurynxError::Io { .. }));
        assert!(aurynx_err.to_string().contains("IO operation failed"));
    }

    #[test]
    fn test_error_source() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let aurynx_err = AurynxError::io_error("Cannot read file", io_err);

        assert!(aurynx_err.source().is_some());
    }

    #[test]
    fn test_helper_methods() {
        let err = AurynxError::parse_error(PathBuf::from("test.php"), "Syntax error at line 10");
        assert!(matches!(err, AurynxError::Parse { .. }));

        let err = AurynxError::lock_error(
            PathBuf::from("/tmp/daemon.lock"),
            "Lock held by another process",
        );
        assert!(matches!(err, AurynxError::LockAcquisition { .. }));
    }
}
