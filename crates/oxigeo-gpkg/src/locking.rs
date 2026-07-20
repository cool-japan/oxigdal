//! POSIX advisory file locking for GeoPackage files.
//!
//! This module provides advisory file locks built on the POSIX `fcntl`
//! interface (`F_SETLK` / `F_SETLKW` with `F_RDLCK` / `F_WRLCK` / `F_UNLCK`).
//! It is used to coordinate concurrent reads and exclusive writes against a
//! single `.gpkg` file across cooperating processes.
//!
//! # POSIX advisory semantics (read this before using)
//!
//! `fcntl`-based record locks have *per-process*, not per-file-descriptor,
//! ownership. This has two consequences that frequently surprise callers:
//!
//! 1. **Same-process locks never contend.** If a process already holds a lock
//!    on a region and then requests an overlapping lock (even through a
//!    different file descriptor on the same file), the kernel sees a single
//!    owner and the second request *succeeds*, silently replacing or merging
//!    with the first. Two locks taken inside one process therefore cannot be
//!    used to detect contention between themselves — genuine contention can
//!    only be observed across distinct processes.
//! 2. **Closing any descriptor to the file drops all of that process's locks
//!    on it.** Because ownership is per-process and keyed by inode, closing a
//!    *different* descriptor referring to the same file releases the lock. To
//!    avoid this footgun [`GpkgFileLock`] takes sole ownership of the
//!    descriptor it opens and closes it only in [`Drop`].
//!
//! This is **not** BSD `flock(2)`. BSD `flock` locks are associated with the
//! open file description (so they are inherited across `fork` and shared by
//! `dup`), they are always whole-file, and they do not interact with `fcntl`
//! record locks on most platforms. The two APIs must not be mixed on the same
//! file.
//!
//! # Whole-file locking
//!
//! Every lock created here covers the whole file: the underlying `flock`
//! request uses `l_whence = SEEK_SET`, `l_start = 0` and `l_len = 0`, where a
//! length of zero is the POSIX convention for "from the start offset to the
//! end of the file, including any future growth".

// Unsafe code is required to invoke the POSIX `fcntl` / `close` syscalls
// through `libc`; every call site documents its safety contract.
#![allow(unsafe_code)]

use crate::error::GpkgError;
use std::fs::OpenOptions;
use std::os::unix::io::{IntoRawFd, RawFd};
use std::path::{Path, PathBuf};

/// The kind of advisory lock to request on a GeoPackage file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// A shared (read) lock. Multiple processes may hold a shared lock on the
    /// same file simultaneously, but a shared lock excludes any exclusive
    /// lock. Maps to [`libc::F_RDLCK`].
    Shared,
    /// An exclusive (write) lock. At most one process may hold an exclusive
    /// lock, and it excludes all other shared and exclusive locks. Maps to
    /// [`libc::F_WRLCK`].
    Exclusive,
}

impl LockMode {
    /// Returns the `fcntl` lock-type constant corresponding to this mode.
    fn fcntl_type(self) -> libc::c_short {
        match self {
            LockMode::Shared => libc::F_RDLCK as libc::c_short,
            LockMode::Exclusive => libc::F_WRLCK as libc::c_short,
        }
    }
}

/// An advisory POSIX (`fcntl`) lock held on a GeoPackage file.
///
/// The lock is acquired when the value is constructed and released when it is
/// dropped (or explicitly via [`GpkgFileLock::release`]). The struct owns the
/// file descriptor it opened and is responsible for closing it; see the
/// [module documentation](self) for the per-process ownership caveats.
#[derive(Debug)]
pub struct GpkgFileLock {
    /// The owned file descriptor the lock was taken on. Closed in [`Drop`].
    fd: RawFd,
    /// The path the descriptor was opened from (for diagnostics).
    path: PathBuf,
    /// The lock mode this lock was acquired in.
    mode: LockMode,
    /// Whether the advisory lock is currently held.
    locked: bool,
}

