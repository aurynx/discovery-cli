use aurynx::config::ConfigFile;
use aurynx::error::AurynxError;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

/// Test that config errors return AurynxError::Config
#[test]
fn test_config_error_type() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    // Create invalid config (zero file size)
    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{"paths": ["/tmp"], "output": "/tmp/cache.php", "max_file_size_mb": 0}}"#
    )
    .unwrap();

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(matches!(err, AurynxError::Config { .. }));
    assert!(
        err.to_string()
            .contains("max_file_size_mb must be greater than 0")
    );
}

/// Test that invalid JSON returns AurynxError::Json
#[test]
fn test_json_error_type() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    // Create malformed JSON
    let mut file = File::create(&config_path).unwrap();
    writeln!(file, r#"{{"paths": ["/tmp", "output": "/tmp/cache.php"}}"#).unwrap(); // Missing comma

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(matches!(err, AurynxError::Json { .. }));
    assert!(err.to_string().contains("JSON error"));
}

/// Test that missing config file returns AurynxError::Config
#[test]
fn test_config_not_found_error_type() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("nonexistent.json");

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(matches!(err, AurynxError::Config { .. }));
    assert!(err.to_string().contains("Config file not found"));
}

/// Test that IO errors are properly wrapped
#[test]
fn test_io_error_conversion() {
    use std::io;

    let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let aurynx_err = AurynxError::from(io_err);

    assert!(matches!(aurynx_err, AurynxError::Io { .. }));
}

/// Test helper methods for creating specific errors
#[test]
fn test_error_helper_methods() {
    use std::path::PathBuf;

    // Test file size error
    let err = AurynxError::file_size_error(
        PathBuf::from("large.php"),
        20 * 1024 * 1024,
        10 * 1024 * 1024,
    );
    assert!(matches!(err, AurynxError::FileSizeLimit { .. }));
    assert!(err.to_string().contains("exceeds size limit"));

    // Test parse error
    let err = AurynxError::parse_error(PathBuf::from("bad.php"), "Unexpected token");
    assert!(matches!(err, AurynxError::Parse { .. }));
    assert!(err.to_string().contains("Parse error"));

    // Test lock error
    let err = AurynxError::lock_error(PathBuf::from("/tmp/daemon.lock"), "Already locked");
    assert!(matches!(err, AurynxError::LockAcquisition { .. }));
    assert!(err.to_string().contains("Failed to acquire daemon lock"));
}

/// Test error chain with source()
#[test]
fn test_error_source_chain() {
    use std::error::Error;
    use std::io;

    let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let aurynx_err = AurynxError::io_error("Cannot write file", io_err);

    // Check that source is available
    assert!(aurynx_err.source().is_some());
    assert!(
        aurynx_err
            .source()
            .unwrap()
            .to_string()
            .contains("access denied")
    );
}

/// Test display formatting for various error types
#[test]
fn test_error_display_messages() {
    use std::path::PathBuf;

    let test_cases = vec![
        (
            AurynxError::config_error("Invalid setting"),
            "Configuration error: Invalid setting",
        ),
        (
            AurynxError::invalid_request_error("Bad query"),
            "Invalid IPC request: Bad query",
        ),
        (
            AurynxError::tree_sitter_error("Grammar error"),
            "Tree-sitter error: Grammar error",
        ),
        (
            AurynxError::daemon_running_error(1234, PathBuf::from("/tmp/daemon.sock")),
            "Daemon already running with PID 1234",
        ),
    ];

    for (err, expected_substring) in test_cases {
        let msg = err.to_string();
        assert!(
            msg.contains(expected_substring),
            "Expected '{}' to contain '{}'",
            msg,
            expected_substring
        );
    }
}

/// Test that validation errors return correct error type
#[test]
fn test_validation_error_types() {
    let temp_dir = TempDir::new().unwrap();

    // Test max_file_size_mb too large
    let config_path = temp_dir.path().join("aurynx1.json");
    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{"paths": ["/tmp"], "output": "/tmp/cache.php", "max_file_size_mb": 2000}}"#
    )
    .unwrap();

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, AurynxError::Config { .. }));

    // Test max_request_size too small
    let config_path = temp_dir.path().join("aurynx2.json");
    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{"paths": ["/tmp"], "output": "/tmp/cache.php", "max_request_size": 100}}"#
    )
    .unwrap();

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, AurynxError::Config { .. }));
}
