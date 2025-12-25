# AGENTS.md - Aurynx Discovery CLI

## Project Overview

This is a **Rust CLI tool** for PHP attribute discovery using tree-sitter parsing. It provides:

- Fast PHP code analysis without runtime reflection
- Daemon mode with file watching and IPC protocol
- Zero-overhead cache generation (plain PHP arrays)

**Key technologies:** Rust 1.70+, tree-sitter, notify (file watching), serde (serialization).

## Repository Structure

```
src/
  ├── main.rs           # CLI entry point (clap commands)
  ├── lib.rs            # Public API
  ├── parser.rs         # Tree-sitter PHP parsing logic
  ├── scanner.rs        # File discovery and scanning
  ├── writer.rs         # Cache file generation (PHP/JSON)
  ├── daemon.rs         # File watcher and IPC server
  ├── incremental.rs    # Change detection and partial rescans
  ├── cache_strategy.rs # Memory vs File storage strategies
  ├── metadata.rs       # Attribute metadata extraction
  └── daemon/
      └── lock.rs       # Advisory file locking (atomicity)

examples/              # Debug examples for testing specific features
tests/                 # Integration tests
```

## Building and Testing

### Quick Commands

```bash
# Format code (always run before committing)
cargo fmt

# Build debug version
cargo build

# Build release version (for production)
cargo build --release

# Run all tests
cargo test

# Run specific test
cargo test daemon_test

# Run with verbose output
cargo test -- --nocapture

# Run clippy linter
cargo clippy -- -D warnings

# Check without building
cargo check
```

### Pre-commit Checklist

Before committing, ensure:

1. `cargo fmt` - code is formatted
2. `cargo clippy -- -D warnings` - no linter warnings
3. `cargo test` - all tests pass
4. `cargo build --release` - release build succeeds

### Integration Tests

Integration tests are in `tests/` directory:

- `integration.rs` - main integration tests
- `daemon_test.rs` - daemon mode and IPC tests
- `file_size_limit_test.rs` - file size handling
- `panic_cleanup_test.rs` - error recovery

Run specific test file:

```bash
cargo test --test daemon_test
```

## Code Style Guidelines

### Rust Conventions

- **Use idiomatic Rust:** Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- **Error handling:** Use `Result<T, E>` and `?` operator; avoid `.unwrap()` in production code
- **Formatting:** Run `cargo fmt` before every commit
- **Clippy:** Address all `cargo clippy` warnings
- **Comments:** Use `///` for documentation, `//` for inline comments
- **Imports:** Group std, external crates, and local modules separately

### Naming Conventions

- **Types:** `PascalCase` (e.g., `CacheStrategy`, `DaemonLock`)
- **Functions/variables:** `snake_case` (e.g., `parse_file`, `cache_path`)
- **Constants:** `SCREAMING_SNAKE_CASE` (e.g., `DEFAULT_SOCKET_PATH`)
- **Lifetimes:** Short lowercase (e.g., `'a`, `'b`)

### Common Patterns

```rust
// Prefer ? operator over match for error propagation
let content = fs::read_to_string(path)?;

// Use if let for single pattern matching
if let Some(value) = optional_value {
    // ...
}

// Prefer iterator chains over for loops
let filtered: Vec<_> = items
    .iter()
    .filter(|x| x.is_valid())
    .collect();

// Use format! with inline variables (Rust 2021+)
let message = format!("Processing {path} with {count} items");
```

### Tree-sitter Specific

- Always handle `None` when accessing tree-sitter nodes
- Use `cursor.goto_first_child()` / `cursor.goto_next_sibling()` for traversal
- Extract byte ranges carefully with `node.byte_range()`
- UTF-8 validation: source text must be valid UTF-8

## Testing Instructions

### Unit Tests

Unit tests are colocated with source code using `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_attribute() {
        // Test implementation
    }
}
```

### Integration Tests

Integration tests create temporary PHP files using `tempfile` crate:

```bash
# Run all integration tests
cargo test --test integration

# Run specific test function
cargo test --test integration test_class_attributes

# Show test output
cargo test -- --nocapture
```

### Testing Daemon Mode

For daemon tests:

