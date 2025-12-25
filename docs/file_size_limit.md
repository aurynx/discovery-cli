# File Size Limit Documentation

## Overview

The Aurynx Discovery CLI implements a file size check to prevent Out of Memory (OOM) errors when processing large PHP files. This feature ensures that only files within a safe size limit are loaded into memory for parsing.

## Configuration

### Default Limit

```rust
/// Maximum file size allowed for parsing (10MB)
/// Files larger than this will be skipped to prevent OOM
pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
```

**Location**: `src/scanner.rs:7-10`

### Why 10MB?

- **Memory Safety**: PHP files larger than 10MB are extremely rare and usually indicate generated code or bundled assets
- **Performance**: Tree-sitter parsing of very large files can significantly impact performance
- **Practical Limit**: Most PHP frameworks have similar limits (e.g., Composer max file size)

## Implementation

### File Size Check Logic

The scanner performs a metadata check before reading the file:

```rust
match fs::metadata(path) {
    Ok(metadata) => {
        let file_size = metadata.len();
        if file_size > MAX_FILE_SIZE {
            warn!(
                "Skipping large file: {:?} ({:.2}MB exceeds limit of {:.2}MB)",
                path,
                file_size as f64 / 1024.0 / 1024.0,
                MAX_FILE_SIZE as f64 / 1024.0 / 1024.0
            );
            return WalkState::Continue;
        }
    }
    Err(e) => {
        warn!("Could not read metadata for {:?}: {}", path, e);
        return WalkState::Continue;
    }
}
```

### Where It's Applied

1. **`scan_directory()`** - Main directory scanning function
   - **Location**: `src/scanner.rs:57-75`
   - Used for initial project scans

2. **`scan_files()`** - Specific file scanning function
   - **Location**: `src/scanner.rs:103-121`
   - Used for incremental updates

## Behavior

### Files Within Limit (≤ 10MB)

✅ **Processed normally**

- File is read into memory
- Tree-sitter parses the content
- Metadata is extracted
- Results are included in output

### Files Over Limit (> 10MB)

⚠️ **Skipped with warning**

- File is NOT read into memory
- Warning is logged with actual size
- File is skipped completely
- No metadata is extracted

**Example Warning**:

```
WARN Skipping large file: "src/Generated.php" (15.32MB exceeds limit of 10.00MB)
```

### Exact Boundary (= 10MB)

✅ **Processed**

- Files exactly at 10MB are processed
- Only files strictly greater than 10MB are skipped

## Performance Impact

The file size check has **minimal performance impact**:

1. **Fast Metadata Read**: `fs::metadata()` only reads file system metadata (inode), not file contents
2. **No I/O**: No disk read is performed for large files
3. **Early Exit**: Prevents expensive parsing operations

## Security Benefits

### Protection Against OOM Attacks

**Scenario**: Attacker places a 1GB PHP file in watched directory

**Without Protection**:

```
1. Scanner finds file
2. fs::read_to_string() loads 1GB into memory
3. Process OOM - daemon crashes
4. Orphaned cache files, stale locks
```

**With Protection**:

```
1. Scanner finds file
2. fs::metadata() checks size
3. Size exceeds 10MB limit
4. Warning logged, file skipped
5. Daemon continues normally
```

### Protection Against Accidental Issues

- Generated PHP files from build tools
- Accidentally copied binary files with .php extension
- Bundled vendor files with inline data
- Minified/concatenated PHP files

## Testing

### Test Coverage

The implementation includes 8 comprehensive tests (`tests/file_size_limit_test.rs`):

1. **`test_small_file_processed`** - Normal small files are processed
2. **`test_large_file_skipped`** - Files > 10MB are skipped
3. **`test_file_at_max_size_boundary`** - Files exactly at 10MB are processed
4. **`test_file_just_over_limit`** - Files at 10MB + 1 byte are skipped
5. **`test_scan_files_with_large_file`** - `scan_files()` function respects limit
6. **`test_mixed_file_sizes`** - Correct behavior with mixed sizes
7. **`test_file_size_check_with_unreadable_file`** - Graceful error handling
8. **`test_many_small_files_performance`** - Performance validation

