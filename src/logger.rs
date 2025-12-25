use anyhow::{Context, Result};
use std::path::Path;
use std::sync::OnceLock;
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

static LOGGER_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Initialize structured logging with tracing
pub fn init_logger(
    log_file: Option<&Path>,
    log_level: &str,
    log_format: &str,
    verbose: bool,
) -> Result<()> {
    // Parse log level
    let level = match log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => {
            eprintln!("⚠️  Invalid log level '{log_level}', using 'info'");
            Level::INFO
        }
    };

    // Override with verbose mode
    let actual_level = if verbose { Level::DEBUG } else { level };

    // Create env filter
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(format!("aurynx={actual_level}")))
        .context("Failed to create log filter")?;

    // Setup logger based on format
    match log_format.to_lowercase().as_str() {
        "json" => {
            if let Some(path) = log_file {
                // JSON to file
                let file_appender = tracing_appender::rolling::never(
                    path.parent().unwrap_or_else(|| Path::new(".")),
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("discovery.log"),
                );
                let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt::layer().json().with_writer(non_blocking))
                    .try_init()?;

                // Keep guard alive (store in static)
                let _ = LOGGER_GUARD.set(_guard);
            } else {
                // JSON to stdout
                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt::layer().json())
                    .try_init()?;
            }
        }
        _ => {
            if let Some(path) = log_file {
                // Text to file
                let file_appender = tracing_appender::rolling::never(
                    path.parent().unwrap_or_else(|| Path::new(".")),
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("discovery.log"),
                );
                let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

                tracing_subscriber::registry()
                    .with(filter)
                    .with(
                        fmt::layer()
                            .with_writer(non_blocking)
                            .with_target(false)
                            .with_thread_ids(false),
                    )
                    .try_init()?;

                // Keep guard alive (store in static)
                let _ = LOGGER_GUARD.set(_guard);
            } else {
                // Text to stdout (default)
                tracing_subscriber::registry()
                    .with(filter)
                    .with(
                        fmt::layer()
                            .with_target(false)
                            .with_thread_ids(false)
                            .compact(),
                    )
                    .try_init()?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_logger_twice_does_not_panic() {
        // First init
        let _ = init_logger(None, "debug", "text", false);

        // Second init - should return error but not panic
        let res = init_logger(None, "debug", "text", false);
        assert!(res.is_err());
    }
}
