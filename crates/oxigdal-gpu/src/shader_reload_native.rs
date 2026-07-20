//! Native filesystem-polling backend for shader hot-reload.
//!
//! The base [`crate::shader_reload::ShaderWatcher`] tracks shader sources by
//! in-memory version counters and never touches the filesystem.  This module
//! adds a real native polling backend, [`FilesystemPoller`], that watches a set
//! of paths on disk and reports modification / deletion / (re-)creation events
//! by comparing recorded modification timestamps (`mtime`).
//!
//! The poller is intentionally dependency-free — it relies only on `std` — and
//! is gated behind the `shader-hot-reload` feature so that embedded / WASM
//! builds that cannot poll a filesystem do not pay for it.
//!
//! # Throttling
//!
//! [`FilesystemPoller::poll`] is throttled to the configured interval so that a
//! tight render loop can call it every frame without hammering the filesystem.
//! [`FilesystemPoller::force_poll`] bypasses the throttle and is primarily used
//! for tests and for an explicit "reload now" command.
//!
//! # Created vs Modified
//!
//! `std::fs::metadata` cannot be read for a file that does not yet exist, so a
//! path cannot be *registered* before its file is created.  To still detect a
//! file that appears later (or reappears after deletion),
//! [`FilesystemPoller::track_expected`] records the path with a
//! [`SystemTime::UNIX_EPOCH`] sentinel.  When the file subsequently appears the
//! poller distinguishes the two cases as follows:
//!
//! * stored timestamp == `UNIX_EPOCH` and the file now exists → [`PolledChangeKind::Created`]
//! * stored timestamp is a real time and the on-disk `mtime` is newer → [`PolledChangeKind::Modified`]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Default polling interval used by [`FilesystemPoller::with_default_interval`].
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Classification of a change detected by [`FilesystemPoller`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolledChangeKind {
    /// The file's modification time advanced relative to the recorded value.
    Modified,
    /// The file is no longer accessible (deleted, moved, or permission lost).
    Deleted,
    /// A previously expected (or previously deleted) file has appeared.
    Created,
}

/// A single change reported by [`FilesystemPoller::poll`] / [`FilesystemPoller::force_poll`].
#[derive(Debug, Clone)]
pub struct PolledChange {
    /// The path whose state changed.
    pub path: PathBuf,
    /// What kind of change was observed.
    pub kind: PolledChangeKind,
}

/// Polls a set of filesystem paths for modification-time changes.
///
/// Each watched path is associated with the [`SystemTime`] of its last observed
/// modification.  On every (un-throttled) poll the current `mtime` of each path
/// is compared against the recorded value to produce [`PolledChange`] events.
///
/// See the [module documentation](self) for the throttling and
/// created-vs-modified semantics.
pub struct FilesystemPoller {
    /// Recorded modification time per watched path.
    mtimes: HashMap<PathBuf, SystemTime>,
    /// Minimum delay between throttled [`Self::poll`] invocations.
    poll_interval: Duration,
    /// Instant of the most recent (throttled or forced) poll, if any.
    last_poll: Option<Instant>,
}

impl FilesystemPoller {
    /// Create a poller with an explicit throttling interval.
    pub fn new(poll_interval: Duration) -> Self {
        Self {
            mtimes: HashMap::new(),
            poll_interval,
            last_poll: None,
        }
    }

    /// Create a poller using the default 250 ms throttling interval.
    pub fn with_default_interval() -> Self {
        Self::new(DEFAULT_POLL_INTERVAL)
    }

    /// Start watching `path`, seeding the modification-time table from the
    /// file's current `mtime`.
    ///
    /// # Errors
    ///
    /// Propagates any [`std::io::Error`] from reading the file's metadata or
    /// modification time — most commonly [`std::io::ErrorKind::NotFound`] when
    /// the file does not exist.  To watch a not-yet-created path use
    /// [`Self::track_expected`] instead.
    pub fn register_path<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        let path_ref = path.as_ref();
        let modified = std::fs::metadata(path_ref)?.modified()?;
        self.mtimes.insert(path_ref.to_path_buf(), modified);
        Ok(())
    }

    /// Track a path that may not exist yet.
    ///
    /// The path is recorded with a [`SystemTime::UNIX_EPOCH`] sentinel so that
    /// when the file later appears the next poll reports it as
    /// [`PolledChangeKind::Created`] (rather than [`PolledChangeKind::Modified`]).
    /// While the file is still absent the sentinel suppresses any
    /// [`PolledChangeKind::Deleted`] event — the path simply waits to appear.
    /// This also re-arms creation detection for a path that was previously
    /// dropped after a deletion.
    pub fn track_expected<P: AsRef<Path>>(&mut self, path: P) {
        self.mtimes
            .insert(path.as_ref().to_path_buf(), SystemTime::UNIX_EPOCH);
    }

    /// Stop watching `path`.
    ///
    /// Returns `true` if the path was being watched, `false` otherwise.
    pub fn deregister_path<P: AsRef<Path>>(&mut self, path: P) -> bool {
        self.mtimes.remove(path.as_ref()).is_some()
    }

    /// Return all currently watched paths, sorted ascending for deterministic
    /// diagnostics.
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.mtimes.keys().cloned().collect();
        paths.sort();
        paths
    }

    /// Number of paths currently being watched.
    pub fn watched_count(&self) -> usize {
        self.mtimes.len()
    }

    /// Poll for changes, honouring the configured throttling interval.
    ///
    /// If a previous poll occurred less than `poll_interval` ago this returns an
    /// empty vector without touching the filesystem.  Otherwise it records the
    /// current instant and runs the full comparison.
    pub fn poll(&mut self) -> Vec<PolledChange> {
        if let Some(last) = self.last_poll
            && last.elapsed() < self.poll_interval
        {
            return Vec::new();
        }
        self.last_poll = Some(Instant::now());
        self.diff_against_disk()
    }

    /// Poll for changes immediately, ignoring the throttling interval.
    ///
    /// The throttle clock is still updated so that a subsequent [`Self::poll`]
    /// is measured relative to this call.
    pub fn force_poll(&mut self) -> Vec<PolledChange> {
        self.last_poll = Some(Instant::now());
        self.diff_against_disk()
    }

    /// Core comparison shared by [`Self::poll`] and [`Self::force_poll`].
    ///
    /// Iterates the watched paths in sorted order (deterministic output),
    /// classifies each against its recorded `mtime`, then applies the resulting
    /// table mutations (timestamp bumps and deletions) after iteration to avoid
    /// borrowing conflicts.
    fn diff_against_disk(&mut self) -> Vec<PolledChange> {
        let mut ordered: Vec<PathBuf> = self.mtimes.keys().cloned().collect();
        ordered.sort();

        let mut changes = Vec::new();
        let mut updates: Vec<(PathBuf, SystemTime)> = Vec::new();
        let mut removals: Vec<PathBuf> = Vec::new();

        for path in ordered {
            let stored = match self.mtimes.get(&path) {
                Some(stored) => *stored,
                None => continue,
            };

            match std::fs::metadata(&path).and_then(|meta| meta.modified()) {
                Ok(current) => {
                    if stored == SystemTime::UNIX_EPOCH {
                        // A path tracked via `track_expected` whose file now
                        // exists: report creation and adopt the real mtime.
                        changes.push(PolledChange {
                            path: path.clone(),
                            kind: PolledChangeKind::Created,
                        });
                        updates.push((path, current));
                    } else if current > stored {
                        changes.push(PolledChange {
                            path: path.clone(),
                            kind: PolledChangeKind::Modified,
                        });
                        updates.push((path, current));
                    }
                }
                Err(_) => {
                    // Metadata unreadable (deleted / moved / permissions).
                    // A path still carrying the `track_expected` sentinel has
                    // never actually appeared, so its absence is the expected
                    // initial state — keep waiting rather than reporting a
                    // bogus deletion. Only a path with a real recorded mtime
                    // (i.e. one that genuinely existed) yields `Deleted`, and
                    // it is then dropped from the table.
                    if stored != SystemTime::UNIX_EPOCH {
                        changes.push(PolledChange {
                            path: path.clone(),
                            kind: PolledChangeKind::Deleted,
                        });
                        removals.push(path);
                    }
                }
            }
        }

        for (path, modified) in updates {
            self.mtimes.insert(path, modified);
        }
        for path in removals {
            self.mtimes.remove(&path);
        }

        changes
    }
}

/// Read a WGSL shader source from disk as UTF-8 text, stripping a leading
/// byte-order mark (`U+FEFF`) if one is present.
///
/// Some editors prepend a UTF-8 BOM; left in place it would appear inside the
/// first token of the shader and break compilation, so it is removed here.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] from reading the file (e.g. the file does
/// not exist or is not valid UTF-8).
pub fn read_shader_source<P: AsRef<Path>>(path: P) -> std::io::Result<String> {
    let contents = std::fs::read_to_string(path)?;
    match contents.strip_prefix('\u{FEFF}') {
        Some(stripped) => Ok(stripped.to_owned()),
        None => Ok(contents),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_interval_constant() {
        let poller = FilesystemPoller::with_default_interval();
        assert_eq!(poller.poll_interval, DEFAULT_POLL_INTERVAL);
        assert!(poller.last_poll.is_none());
        assert_eq!(poller.watched_count(), 0);
    }

    #[test]
    fn test_strip_bom_via_str() {
        // Sanity check of the BOM-stripping logic without filesystem access.
        let with_bom = "\u{FEFF}fn main() {}";
        assert_eq!(with_bom.strip_prefix('\u{FEFF}'), Some("fn main() {}"));
    }
}
