use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_daemon_pid_file_creation() {
    // Setup test environment
    let temp_dir = TempDir::new().unwrap();
    let src_dir = temp_dir.path().join("src");
    fs::create_dir(&src_dir).unwrap();

    // Create a simple PHP file to watch
    let php_file = src_dir.join("Test.php");
    fs::write(&php_file, "<?php\nnamespace App;\nclass Test {}\n").unwrap();

    // Paths for daemon
    let output = temp_dir.path().join("cache.php");
    let socket = temp_dir.path().join("daemon.sock");
    let pid_file = temp_dir.path().join("daemon.pid");

    // Build path to the binary
    let binary = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("aurynx");

    // If binary doesn't exist, try debug build location
    let binary = if !binary.exists() {
        let debug_binary = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("debug")
            .join("aurynx");
        debug_binary
    } else {
        binary
    };

    println!("Using binary: {:?}", binary);

    // Start daemon
    let mut child = Command::new(&binary)
        .arg("discovery:scan")
        .arg("--path")
        .arg(&src_dir)
        .arg("--output")
        .arg(&output)
        .arg("--watch")
        .arg("--socket")
        .arg(&socket)
        .arg("--pid")
        .arg(&pid_file)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start daemon");

    // Wait for daemon to start and create PID file
    let mut pid_created = false;
    for _ in 0..20 {
        // Wait up to 2 seconds
        if pid_file.exists() {
            pid_created = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    assert!(pid_created, "PID file was not created within timeout");

    // Verify PID file content
    let pid_content = fs::read_to_string(&pid_file).expect("Failed to read PID file");
    let pid_from_file: u32 = pid_content
        .trim()
        .parse()
        .expect("PID file content is not a number");

    assert_eq!(
        pid_from_file,
        child.id(),
        "PID in file does not match actual process ID"
    );

    println!(
        "✅ PID file verified: {} matches process ID {}",
        pid_from_file,
        child.id()
    );

    // Kill daemon
    let _ = child.kill();

    // Wait for cleanup
    let _ = child.wait();
    thread::sleep(Duration::from_millis(500));

    // Verify PID file removal (optional, but good behavior)
    // Note: The daemon might not have enough time to clean up if killed forcefully,
    // but with SIGTERM it should try.
    if !pid_file.exists() {
        println!("✅ PID file removed after shutdown");
    } else {
        println!(
            "⚠️ PID file still exists after shutdown (might be expected depending on kill signal)"
        );
    }
}
