use aurynx::scanner::{DEFAULT_MAX_FILE_SIZE, scan_directory, scan_files};
use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;

/// Test that small files are processed normally
#[test]
fn test_small_file_processed() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create a small PHP file
    let small_file = root.join("Small.php");
    let mut f = File::create(&small_file).unwrap();
    writeln!(f, "<?php namespace App; class SmallClass {{}}").unwrap();

    let paths = vec![root.to_path_buf()];
    let results = scan_directory(&paths, &[]);

    assert_eq!(results.len(), 1, "Small file should be processed");
    assert_eq!(results[0].fqcn, "\\App\\SmallClass");
}

/// Test that large files (> DEFAULT_MAX_FILE_SIZE) are skipped
#[test]
fn test_large_file_skipped() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create a large PHP file (> 10MB)
    let large_file = root.join("Large.php");
    let mut f = File::create(&large_file).unwrap();

    // Write PHP header
    writeln!(f, "<?php namespace App;").unwrap();
    writeln!(f, "class LargeClass {{").unwrap();

    // Write enough data to exceed DEFAULT_MAX_FILE_SIZE
    let chunk_size = 1024 * 1024; // 1MB chunks
    let padding = "a".repeat(chunk_size);
    for i in 0..12 {
        // Write 12MB worth of comments
        writeln!(f, "// Padding {}: {}", i, padding).unwrap();
    }

    writeln!(f, "}}").unwrap();
    drop(f);

    // Verify file is actually large
    let metadata = fs::metadata(&large_file).unwrap();
    assert!(
        metadata.len() > DEFAULT_MAX_FILE_SIZE,
        "Test file should exceed DEFAULT_MAX_FILE_SIZE. Got: {} bytes, expected > {}",
        metadata.len(),
        DEFAULT_MAX_FILE_SIZE
    );

    // Create a normal-sized file to ensure scanning works
    let normal_file = root.join("Normal.php");
    let mut f = File::create(&normal_file).unwrap();
    writeln!(f, "<?php namespace App; class NormalClass {{}}").unwrap();

    let paths = vec![root.to_path_buf()];
    let results = scan_directory(&paths, &[]);

    // Should only contain the normal file, large file should be skipped
    assert_eq!(results.len(), 1, "Only normal file should be processed");
    assert_eq!(results[0].fqcn, "\\App\\NormalClass");

    // Verify large file is not in results
    let has_large = results.iter().any(|m| m.fqcn == "\\App\\LargeClass");
    assert!(!has_large, "Large file should be skipped");
}

/// Test exact boundary: file exactly at DEFAULT_MAX_FILE_SIZE
#[test]
fn test_file_at_max_size_boundary() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create file exactly at DEFAULT_MAX_FILE_SIZE
    let boundary_file = root.join("Boundary.php");
    let mut f = File::create(&boundary_file).unwrap();

    let php_code = "<?php namespace App; class BoundaryClass {}";
    write!(f, "{}", php_code).unwrap();

    // Fill remaining space to reach DEFAULT_MAX_FILE_SIZE
    let current_size = php_code.len() as u64;
    let remaining = DEFAULT_MAX_FILE_SIZE - current_size;
    let padding = vec![b' '; remaining as usize];
    f.write_all(&padding).unwrap();
    drop(f);

    // Verify file is exactly at DEFAULT_MAX_FILE_SIZE
    let metadata = fs::metadata(&boundary_file).unwrap();
    assert_eq!(
        metadata.len(),
        DEFAULT_MAX_FILE_SIZE,
        "File should be exactly DEFAULT_MAX_FILE_SIZE"
    );

    let paths = vec![root.to_path_buf()];
    let results = scan_directory(&paths, &[]);

    // File at exactly DEFAULT_MAX_FILE_SIZE should be processed (not exceeding)
    assert_eq!(
        results.len(),
        1,
        "File at DEFAULT_MAX_FILE_SIZE should be processed"
    );
    assert_eq!(results[0].fqcn, "\\App\\BoundaryClass");
}

/// Test file just over the limit (DEFAULT_MAX_FILE_SIZE + 1 byte)
#[test]
fn test_file_just_over_limit() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create file just over DEFAULT_MAX_FILE_SIZE
    let over_file = root.join("OverLimit.php");
    let mut f = File::create(&over_file).unwrap();

    let php_code = "<?php namespace App; class OverClass {}";
    write!(f, "{}", php_code).unwrap();

    // Fill to exceed DEFAULT_MAX_FILE_SIZE by 1 byte
    let current_size = php_code.len() as u64;
    let target_size = DEFAULT_MAX_FILE_SIZE + 1;
    let remaining = target_size - current_size;
    let padding = vec![b' '; remaining as usize];
    f.write_all(&padding).unwrap();
    drop(f);

    // Verify file exceeds DEFAULT_MAX_FILE_SIZE
    let metadata = fs::metadata(&over_file).unwrap();
    assert!(
        metadata.len() > DEFAULT_MAX_FILE_SIZE,
        "File should exceed DEFAULT_MAX_FILE_SIZE by 1 byte"
    );

    let paths = vec![root.to_path_buf()];
    let results = scan_directory(&paths, &[]);

    // File over DEFAULT_MAX_FILE_SIZE should be skipped
    assert_eq!(
        results.len(),
        0,
        "File over DEFAULT_MAX_FILE_SIZE should be skipped"
    );
}

