//! Integration tests for POSIX advisory file locking (`oxigdal_gpkg::locking`).
//!
//! # Why some tests fork
//!
//! POSIX `fcntl` record locks are owned *per process*, not per file
//! descriptor. Two locks taken inside the same process therefore never
//! contend — the kernel sees a single owner and grants the second request.
//! To observe genuine contention (e.g. an exclusive lock blocking a shared
//! one) the test must hold the first lock in a *separate* process. These tests
//! use `libc::fork`: the child acquires a lock and sleeps, while the parent
//! probes with [`GpkgFileLock::try_acquire`] and asserts the expected outcome.
//!
//! All tests are gated on `all(feature = "file-locking", unix)` to match the
//! module they exercise.
#![cfg(all(feature = "file-locking", unix))]
#![allow(unsafe_code)]
// `expect()` with descriptive messages is the desired failure signal in tests;
// the workspace `allow-expect-in-tests` only covers `#[test]` bodies, not the
// free helper functions in this integration-test crate.
#![allow(clippy::expect_used)]

use oxigdal_gpkg::locking::{GpkgFileLock, LockMode, lock_for_read, lock_for_write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Monotonic counter ensuring each test gets a unique temp file name even when
/// tests run concurrently within the same process.
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Creates a fresh, empty temp file and returns its path. The file lives in the
/// platform temp dir with a name unique to this process and call (per the
/// COOLJAPAN temporary-file policy).
fn make_temp_file() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut path = std::env::temp_dir();
    path.push(format!("oxigdal_gpkg_lock_{pid}_{id}.gpkg"));
    std::fs::write(&path, b"oxigdal-gpkg locking test fixture").expect("create temp lock fixture");
    path
}

