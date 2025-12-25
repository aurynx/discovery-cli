use aurynx::config::ConfigFile;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

/// Test default values when config fields are not set
#[test]
fn test_default_limit_values() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    // Create minimal config
    let mut file = File::create(&config_path).unwrap();
    writeln!(file, r#"{{"paths": ["/tmp"], "output": "/tmp/cache.php"}}"#).unwrap();

    let config = ConfigFile::load(Some(config_path)).unwrap();

    // Check default values
    assert_eq!(config.max_file_size_bytes(), 10 * 1024 * 1024); // 10MB
    assert_eq!(config.max_request_size_bytes(), 1024); // 1KB
    assert_eq!(config.max_cache_entries_limit(), 50_000); // 50k
}

/// Test custom limit values from config
#[test]
fn test_custom_limit_values() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    // Create config with custom limits
    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{
        "paths": ["/tmp"],
        "output": "/tmp/cache.php",
        "max_file_size_mb": 20,
        "max_request_size": 2048,
        "max_cache_entries": 100000
    }}"#
    )
    .unwrap();

    let config = ConfigFile::load(Some(config_path)).unwrap();

    // Check custom values
    assert_eq!(config.max_file_size_bytes(), 20 * 1024 * 1024); // 20MB
    assert_eq!(config.max_request_size_bytes(), 2048); // 2KB
    assert_eq!(config.max_cache_entries_limit(), 100_000); // 100k
}

/// Test validation: max_file_size_mb must be > 0
#[test]
fn test_validation_file_size_zero() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{
        "paths": ["/tmp"],
        "output": "/tmp/cache.php",
        "max_file_size_mb": 0
    }}"#
    )
    .unwrap();

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("max_file_size_mb must be greater than 0"));
}

/// Test validation: max_file_size_mb must be <= 1024
#[test]
fn test_validation_file_size_too_large() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{
        "paths": ["/tmp"],
        "output": "/tmp/cache.php",
        "max_file_size_mb": 2000
    }}"#
    )
    .unwrap();

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("max_file_size_mb too large"));
}

/// Test validation: max_request_size must be >= 256
#[test]
fn test_validation_request_size_too_small() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{
        "paths": ["/tmp"],
        "output": "/tmp/cache.php",
        "max_request_size": 100
    }}"#
    )
    .unwrap();

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("max_request_size too small"));
}

/// Test validation: max_request_size must be <= 1MB
#[test]
fn test_validation_request_size_too_large() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{
        "paths": ["/tmp"],
        "output": "/tmp/cache.php",
        "max_request_size": 2000000
    }}"#
    )
    .unwrap();

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("max_request_size too large"));
}

/// Test validation: max_cache_entries must be > 0
#[test]
fn test_validation_cache_entries_zero() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{
        "paths": ["/tmp"],
        "output": "/tmp/cache.php",
        "max_cache_entries": 0
    }}"#
    )
    .unwrap();

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("max_cache_entries must be greater than 0"));
}

/// Test validation: max_cache_entries must be <= 1M
#[test]
fn test_validation_cache_entries_too_large() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{
        "paths": ["/tmp"],
        "output": "/tmp/cache.php",
        "max_cache_entries": 2000000
    }}"#
    )
    .unwrap();

    let result = ConfigFile::load(Some(config_path));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("max_cache_entries too large"));
}

/// Test that limits at boundary values are accepted
#[test]
fn test_boundary_values_accepted() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    // Test max allowed values
    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{
        "paths": ["/tmp"],
        "output": "/tmp/cache.php",
        "max_file_size_mb": 1024,
        "max_request_size": 1048576,
        "max_cache_entries": 1000000
    }}"#
    )
    .unwrap();

    let config = ConfigFile::load(Some(config_path)).unwrap();

    assert_eq!(config.max_file_size_bytes(), 1024 * 1024 * 1024); // 1GB
    assert_eq!(config.max_request_size_bytes(), 1048576); // 1MB
    assert_eq!(config.max_cache_entries_limit(), 1_000_000); // 1M
}

/// Test that min allowed values are accepted
#[test]
fn test_min_boundary_values_accepted() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("aurynx.json");

    let mut file = File::create(&config_path).unwrap();
    writeln!(
        file,
        r#"{{
        "paths": ["/tmp"],
        "output": "/tmp/cache.php",
        "max_file_size_mb": 1,
        "max_request_size": 256,
        "max_cache_entries": 1
    }}"#
    )
    .unwrap();

    let config = ConfigFile::load(Some(config_path)).unwrap();

    assert_eq!(config.max_file_size_bytes(), 1024 * 1024); // 1MB
    assert_eq!(config.max_request_size_bytes(), 256); // 256B
    assert_eq!(config.max_cache_entries_limit(), 1); // 1
}