/// Test scan_files function with large files
#[test]
fn test_scan_files_with_large_file() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create a large file
    let large_file = root.join("Large.php");
    let mut f = File::create(&large_file).unwrap();
    writeln!(f, "<?php namespace App; class LargeClass {{").unwrap();

    // Write 11MB of data
    let chunk = "a".repeat(1024 * 1024);
    for _ in 0..11 {
        writeln!(f, "// {}", chunk).unwrap();
    }
    writeln!(f, "}}").unwrap();
    drop(f);

    // Create a normal file
    let normal_file = root.join("Normal.php");
    let mut f = File::create(&normal_file).unwrap();
    writeln!(f, "<?php namespace App; class NormalClass {{}}").unwrap();

    // Scan specific files
    let files = vec![large_file, normal_file];
    let results = scan_files(&files);

    // Should only contain the normal file
    assert_eq!(results.len(), 1, "Only normal file should be processed");
    assert_eq!(results[0].fqcn, "\\App\\NormalClass");
}

/// Test multiple files with mixed sizes
#[test]
fn test_mixed_file_sizes() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Small file (1KB)
    let small_file = root.join("Small.php");
    let mut f = File::create(&small_file).unwrap();
    writeln!(f, "<?php namespace App; class SmallClass {{}}").unwrap();
    drop(f);

    // Medium file (1MB)
    let medium_file = root.join("Medium.php");
    let mut f = File::create(&medium_file).unwrap();
    writeln!(f, "<?php namespace App; class MediumClass {{").unwrap();
    let padding = "a".repeat(1024 * 1024);
    writeln!(f, "// {}", padding).unwrap();
    writeln!(f, "}}").unwrap();
    drop(f);

    // Large file (15MB)
    let large_file = root.join("Large.php");
    let mut f = File::create(&large_file).unwrap();
    writeln!(f, "<?php namespace App; class LargeClass {{").unwrap();
    let chunk = "b".repeat(1024 * 1024);
    for _ in 0..15 {
        writeln!(f, "// {}", chunk).unwrap();
    }
    writeln!(f, "}}").unwrap();
    drop(f);

    let paths = vec![root.to_path_buf()];
    let results = scan_directory(&paths, &[]);

    // Should contain small and medium, but not large
    assert_eq!(
        results.len(),
        2,
        "Should process small and medium files only"
    );

    let fqcns: Vec<String> = results.iter().map(|m| m.fqcn.clone()).collect();
    assert!(fqcns.contains(&"\\App\\SmallClass".to_string()));
    assert!(fqcns.contains(&"\\App\\MediumClass".to_string()));
    assert!(!fqcns.contains(&"\\App\\LargeClass".to_string()));
}

/// Test that file size check doesn't affect error handling
#[test]
fn test_file_size_check_with_unreadable_file() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create a normal file
    let normal_file = root.join("Normal.php");
    let mut f = File::create(&normal_file).unwrap();
    writeln!(f, "<?php namespace App; class NormalClass {{}}").unwrap();
    drop(f);

    // Create a file path that doesn't exist
    let nonexistent_file = root.join("DoesNotExist.php");

    // scan_files should handle nonexistent files gracefully
    let files = vec![normal_file, nonexistent_file];
    let results = scan_files(&files);

    assert_eq!(results.len(), 1, "Should process only existing file");
    assert_eq!(results[0].fqcn, "\\App\\NormalClass");
}

/// Performance test: ensure size check is fast for many small files
#[test]
fn test_many_small_files_performance() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // Create 100 small files
    for i in 0..100 {
        let file = root.join(format!("Class{}.php", i));
        let mut f = File::create(&file).unwrap();
        writeln!(f, "<?php namespace App; class Class{} {{}}", i).unwrap();
    }

    let paths = vec![root.to_path_buf()];

    // This should complete quickly (metadata check is fast)
    let start = std::time::Instant::now();
    let results = scan_directory(&paths, &[]);
    let duration = start.elapsed();

    assert_eq!(results.len(), 100, "All small files should be processed");
    assert!(duration.as_secs() < 5, "Should complete within 5 seconds");
}
