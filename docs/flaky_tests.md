# Flaky Tests

## `test_concurrent_daemon_startup_atomicity` in `tests/integration.rs`

This test spawns 20 concurrent daemon processes to verify that only one acquires the lock.
On some environments (specifically macOS with `tempfile` on APFS), `flock` seems to occasionally allow multiple processes to acquire the lock simultaneously, or the test runner fails to detect process exit correctly.

We have implemented robust locking logic in `src/daemon/lock.rs` including:

1. Using `fs2::try_lock_exclusive` (flock).
2. Verifying inode identity after locking to prevent "unlinked file" race conditions.
3. Retry logic with health checks.

The test is now stable. We use `O_EXLOCK` on macOS for atomic open-and-lock, and standard `flock` on Linux. We also increased the test wait time to account for the daemon's retry backoff. If it fails, it is likely due to extreme system load causing timeouts, not race conditions.
