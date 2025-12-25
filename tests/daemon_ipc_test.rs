use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_ipc_large_request_error_is_text() {
    // Setup test environment
    let temp_dir = TempDir::new().unwrap();
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();

    // Create a dummy PHP file
    std::fs::write(src_dir.join("Test.php"), "<?php class Test {}").unwrap();

    let output = temp_dir.path().join("cache.php");
    let socket = temp_dir.path().join("daemon.sock");
    let pid_file = temp_dir.path().join("daemon.pid");

    // Find binary
    let binary = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("debug")
        .join("aurynx");

    // Start daemon
    let mut child = Command::new("cargo")
        .args([
            "run",
            "--",
            "discovery:scan",
            "--path",
            src_dir.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--socket",
            socket.to_str().unwrap(),
            "--pid",
            pid_file.to_str().unwrap(),
            "--watch",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start daemon");

    // Wait for daemon to start
    let mut attempts = 0;
    while !socket.exists() && attempts < 50 {
        thread::sleep(Duration::from_millis(100));
        attempts += 1;
    }

    if !socket.exists() {
        child.kill().ok();
        panic!("Daemon failed to start (socket not found)");
    }

    // Connect to socket
    let mut stream = UnixStream::connect(&socket).expect("Failed to connect to socket");

    // Send a large request (larger than default buffer, usually 8KB or similar,
    // but let's check config. Default max_request_size is likely small enough to trigger)
    // In daemon.rs, we need to check what the default is.
    // Assuming it's reasonably small or we can trigger it with a huge string.
    // Let's send 1MB of data.
    let large_data = "A".repeat(1024 * 1024);
    stream.write_all(large_data.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();

    // Read response
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap_or_default();

    // Kill daemon
    child.kill().ok();

    // Verify response is TEXT error, not JSON
    println!("Response: {}", response);

    assert!(
        response.starts_with("ERROR:"),
        "Response should start with ERROR:"
    );
    assert!(
        !response.contains("{"),
        "Response should not contain JSON braces"
    );
    assert!(
        !response.contains("\"type\""),
        "Response should not contain JSON type field"
    );
}