/// Removes a temp file, ignoring any error (best-effort cleanup).
fn remove_temp_file(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

/// Sleep that is safe to call after `fork` (async-signal-safe).
fn child_sleep_ms(ms: u32) {
    // SAFETY: `usleep` is async-signal-safe and takes a plain integer; no
    // pointers are involved.
    unsafe {
        libc::usleep(ms * 1_000);
    }
}

/// Runs a fork-based contention scenario.
///
/// The child acquires `child_mode` on `path`, signals readiness by sleeping
/// briefly, then `_exit(0)`s. The parent waits for the child to grab the lock,
/// then `try_acquire`s `parent_mode` and hands the result to `assert_parent`.
/// The child is always reaped. If `fork` is unavailable the test is skipped.
fn run_fork_contention(
    path: &PathBuf,
    child_mode: LockMode,
    parent_mode: LockMode,
    assert_parent: impl FnOnce(Option<GpkgFileLock>),
) {
    // SAFETY: `fork` takes no arguments. The child branch restricts itself to
    // async-signal-safe-ish operations (file open, fcntl, usleep) and exits
    // via `_exit`, never returning to the test harness or running destructors.
    let pid = unsafe { libc::fork() };

    if pid < 0 {
        // fork failed (e.g. sandbox without process creation): skip gracefully.
        eprintln!("skipping fork-based test: fork() unavailable");
        return;
    }

    if pid == 0 {
        // ── Child ──
        // Acquire the lock and hold it across a sleep so the parent observes
        // contention, then terminate (which releases the lock).
        match GpkgFileLock::acquire(path, child_mode) {
            Ok(lock) => {
                child_sleep_ms(500);
                // Keep the lock alive until after the sleep.
                drop(lock);
                // SAFETY: `_exit` is async-signal-safe; terminates the child
                // without flushing the parent's buffers or running atexit
                // handlers.
                unsafe { libc::_exit(0) };
            }
            Err(_) => {
                // SAFETY: as above; non-zero status signals child-side failure.
                unsafe { libc::_exit(1) };
            }
        }
    }

    // ── Parent ──
    // Give the child time to acquire its lock before probing.
    std::thread::sleep(std::time::Duration::from_millis(100));

    let result = GpkgFileLock::try_acquire(path, parent_mode).expect("try_acquire syscall");
    assert_parent(result);

    // Reap the child to avoid a zombie, and confirm it acquired successfully.
    let mut status: libc::c_int = 0;
    // SAFETY: `status` is a valid, writable `c_int`; `waitpid` only writes
    // through the provided pointer for the duration of the call.
    let waited = unsafe { libc::waitpid(pid, &mut status as *mut libc::c_int, 0) };
    assert_eq!(waited, pid, "waitpid should reap the forked child");
    let child_exit = libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0;
    assert!(
        child_exit,
        "child should have acquired its lock and exited 0"
    );
}

// ── Single-process tests ──────────────────────────────────────────────────────

#[test]
fn test_lock_acquire_shared_succeeds() {
    let path = make_temp_file();
    let lock = GpkgFileLock::acquire(&path, LockMode::Shared).expect("acquire shared");
    assert!(lock.is_locked());
    assert_eq!(lock.mode(), LockMode::Shared);
    drop(lock);
    remove_temp_file(&path);
}

#[test]
fn test_lock_acquire_exclusive_succeeds() {
    let path = make_temp_file();
    let lock = GpkgFileLock::acquire(&path, LockMode::Exclusive).expect("acquire exclusive");
    assert!(lock.is_locked());
    assert_eq!(lock.mode(), LockMode::Exclusive);
    drop(lock);
    remove_temp_file(&path);
}

#[test]
fn test_lock_release_explicit_succeeds() {
    let path = make_temp_file();
    let mut lock = lock_for_write(&path).expect("acquire write");
    assert!(lock.is_locked());
    lock.release().expect("explicit release");
    assert!(!lock.is_locked());
    // Releasing again is a harmless no-op.
    lock.release().expect("idempotent release");
    assert!(!lock.is_locked());
    drop(lock);
    remove_temp_file(&path);
}

#[test]
fn test_lock_drop_releases_automatically() {
    let path = make_temp_file();
    {
        let lock = lock_for_write(&path).expect("acquire write in scope");
        assert!(lock.is_locked());
        // `lock` dropped here, releasing the advisory lock and closing the fd.
    }
    // A fresh acquisition must succeed once the previous lock is gone. (Within
    // a single process this would succeed regardless of release due to
    // per-process semantics, but it also confirms the descriptor was not
    // leaked into an unusable state.)
    let again = lock_for_write(&path).expect("re-acquire write after drop");
    assert!(again.is_locked());
    drop(again);
    remove_temp_file(&path);
}

// ── Fork-based contention tests ─────────────────────────────────────────────

#[test]
fn test_try_acquire_shared_when_held_shared_succeeds() {
    let path = make_temp_file();
    // Shared + shared are compatible: the parent probe should succeed.
    run_fork_contention(&path, LockMode::Shared, LockMode::Shared, |result| {
        let lock = result.expect("shared lock should be compatible with another shared lock");
        assert!(lock.is_locked());
        assert_eq!(lock.mode(), LockMode::Shared);
    });
    remove_temp_file(&path);
}

#[test]
fn test_try_acquire_exclusive_when_held_shared_returns_none() {
    let path = make_temp_file();
    // Exclusive conflicts with a held shared lock: probe should return None.
    run_fork_contention(&path, LockMode::Shared, LockMode::Exclusive, |result| {
        assert!(
            result.is_none(),
            "exclusive lock must not be granted while a shared lock is held"
        );
    });
    remove_temp_file(&path);
}

#[test]
fn test_try_acquire_exclusive_when_held_exclusive_returns_none() {
    let path = make_temp_file();
    run_fork_contention(&path, LockMode::Exclusive, LockMode::Exclusive, |result| {
        assert!(
            result.is_none(),
            "exclusive lock must not be granted while another exclusive lock is held"
        );
    });
    remove_temp_file(&path);
}

#[test]
fn test_try_acquire_shared_when_held_exclusive_returns_none() {
    let path = make_temp_file();
    run_fork_contention(&path, LockMode::Exclusive, LockMode::Shared, |result| {
        assert!(
            result.is_none(),
            "shared lock must not be granted while an exclusive lock is held"
        );
    });
    remove_temp_file(&path);
}

// ── Accessor / state tests ──────────────────────────────────────────────────

#[test]
fn test_lock_mode_accessor() {
    let path = make_temp_file();
    let shared = lock_for_read(&path).expect("acquire shared");
    assert_eq!(shared.mode(), LockMode::Shared);
    drop(shared);

    let exclusive = lock_for_write(&path).expect("acquire exclusive");
    assert_eq!(exclusive.mode(), LockMode::Exclusive);
    drop(exclusive);
    remove_temp_file(&path);
}

#[test]
fn test_lock_path_accessor() {
    let path = make_temp_file();
    let lock = lock_for_read(&path).expect("acquire shared");
    assert_eq!(lock.path(), path.as_path());
    drop(lock);
    remove_temp_file(&path);
}

#[test]
fn test_lock_is_locked_after_acquire() {
    let path = make_temp_file();
    let lock = lock_for_write(&path).expect("acquire write");
    assert!(lock.is_locked());
    drop(lock);
    remove_temp_file(&path);
}

#[test]
fn test_lock_is_not_locked_after_release() {
    let path = make_temp_file();
    let mut lock = lock_for_write(&path).expect("acquire write");
    assert!(lock.is_locked());
    lock.release().expect("release");
    assert!(!lock.is_locked());
    drop(lock);
    remove_temp_file(&path);
}