### Running Tests

```bash
# Run file size limit tests only
cargo test --test file_size_limit_test

# Run all tests
cargo test --all
```

## Configuration (Future Enhancement)

Currently, `MAX_FILE_SIZE` is a compile-time constant. A future enhancement could make it configurable:

```json
// aurynx.json
{
  "max_file_size_mb": 10,
  "paths": ["src/"],
  "output": "cache.php"
}
```

This would require:

1. Adding field to `ConfigFile` struct
2. Passing config to scanner functions
3. Updating tests

## Monitoring and Debugging

### Enable Verbose Logging

To see file size warnings:

```bash
# Watch mode with verbose logging
./aurynx discovery:scan --watch --verbose \
  --path src/ \
  --output cache.php \
  --socket /tmp/daemon.sock \
  --pid /tmp/daemon.pid
```

### Log Output Example

```
2025-12-25T10:30:15.123456Z  WARN Skipping large file: "vendor/generated/Bundle.php" (15.32MB exceeds limit of 10.00MB)
2025-12-25T10:30:15.123789Z  INFO Metadata crafted: 1234 classes discovered
```

### Metrics (Future)

Future versions could track:

- Number of skipped files
- Total size of skipped files
- Largest file encountered
- Distribution of file sizes

## Related Files

- **Implementation**: `src/scanner.rs`
- **Tests**: `tests/file_size_limit_test.rs`
- **Documentation**: [`code_review.md`](code_review.md) (Section: P1)

## Migration Notes

### Upgrading from Previous Versions

Previous versions did not check file size. After upgrading:

1. **No Breaking Changes**: All previously processed files < 10MB continue to work
2. **New Warnings**: Large files that were previously processed (slowly or causing issues) will now be skipped with warnings
3. **Improved Stability**: Daemon is more stable under edge cases

### Handling Legitimately Large Files

If you have legitimate PHP files > 10MB:

**Option 1**: Split the file into smaller modules

```php
// Instead of one 15MB file
// Split into multiple < 10MB files
```

**Option 2**: Exclude from scanning

```json
{
  "ignore": ["src/Generated.php"]
}
```

**Option 3**: Increase limit (requires recompilation)

```rust
// src/scanner.rs
pub const MAX_FILE_SIZE: u64 = 20 * 1024 * 1024; // 20MB
```

## Best Practices

1. **Monitor Warnings**: If you see file size warnings, investigate why files are so large
2. **Code Generation**: Ensure generated PHP files are reasonable size
3. **Vendor Files**: Use `.gitignore` and ignore patterns to exclude vendor directories
4. **Build Artifacts**: Don't place build artifacts in scanned directories

## FAQ

**Q: Why was my large generated file skipped?**
A: Files > 10MB are skipped to prevent memory issues. Consider splitting the file or excluding it from scanning.

**Q: Can I increase the limit?**
A: Currently, you need to modify `MAX_FILE_SIZE` in `src/scanner.rs` and recompile. Future versions may support config-based limits.

**Q: Does this affect incremental scanning?**
A: Yes, both `scan_directory()` and `scan_files()` respect the size limit.

**Q: What happens to attributes in skipped files?**
A: They are not discovered. The entire file is skipped.

**Q: Is this check fast?**
A: Yes, `fs::metadata()` only reads file system metadata, not the file contents. It's extremely fast.

## Changelog

### v0.2.0 (2025-12-25)

- ✅ Added `MAX_FILE_SIZE` constant (10MB)
- ✅ Implemented file size check in `scan_directory()`
- ✅ Implemented file size check in `scan_files()`
- ✅ Added 8 comprehensive tests
- ✅ Added warning logs for skipped files
- ✅ Updated [`code_review.md`](code_review.md) documentation

## Contributing

When modifying file size limit behavior:

1. Update tests in `tests/file_size_limit_test.rs`
2. Update this documentation
3. Update [`code_review.md`](code_review.md)
4. Ensure all tests pass: `cargo test --all`
5. Consider performance impact

## License

Same as main project (see LICENSE file).