impl GpkgFileLock {
    /// Acquires an advisory lock on `path`, blocking until it can be granted.
    ///
    /// The file must already exist; it is opened read/write for
    /// [`LockMode::Exclusive`] and read-only for [`LockMode::Shared`]. The
    /// lock covers the whole file. This uses the blocking `F_SETLKW` request,
    /// so the call will not return until the lock is granted or an error
    /// occurs.
    ///
    /// # Errors
    ///
    /// Returns [`GpkgError::LockingError`] if the file cannot be opened or the
    /// `fcntl` request fails (for example if the call is interrupted by a
    /// signal, or a deadlock is detected by the kernel).
    pub fn acquire<P: AsRef<Path>>(path: P, mode: LockMode) -> Result<Self, GpkgError> {
        let path = path.as_ref();
        let fd = open_fd(path, mode)?;

        let mut lock = build_flock(mode.fcntl_type());

        // SAFETY: `fd` is a valid descriptor we just obtained from
        // `into_raw_fd` and still exclusively own; `lock` is a fully
        // initialised `flock` whose address remains valid for the duration of
        // the call. `F_SETLKW` reads but does not retain the pointer.
        let rc = unsafe { libc::fcntl(fd, libc::F_SETLKW, &mut lock as *mut libc::flock) };
        if rc == -1 {
            let err = std::io::Error::last_os_error();
            close_fd(fd);
            return Err(GpkgError::LockingError(format!(
                "failed to acquire {mode:?} lock on {}: {err}",
                path.display()
            )));
        }

        Ok(Self {
            fd,
            path: path.to_path_buf(),
            mode,
            locked: true,
        })
    }

    /// Attempts to acquire an advisory lock on `path` without blocking.
    ///
    /// Behaves like [`GpkgFileLock::acquire`] but uses the non-blocking
    /// `F_SETLK` request. If the lock is currently held by another process in
    /// a conflicting mode the kernel reports `EAGAIN` (or, on some platforms,
    /// `EACCES`) and this returns `Ok(None)` after closing the descriptor it
    /// opened. Any other failure is returned as an error.
    ///
    /// # Errors
    ///
    /// Returns [`GpkgError::LockingError`] if the file cannot be opened, or if
    /// the `fcntl` request fails for a reason other than contention.
    pub fn try_acquire<P: AsRef<Path>>(path: P, mode: LockMode) -> Result<Option<Self>, GpkgError> {
        let path = path.as_ref();
        let fd = open_fd(path, mode)?;

        let mut lock = build_flock(mode.fcntl_type());

        // SAFETY: identical contract to `acquire`; `F_SETLK` is the
        // non-blocking variant and uses the pointer only for the duration of
        // the call.
        let rc = unsafe { libc::fcntl(fd, libc::F_SETLK, &mut lock as *mut libc::flock) };
        if rc == -1 {
            let err = std::io::Error::last_os_error();
            let raw = err.raw_os_error();
            // Always release the descriptor we opened: keeping it would leak
            // an fd and (because POSIX locks are per-process and per-inode)
            // could disturb other locks this process holds on the same file.
            close_fd(fd);

            if raw == Some(libc::EAGAIN) || raw == Some(libc::EACCES) {
                return Ok(None);
            }
            return Err(GpkgError::LockingError(format!(
                "failed to try-acquire {mode:?} lock on {}: {err}",
                path.display()
            )));
        }

        Ok(Some(Self {
            fd,
            path: path.to_path_buf(),
            mode,
            locked: true,
        }))
    }

    /// Releases the advisory lock if it is currently held.
    ///
    /// This issues an `F_UNLCK` request through the non-blocking `F_SETLK`
    /// command. It is idempotent: calling it on a lock that has already been
    /// released (or was never granted) is a no-op that returns `Ok(())`. The
    /// file descriptor stays open until the value is dropped, so the lock can
    /// not be re-acquired through this descriptor afterwards.
    ///
    /// # Errors
    ///
    /// Returns [`GpkgError::LockingError`] if the `fcntl` unlock request
    /// fails.
    pub fn release(&mut self) -> Result<(), GpkgError> {
        if !self.locked {
            return Ok(());
        }

        let mut lock = build_flock(libc::F_UNLCK as libc::c_short);

        // SAFETY: `self.fd` is a valid descriptor owned by this struct (not
        // yet closed because `Drop` has not run); `lock` is a fully
        // initialised `flock` valid for the call.
        let rc = unsafe { libc::fcntl(self.fd, libc::F_SETLK, &mut lock as *mut libc::flock) };
        if rc == -1 {
            let err = std::io::Error::last_os_error();
            return Err(GpkgError::LockingError(format!(
                "failed to release lock on {}: {err}",
                self.path.display()
            )));
        }

        self.locked = false;
        Ok(())
    }

