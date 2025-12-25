use aurynx::scanner::scan_directory;
use std::fs::{self, File};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_integration_scan_variations() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // 1. Simple attribute with namespace
    let simple_path = root.join("Simple.php");
    let mut f = File::create(&simple_path).unwrap();
    writeln!(f, "<?php namespace App; #[Attribute] class Simple {{}}").unwrap();

    // 2. No attribute
    let no_attr_path = root.join("NoAttr.php");
    let mut f = File::create(&no_attr_path).unwrap();
    writeln!(f, "<?php namespace App; class NoAttr {{}}").unwrap();

    // 3. Commented attribute (should be ignored)
    let commented_path = root.join("Commented.php");
    let mut f = File::create(&commented_path).unwrap();
    writeln!(
        f,
        "<?php namespace App; // #[Attribute]\nclass Commented {{}}"
    )
    .unwrap();

    // 4. String attribute (should be ignored)
    let string_path = root.join("String.php");
    let mut f = File::create(&string_path).unwrap();
    writeln!(
        f,
        "<?php namespace App; $s = '#[Attribute]'; class StringClass {{}}"
    )
    .unwrap();

    // 5. Complex/Multiline attribute
    let complex_path = root.join("Complex.php");
    let mut f = File::create(&complex_path).unwrap();
    writeln!(
        f,
        "<?php
namespace App;

#[
    Attribute,
    AnotherAttribute(1, 2)
]
class Complex {{}}"
    )
    .unwrap();

    // 6. Ignored file (via argument)
    let ignored_path = root.join("IgnoredFile.php");
    let mut f = File::create(&ignored_path).unwrap();
    writeln!(f, "<?php namespace App; #[Attribute] class Ignored {{}}").unwrap();

    // 7. Vendor file (should be ignored if we passed vendor to ignore)
    let vendor_dir = root.join("vendor");
    std::fs::create_dir(&vendor_dir).unwrap();
    let vendor_file = vendor_dir.join("Lib.php");
    let mut f = File::create(&vendor_file).unwrap();
    writeln!(f, "<?php namespace Vendor; #[Attribute] class Lib {{}}").unwrap();

    let paths = vec![root.to_path_buf()];
    let ignored = vec!["IgnoredFile.php".to_string(), "vendor/".to_string()];

    let results = scan_directory(&paths, &ignored);

    // Check results - all classes should be found (new behavior: we extract all classes)
    let result_fqcns: Vec<String> = results.iter().map(|m| m.fqcn.clone()).collect();

    assert!(
        result_fqcns.contains(&"\\App\\Simple".to_string()),
        "Simple class missing"
    );
    assert!(
        result_fqcns.contains(&"\\App\\Complex".to_string()),
        "Complex class missing"
    );
    // Now we also extract classes without attributes
    assert!(
        result_fqcns.contains(&"\\App\\NoAttr".to_string()),
        "NoAttr class should be included"
    );
    assert!(
        result_fqcns.contains(&"\\App\\StringClass".to_string()),
        "StringClass should be included"
    );

    // These should NOT be included (ignored)
    assert!(
        !result_fqcns.contains(&"\\App\\Ignored".to_string()),
        "Ignored class should be excluded"
    );
    assert!(
        !result_fqcns.contains(&"\\Vendor\\Lib".to_string()),
        "vendor/Lib class should be excluded"
    );

    // Verify metadata structure for one class
    let simple_meta = results.iter().find(|m| m.fqcn == "\\App\\Simple").unwrap();
    assert_eq!(simple_meta.kind, "class");
    // The Attribute class will be resolved to \App\Attribute since there's no use import
    assert!(simple_meta.attributes.contains_key("\\App\\Attribute"));
}

