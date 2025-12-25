use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

/// Test raw text IPC protocol (no JSON parsing overhead)
#[test]
#[ignore] // Integration test - run manually
fn test_raw_ipc_get_cache_code() {
    // Create temp directories
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("test.sock");
    let pid_file = temp_dir.path().join("test.pid");
    let output_path = temp_dir.path().join("cache.php");

    // Create test PHP file
    let test_file = temp_dir.path().join("TestClass.php");
    std::fs::write(
        &test_file,
        r#"<?php
namespace App\Models;

#[Entity]
class User {
    #[Column]
    public string $name;
}
"#,
    )
    .unwrap();

    // Start daemon
    let mut daemon = Command::new("cargo")
        .args([
            "run",
            "--",
            "discovery:scan",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
            "--socket",
            socket_path.to_str().unwrap(),
            "--pid",
            pid_file.to_str().unwrap(),
            "--watch",
        ])
        .spawn()
        .expect("Failed to start daemon");

    // Wait for daemon to start and socket to be ready
    thread::sleep(Duration::from_secs(2));

    // Test raw text protocol: getCacheCode
    let stream = UnixStream::connect(&socket_path);
    if stream.is_err() {
        daemon.kill().ok();
        panic!("Failed to connect - daemon may not have started");
    }

    let mut stream = stream.unwrap();

    // Send raw text command (no JSON!)
    stream.write_all(b"getCacheCode\n").unwrap();
    stream.flush().unwrap();

    // Read response - should be raw PHP code
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut response = String::new();
    reader.read_line(&mut response).unwrap();

    // Verify it's PHP code (not JSON)
    assert!(
        response.starts_with("<?php"),
        "Expected raw PHP code, got: {}",
        response
    );
    assert!(
        !response.contains(r#""type":"#),
        "Response should not be JSON, got: {}",
        response
    );
    assert!(
        response.contains("declare(strict_types=1)"),
        "Missing declare statement"
    );

    // Test ping command
    stream.write_all(b"ping\n").unwrap();
    stream.flush().unwrap();

    let mut pong = String::new();
    reader.read_line(&mut pong).unwrap();
    assert_eq!(pong.trim(), "PONG", "Ping should return PONG");

    // Test unknown command
    stream.write_all(b"unknown\n").unwrap();
    stream.flush().unwrap();

    let mut error = String::new();
    reader.read_line(&mut error).unwrap();
    assert!(error.starts_with("ERROR:"), "Should return error message");

    // Cleanup
    drop(stream);
    daemon.kill().ok();
}

/// Test JSON protocol still works (backward compatibility)
#[test]
#[ignore] // Integration test - run manually
fn test_json_ipc_still_works() {
    use serde_json::json;

    // Create temp directories
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("test.sock");
    let pid_file = temp_dir.path().join("test.pid");
    let output_path = temp_dir.path().join("cache.php");

    // Create test PHP file
    let test_file = temp_dir.path().join("TestClass.php");
    std::fs::write(
        &test_file,
        r#"<?php
namespace App;

#[Route]
class Controller {}
"#,
    )
    .unwrap();

    // Start daemon
    let mut daemon = Command::new("cargo")
        .args([
            "run",
            "--",
            "discovery:scan",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
            "--socket",
            socket_path.to_str().unwrap(),
            "--pid",
            pid_file.to_str().unwrap(),
            "--watch",
        ])
        .spawn()
        .expect("Failed to start daemon");

    // Wait for daemon to start
    thread::sleep(Duration::from_millis(500));

    // Test JSON protocol
    let mut stream = UnixStream::connect(&socket_path).expect("Failed to connect to socket");

    // Send JSON request
    let request = json!({"action": "getCacheCode"});
    let request_str = serde_json::to_string(&request).unwrap();
    stream.write_all(request_str.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();

    // Read JSON response
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut response = String::new();
    reader.read_line(&mut response).unwrap();

    let json_response: serde_json::Value = serde_json::from_str(&response).unwrap();

    // Verify JSON structure
    assert_eq!(json_response["type"], "phpCode", "Should have type field");
    assert!(
        json_response["code"].is_string(),
        "Should have code field with string"
    );

    let php_code = json_response["code"].as_str().unwrap();
    assert!(php_code.starts_with("<?php"), "Code should be PHP");

    // Cleanup
    drop(stream);
    daemon.kill().ok();
}