1. Ensure no other daemon instances are running
2. Clean up `/tmp/aurynx-discovery-*.lock` files if tests fail
3. Use `--test-threads=1` to avoid race conditions:

```bash
cargo test --test daemon_test -- --test-threads=1
```

### Adding New Tests

When adding features:

1. Add unit tests in the same file as the code
2. Add integration tests in `tests/` for end-to-end scenarios
3. Use `#[should_panic]` for expected error cases
4. Create temporary test files in tests if needed

## Commit Message Guidelines

cargo clippycargo clippyWe follow [Conventional Commits](https://www.conventionalcommits.org/).

### Format

```text
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Types

- **feat**: New feature (e.g., `feat: add json output support`)
- **fix**: Bug fix (e.g., `fix: resolve race condition in daemon`)
- **build**: Build system/dependencies (e.g., `build: update tree-sitter-php`)
- **chore**: Routine tasks, maintenance (e.g., `chore: release v0.2.0`)
- **refactor**: Code change that neither fixes a bug nor adds a feature
- **docs**: Documentation only changes
- **style**: Formatting, missing semi-colons, etc.
- **perf**: Performance improvements
- **test**: Adding or correcting tests
- **ci**: CI/CD configuration changes

### Rules

1. **Imperative Mood**: Use "add" not "added" or "adds".
   - ✅ `feat: add support for enums`
   - ❌ `feat: added support for enums`
2. **No Period**: Do not end the subject line with a period.
3. **Lowercase**: Use lowercase for the description.
   - ✅ `fix: prevent memory leak`
   - ❌ `fix: Prevent memory leak`
4. **Scope (Optional)**: Use for specific modules (`parser`, `daemon`, `cli`).
   - `feat(parser): support php 8.2 readonly classes`

### Breaking Changes

Indicate breaking changes with a `!` after the type or a footer.

```text
feat!: drop support for PHP 7.4
```

## Pull Request Guidelines

### Before Opening PR

1. Rebase on latest `main` (not merge)
2. Run full test suite: `cargo test`
3. Check formatting: `cargo fmt -- --check`
4. Run clippy: `cargo clippy -- -D warnings`
5. Update CHANGELOG.md if user-facing change
6. Update README.md for new features

### PR Template

**Title:** `feat: add JSON output format`

**Description:**

```markdown
## What changed?
- Added `--format json` flag
- Implemented JSON serialization for cache output
- Added pretty-print support with `--pretty` flag

## Why?
Enables integration with non-PHP tools (Node.js, Python, etc.)

## Breaking changes?
None. Default behavior unchanged (PHP format).

## Testing
- Added integration test: `test_json_output_format`
- Tested with `examples/test_all_cases.rs`
```

### Review Checklist

- [ ] Code follows Rust style guidelines
- [ ] All tests pass (`cargo test`)
- [ ] No clippy warnings (`cargo clippy`)
- [ ] Documentation updated (README.md, doc comments)
- [ ] CHANGELOG.md updated for user-facing changes
- [ ] Commit messages follow conventional format

## Project-Specific Conventions

### IPC Protocol

When modifying IPC protocol in `daemon.rs`:

1. **CRITICAL:** Use **Raw Text Protocol** only. JSON is strictly forbidden for performance reasons.
2. Commands must be plain text strings (e.g., `getCacheCode`).
3. Responses must be raw data (PHP code) or plain text errors (starting with `ERROR:`).
4. Test with `nc -U /tmp/discovery.sock` manually.
5. Document new commands in README.md.

**Forbidden:**

- `serde_json` serialization in IPC path
- Wrapping PHP code in JSON objects
- Structured error responses (use `ERROR: message` string)

### Cache Strategy

The daemon selects cache strategy automatically:

- Linux → `CacheStrategy::File` (tmpfs)
- macOS/Windows → `CacheStrategy::StreamWrapper` (memory)

When modifying:

- Test on both Linux and macOS if possible
- Ensure atomic writes (temp file + rename)
- Handle disk full errors gracefully

### File Locking

Advisory file locks in `daemon/lock.rs` prevent race conditions:

- Lock file: `/tmp/aurynx-discovery-{hash}.lock`
- Uses `flock(LOCK_EX | LOCK_NB)` for atomicity
- Health check via IPC ping verifies daemon is alive
- Auto-cleanup on daemon exit (crash or graceful)

**When modifying lock logic:**

- Test with 100+ concurrent daemon starts
- Verify stale lock detection works
- Never use `--force` flag in automated scripts

### PHP Code Generation

When modifying `writer.rs`:

- Always use `declare(strict_types=1)` in PHP output
- Escape backslashes in FQCNs: `\\App\\User`
- Use `var_export()` style for arrays (readable)
- Test with `php -l` to validate syntax

## Security Considerations

- **File size limits:** Check `docs/file_size_limit.md` before modifying scanner
- **Path traversal:** Validate all file paths before reading
- **Symlink attacks:** Use `fs::canonicalize()` for paths
- **Daemon permissions:** Lock files must be writable by daemon user
- **IPC security:** Unix sockets have filesystem permissions

## Common Development Tasks

### Adding a New CLI Command

1. Add command to `main.rs` using clap:

   ```rust
   #[derive(Parser)]
   enum Commands {
       #[command(name = "discovery:scan")]
       DiscoveryScan(DiscoveryScanArgs),

       #[command(name = "discovery:validate")]
       DiscoveryValidate(DiscoveryValidateArgs), // New command
   }
   ```

2. Add handler function in `lib.rs`
3. Add integration test in `tests/`
4. Update README.md with usage example

### Adding PHP Element Support

To add support for new PHP elements (e.g., constants):

1. Add parsing logic in `parser.rs`
2. Add metadata structure in `metadata.rs`
3. Update output format in `writer.rs`
4. Add integration tests in `tests/`
5. Update README.md "Supported Elements" section

### Debugging Tips

```bash
# Enable debug logging
RUST_LOG=debug cargo run -- discovery:scan --path src/ --output /tmp/cache.php

# Trace level (very verbose)
RUST_LOG=trace cargo run -- discovery:scan --path src/ --output /tmp/cache.php

# Run example for debugging
cargo run --example debug_tree -- path/to/file.php

# Use lldb/gdb for crashes
rust-lldb target/debug/aurynx
```

## Performance Optimization

- **Parallel scanning:** Uses `rayon` for multi-threaded file processing
- **Incremental mode:** Only rescans changed files (track by mtime + size)
- **Memory efficiency:** Stream large files instead of loading into memory
- **Caching:** Reuse tree-sitter parsers across files

**When optimizing:**

- Run benchmarks with `cargo bench` (if added)
- Profile with `cargo flamegraph` or `perf`
- Measure file I/O separately from parsing
- Test with large codebases (10k+ files)

## Documentation

### Structure & Naming

- **Root Directory:** Only `README.md` (user-facing) and `AGENTS.md` (AI/Dev context).
- **Docs Directory:** All detailed documentation goes into `docs/`.
- **Naming Convention:** Use **snake_case** for files in `docs/` (e.g., `docs/file_size_limit.md`).
- **Linking:**
  - Docs must link to relevant `examples/` (e.g., `[See example](../examples/debug_tree.rs)`).
  - Docs should cross-link to each other using relative paths.
  - `README.md` serves as the index for `docs/`.

### Key Files

- **User docs:** `README.md` (installation, usage, examples)
- **Architecture:** `docs/architecture.md` (design decisions)
- **API docs:** Inline `///` comments (generated with `cargo doc`)
- **Agent instructions:** This file (`AGENTS.md`)

### When to Update

Update documentation when:

- Adding new CLI flags
- Changing IPC protocol
- Modifying cache format
- Fixing critical bugs

## Release Process

1. Update version in `Cargo.toml`
2. Update CHANGELOG.md with release notes
3. Run full test suite: `cargo test --release`
4. Build release binaries:

   ```bash
   cargo build --release --target aarch64-apple-darwin
   cargo build --release --target x86_64-apple-darwin
   cargo build --release --target x86_64-unknown-linux-gnu
   ```

5. Create Git tag: `git tag v0.3.0`
6. Push to GitHub: `git push origin v0.3.0`
7. GitHub Actions will build and publish release artifacts

---

<p align="center">Crafted by Aurynx 🔮</p>