    /// Returns the mode this lock was acquired in.
    #[must_use]
    pub fn mode(&self) -> LockMode {
        self.mode
    }

    /// Returns the path the lock was taken on.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns `true` while the advisory lock is held.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.locked
    }
}

impl Drop for GpkgFileLock {
    fn drop(&mut self) {
        // Best-effort unlock; errors during teardown cannot be propagated.
        let _ = self.release();
        close_fd(self.fd);
        self.locked = false;
    }
}

/// Acquires a shared (read) advisory lock on `path`, blocking until granted.
///
/// Convenience wrapper for `GpkgFileLock::acquire(path, LockMode::Shared)`.
///
/// # Errors
///
/// Returns [`GpkgError::LockingError`] if the file cannot be opened or the
/// lock cannot be acquired.
pub fn lock_for_read<P: AsRef<Path>>(path: P) -> Result<GpkgFileLock, GpkgError> {
    GpkgFileLock::acquire(path, LockMode::Shared)
}

/// Acquires an exclusive (write) advisory lock on `path`, blocking until
/// granted.
///
/// Convenience wrapper for `GpkgFileLock::acquire(path, LockMode::Exclusive)`.
///
/// # Errors
///
/// Returns [`GpkgError::LockingError`] if the file cannot be opened or the
/// lock cannot be acquired.
pub fn lock_for_write<P: AsRef<Path>>(path: P) -> Result<GpkgFileLock, GpkgError> {
    GpkgFileLock::acquire(path, LockMode::Exclusive)
}

/// Opens an owned file descriptor for `path` suitable for `mode`.
///
/// Exclusive locks require a writable descriptor (`O_RDWR`), shared locks only
/// a readable one (`O_RDONLY`). The file is never created — it must already
/// exist. Ownership of the descriptor is transferred to the caller, which must
/// close it.
fn open_fd(path: &Path, mode: LockMode) -> Result<RawFd, GpkgError> {
    let mut options = OpenOptions::new();
    match mode {
        LockMode::Shared => {
            options.read(true);
        }
        LockMode::Exclusive => {
            options.read(true).write(true);
        }
    }

    let file = options.open(path).map_err(|err| {
        GpkgError::LockingError(format!(
            "failed to open {} for {mode:?} locking: {err}",
            path.display()
        ))
    })?;

    // Take ownership of the raw fd; `file` is consumed so its destructor will
    // not close the descriptor out from under us.
    Ok(file.into_raw_fd())
}

/// Builds a whole-file [`libc::flock`] request of the given lock type.
///
/// `l_whence = SEEK_SET`, `l_start = 0`, `l_len = 0` selects the entire file,
/// including any bytes appended in the future.
fn build_flock(l_type: libc::c_short) -> libc::flock {
    // SAFETY: `flock` is a plain-old-data C struct; an all-zero bit pattern is
    // a valid initial value, and we immediately overwrite the meaningful
    // fields below.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = l_type;
    lock.l_whence = libc::SEEK_SET as libc::c_short;
    lock.l_start = 0;
    lock.l_len = 0;
    lock
}

/// Closes a raw file descriptor, ignoring any error.
///
/// Used during cleanup paths (failed acquisition, `Drop`) where an error from
/// `close` cannot be acted upon.
fn close_fd(fd: RawFd) {
    // SAFETY: `fd` is a descriptor this module obtained from `into_raw_fd` and
    // owns; it is closed exactly once because callers relinquish it here.
    unsafe {
        libc::close(fd);
    }
}