/// Test that multiple concurrent daemon start attempts result in only ONE running daemon
///
/// This test verifies the atomicity guarantee of DaemonLock by spawning 20 parallel
/// processes that all try to start the daemon simultaneously. Only one should succeed
/// in acquiring the lock, while others should exit cleanly with "already running" error.
///
/// This simulates the scenario where 100 concurrent PHP requests all try to start
/// the daemon at the same time.
#[test]
fn test_concurrent_daemon_startup_atomicity() {
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
    let pid = temp_dir.path().join("daemon.pid");
    let log_file = temp_dir.path().join("daemon.log");

    // Build path to the binary (cargo test compiles it)
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

    // Spawn 20 concurrent daemon processes
    const NUM_PROCESSES: usize = 20;
    let results = Arc::new(Mutex::new(Vec::new()));
    let mut handles = vec![];

    for i in 0..NUM_PROCESSES {
        let binary = binary.clone();
        let src_dir = src_dir.clone();
        let output = output.clone();
        let socket = socket.clone();
        let pid = pid.clone();
        let log_file = log_file.clone();
        let results = Arc::clone(&results);

        let handle = thread::spawn(move || {
            // Each process tries to start daemon
            let process_log = log_file.with_extension(format!("log.{}", i));

            let result = Command::new(&binary)
                .arg("discovery:scan")
                .arg("--path")
                .arg(&src_dir)
                .arg("--output")
                .arg(&output)
                .arg("--watch")
                .arg("--socket")
                .arg(&socket)
                .arg("--pid")
                .arg(&pid)
                .arg("--log-file")
                .arg(&process_log)
                .arg("--log-level")
                .arg("info")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn();

            match result {
                Ok(mut child) => {
                    // Wait a bit for daemon to start (or fail)
                    // The daemon has retry logic (up to ~1.5s) when checking for lock
                    // so we need to wait longer than that to ensure failed instances exit
                    thread::sleep(Duration::from_millis(5000));

                    // Check if process is still running
                    let still_running = match child.try_wait() {
                        Ok(Some(_status)) => false, // Exited
                        Ok(None) => true,           // Still running
                        Err(_) => false,            // Error checking
                    };

                    let mut stderr = String::new();

                    // If running, kill it to read stderr (we need debug info)
                    if still_running {
                        let _ = child.kill();
                        let _ = child.wait();
                    }

                    if let Some(ref mut pipe) = child.stderr {
                        use std::io::Read;
                        let _ = pipe.read_to_string(&mut stderr);
                    }

                    results
                        .lock()
                        .unwrap()
                        .push((i, still_running, stderr, child));
                }
                Err(e) => {
                    panic!("Failed to spawn process {}: {}", i, e);
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Give daemons a moment to stabilize
    thread::sleep(Duration::from_millis(500));

    // Analyze results
    let mut results = results.lock().unwrap();
    let running_count = results.iter().filter(|(_, running, _, _)| *running).count();
    let failed_count = results
        .iter()
        .filter(|(_, running, _, _)| !*running)
        .count();

    println!("\n=== Concurrent Daemon Startup Test Results ===");
    println!("Total processes spawned: {}", NUM_PROCESSES);
    println!("Still running (acquired lock): {}", running_count);
    println!("Exited early (lock held): {}", failed_count);

    // Debug: Print stderr of all processes if no daemon is running
    if running_count != 1 {
        println!("\n=== DEBUG: Process Status (Expected 1 running) ===");
        for (i, running, stderr, _) in results.iter() {
            println!("Process {}: Running={}, Stderr: {}", i, running, stderr);
        }
    }

    // Verify atomicity: exactly ONE daemon should be running
    assert_eq!(
        running_count, 1,
        "Expected exactly 1 daemon to be running, but {} are running",
        running_count
    );

    assert_eq!(
        failed_count,
        NUM_PROCESSES - 1,
        "Expected {} daemons to fail (lock held), but {} failed",
        NUM_PROCESSES - 1,
        failed_count
    );

    // Verify that failed processes exited due to lock being held
    for (i, running, stderr, _) in results.iter() {
        if !running {
            let stderr_lower = stderr.to_lowercase();
            assert!(
                stderr_lower.contains("already running")
                    || stderr_lower.contains("lock held")
                    || stderr_lower.contains("failed to acquire"),
                "Process {} exited but stderr doesn't mention lock conflict. Stderr: {}",
                i,
                stderr
            );
        }
    }

    // Cleanup: kill the running daemon
    for (_, running, _, child) in results.iter_mut() {
        if *running {
            // Kill the daemon process
            let _ = child.kill();
        }
    }
    drop(results);

    // Give it time to cleanup
    thread::sleep(Duration::from_millis(500));

    println!(
        "✅ Atomicity test passed: only 1 daemon ran despite {} concurrent attempts",
        NUM_PROCESSES
    );
}
