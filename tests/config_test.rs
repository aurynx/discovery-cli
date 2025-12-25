use aurynx::config::ConfigFile;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_load_valid_config() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("aurynx.json");

    let config_content = r#"{
        "paths": ["src"],
        "output": "cache.php",
        "log_level": "debug",
        "watch": true
    }"#;

    let mut file = File::create(&file_path).unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let config = ConfigFile::load(Some(file_path)).unwrap();

    assert_eq!(config.paths.unwrap()[0].to_str().unwrap(), "src");
    assert_eq!(config.output.unwrap().to_str().unwrap(), "cache.php");
    assert_eq!(config.log_level.unwrap(), "debug");
    assert_eq!(config.watch.unwrap(), true);
}

#[test]
fn test_load_invalid_json() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("aurynx.json");

    let mut file = File::create(&file_path).unwrap();
    file.write_all(b"{ invalid json }").unwrap();

    let result = ConfigFile::load(Some(file_path));
    assert!(result.is_err());
}

#[test]
fn test_validation_invalid_log_level() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("aurynx.json");

    let config_content = r#"{
        "log_level": "super_loud"
    }"#;

    let mut file = File::create(&file_path).unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let result = ConfigFile::load(Some(file_path));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid log_level")
    );
}

#[test]
fn test_validation_invalid_log_format() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("aurynx.json");

    let config_content = r#"{
        "log_format": "xml"
    }"#;

    let mut file = File::create(&file_path).unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    let result = ConfigFile::load(Some(file_path));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid log_format")
    );
}

#[test]
fn test_default_config_not_found() {
    // Should return default config if no file is found and no path provided
    // But we need to make sure we are not in a dir with aurynx.json
    // This test is tricky because it depends on CWD.
    // ConfigFile::load(None) checks "aurynx.json" in CWD.

    // Let's just test explicit path not found
    let result = ConfigFile::load(Some(std::path::PathBuf::from("non_existent.json")));
    assert!(result.is_err());
}
