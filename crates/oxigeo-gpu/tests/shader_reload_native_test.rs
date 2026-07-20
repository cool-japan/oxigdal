//! Integration tests for the native filesystem-polling shader hot-reload
//! backend ([`oxigeo_gpu::shader_reload_native`]).
//!
//! All tests are gated behind the `shader-hot-reload` feature and only compile
//! when that feature is enabled (e.g. `--features shader-hot-reload`).
//!
//! Modification detection is made robust against coarse filesystem `mtime`
//! granularity by explicitly advancing the file's modification time with
//! [`std::fs::File::set_modified`] (stable since Rust 1.75) rather than relying
//! on wall-clock advancement between writes.
#![cfg(feature = "shader-hot-reload")]
// Tests use `.expect()` for clear failure messages; the workspace warns on
// `expect_used`, so opt out here (mirrors the existing GPU integration tests).
#![allow(clippy::expect_used)]

use std::fs::File;
use std::io::Write;
use std::time::{Duration, SystemTime};

use oxigeo_gpu::shader_reload::ShaderWatcher;
use oxigeo_gpu::shader_reload_native::{FilesystemPoller, PolledChangeKind, read_shader_source};
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Create an isolated temporary directory for a test.
fn temp_dir() -> TempDir {
    TempDir::new().expect("failed to create temp dir")
}

/// Write `contents` to `path`, fully flushing to disk.
fn write_file(path: &std::path::Path, contents: &str) {
    let mut file = File::create(path).expect("failed to create file");
    file.write_all(contents.as_bytes())
        .expect("failed to write file");
    file.flush().expect("failed to flush file");
}

/// Advance a file's modification time to `now + 2s` so that the change is
/// detectable regardless of filesystem `mtime` granularity.
fn bump_mtime(path: &std::path::Path) {
    let future = SystemTime::now() + Duration::from_secs(2);
    let file = File::options()
        .write(true)
        .open(path)
        .expect("failed to open file for mtime bump");
    file.set_modified(future)
        .expect("failed to set modification time");
}

// ── 1. construction ───────────────────────────────────────────────────────────

#[test]
fn test_filesystem_poller_new_default_interval() {
    // A freshly-constructed poller watches nothing; force_poll yields no events.
    let mut poller = FilesystemPoller::with_default_interval();
    assert!(poller.watched_paths().is_empty());
    assert!(poller.force_poll().is_empty());

    // Explicit interval constructor is also usable.
    let explicit = FilesystemPoller::new(Duration::from_millis(500));
    assert!(explicit.watched_paths().is_empty());
}

// ── 2. register seeds table ─────────────────────────────────────────────────

#[test]
fn test_register_path_seeds_mtime_table() {
    let dir = temp_dir();
    let path = dir.path().join("seed.wgsl");
    write_file(&path, "@compute fn main() {}");

    let mut poller = FilesystemPoller::with_default_interval();
    poller
        .register_path(&path)
        .expect("register should succeed for existing file");

    let watched = poller.watched_paths();
    assert_eq!(watched.len(), 1);
    assert_eq!(watched[0], path);

    // Immediately polling an unchanged file produces nothing.
    assert!(poller.force_poll().is_empty());
}

// ── 3. register missing → io error ──────────────────────────────────────────

#[test]
fn test_register_path_nonexistent_returns_io_error() {
    let dir = temp_dir();
    let missing = dir.path().join("does_not_exist.wgsl");

    let mut poller = FilesystemPoller::with_default_interval();
    let err = poller
        .register_path(&missing)
        .expect_err("registering a missing file must error");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert!(poller.watched_paths().is_empty());
}

// ── 4. deregister removes ───────────────────────────────────────────────────

#[test]
fn test_deregister_path_removes_entry() {
    let dir = temp_dir();
    let path = dir.path().join("drop.wgsl");
    write_file(&path, "fn main() {}");

    let mut poller = FilesystemPoller::with_default_interval();
    poller
        .register_path(&path)
        .expect("register should succeed");
    assert_eq!(poller.watched_paths().len(), 1);

    assert!(
        poller.deregister_path(&path),
        "first deregister returns true"
    );
    assert!(poller.watched_paths().is_empty());
    assert!(
        !poller.deregister_path(&path),
        "second deregister returns false"
    );
}

// ── 5. watched_paths sorted ─────────────────────────────────────────────────

#[test]
fn test_watched_paths_returns_sorted_list() {
    let dir = temp_dir();
    let names = ["c.wgsl", "a.wgsl", "b.wgsl"];
    let mut paths = Vec::new();
    for name in names {
        let path = dir.path().join(name);
        write_file(&path, "fn main() {}");
        paths.push(path);
    }

    let mut poller = FilesystemPoller::with_default_interval();
    // Register out of alphabetical order.
    poller.register_path(&paths[0]).expect("register c");
    poller.register_path(&paths[1]).expect("register a");
    poller.register_path(&paths[2]).expect("register b");

    let watched = poller.watched_paths();
    let mut expected = paths.clone();
    expected.sort();
    assert_eq!(watched, expected);
    // Verify the result is genuinely sorted ascending.
    assert!(watched.windows(2).all(|w| w[0] <= w[1]));
}

// ── 6. modification ─────────────────────────────────────────────────────────

#[test]
fn test_poll_detects_modification() {
    let dir = temp_dir();
    let path = dir.path().join("mod.wgsl");
    write_file(&path, "@compute fn v1() {}");

    let mut poller = FilesystemPoller::with_default_interval();
    poller
        .register_path(&path)
        .expect("register should succeed");

    // Rewrite and bump mtime forward so the change is observable.
    write_file(&path, "@compute fn v2() {}");
    bump_mtime(&path);

    let changes = poller.force_poll();
    assert_eq!(changes.len(), 1, "expected exactly one change");
    assert_eq!(changes[0].path, path);
    assert_eq!(changes[0].kind, PolledChangeKind::Modified);

    // A subsequent poll with no further change is empty (table was updated).
    assert!(poller.force_poll().is_empty());
}

// ── 7. deletion ─────────────────────────────────────────────────────────────

#[test]
fn test_poll_detects_deletion() {
    let dir = temp_dir();
    let path = dir.path().join("gone.wgsl");
    write_file(&path, "fn main() {}");

    let mut poller = FilesystemPoller::with_default_interval();
    poller
        .register_path(&path)
        .expect("register should succeed");

    std::fs::remove_file(&path).expect("failed to remove file");

    let changes = poller.force_poll();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, path);
    assert_eq!(changes[0].kind, PolledChangeKind::Deleted);

    // The path was dropped after the deletion event — no repeat event.
    assert!(poller.watched_paths().is_empty());
    assert!(poller.force_poll().is_empty());
}

// ── 8. creation of previously-unknown path ──────────────────────────────────

#[test]
fn test_poll_detects_creation_of_previously_unknown_path() {
    let dir = temp_dir();
    let path = dir.path().join("appears_later.wgsl");

    let mut poller = FilesystemPoller::with_default_interval();
    // Cannot register a missing file; track it as expected instead.
    poller.track_expected(&path);
    assert_eq!(poller.watched_paths().len(), 1);
    // While still missing, polling yields nothing.
    assert!(poller.force_poll().is_empty());

    // Now create the file.
    write_file(&path, "@compute fn main() {}");

    let changes = poller.force_poll();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, path);
    assert_eq!(changes[0].kind, PolledChangeKind::Created);

    // After creation it behaves like any tracked file: no spurious repeats.
    assert!(poller.force_poll().is_empty());
}

// ── 9. throttling ───────────────────────────────────────────────────────────

#[test]
fn test_poll_throttles_to_interval() {
    let dir = temp_dir();
    let path = dir.path().join("throttle.wgsl");
    write_file(&path, "fn main() {}");

    // One-second interval so the second poll falls inside the window.
    let mut poller = FilesystemPoller::new(Duration::from_secs(1));
    poller
        .register_path(&path)
        .expect("register should succeed");

    // Prime the throttle clock with a forced poll.
    let _ = poller.force_poll();

    // Modify the file, but the throttled poll should ignore it (within window).
    write_file(&path, "fn main_v2() {}");
    bump_mtime(&path);

    let throttled = poller.poll();
    assert!(
        throttled.is_empty(),
        "poll within the throttle window must return empty"
    );

    // Forcing a poll still sees the pending modification.
    let forced = poller.force_poll();
    assert_eq!(forced.len(), 1);
    assert_eq!(forced[0].kind, PolledChangeKind::Modified);
}

// ── 10. force bypasses throttle ─────────────────────────────────────────────

#[test]
fn test_force_poll_bypasses_throttle() {
    let dir = temp_dir();
    let path = dir.path().join("force.wgsl");
    write_file(&path, "fn main() {}");

    let mut poller = FilesystemPoller::new(Duration::from_secs(60));
    poller
        .register_path(&path)
        .expect("register should succeed");

    // First force_poll primes the clock; immediate second poll() is throttled.
    let _ = poller.force_poll();
    assert!(
        poller.poll().is_empty(),
        "poll is throttled by long interval"
    );

    // But repeated force_poll always runs, even inside a 60s window.
    write_file(&path, "fn main_v2() {}");
    bump_mtime(&path);
    let forced = poller.force_poll();
    assert_eq!(forced.len(), 1);
    assert_eq!(forced[0].kind, PolledChangeKind::Modified);
}

// ── 11. ShaderWatcher round-trip ────────────────────────────────────────────

#[test]
fn test_shader_watcher_poll_filesystem_round_trip() {
    let dir = temp_dir();
    let path = dir.path().join("pipeline.wgsl");
    write_file(&path, "@compute fn main() {}");
    let label = path.to_string_lossy().into_owned();

    // Register the source under its path-string label and seed the poller.
    let initial_wgsl = read_shader_source(&path).expect("read initial source");
    let mut watcher = ShaderWatcher::new(100);
    watcher.add_inline(label.clone(), initial_wgsl);
    watcher.add_path(label.clone());

    let mut poller = FilesystemPoller::with_default_interval();
    poller
        .register_path(&path)
        .expect("register should succeed");

    assert_eq!(watcher.source_version(&label), Some(1));

    // Edit the shader on disk and bump its mtime.
    write_file(&path, "@compute fn main_v2() {}\n@vertex fn vs() {}");
    bump_mtime(&path);

    let events = watcher.poll_filesystem(&mut poller);
    assert_eq!(events.len(), 1, "expected one shader change event");
    assert_eq!(events[0].label, label);
    assert_eq!(events[0].old_version, 1);
    assert_eq!(events[0].new_version, 2);

    // The in-memory source reflects the new content and version.
    assert_eq!(watcher.source_version(&label), Some(2));
    let src = watcher.get_source(&label).expect("source should exist");
    assert!(src.wgsl_source.contains("main_v2"));
    assert!(src.entry_points.iter().any(|e| e.name == "vs"));
}

// ── 12. BOM stripping ───────────────────────────────────────────────────────

#[test]
fn test_read_shader_source_strips_bom() {
    let dir = temp_dir();

    // File with a leading UTF-8 BOM.
    let bom_path = dir.path().join("bom.wgsl");
    write_file(&bom_path, "\u{FEFF}@compute fn main() {}");
    let read_bom = read_shader_source(&bom_path).expect("read bom file");
    assert!(
        !read_bom.starts_with('\u{FEFF}'),
        "BOM must be stripped from the front"
    );
    assert_eq!(read_bom, "@compute fn main() {}");

    // File without a BOM is returned verbatim.
    let plain_path = dir.path().join("plain.wgsl");
    write_file(&plain_path, "@compute fn main() {}");
    let read_plain = read_shader_source(&plain_path).expect("read plain file");
    assert_eq!(read_plain, "@compute fn main() {}");
}

// ── 13. missing label handled gracefully ────────────────────────────────────

#[test]
fn test_shader_watcher_poll_filesystem_handles_missing_label() {
    let dir = temp_dir();
    let path = dir.path().join("orphan.wgsl");
    write_file(&path, "@compute fn main() {}");

    // Watcher has NO source registered under this path's label.
    let mut watcher = ShaderWatcher::new(100);
    watcher.add_inline("unrelated_label", "@compute fn other() {}");

    let mut poller = FilesystemPoller::with_default_interval();
    poller
        .register_path(&path)
        .expect("register should succeed");

    // Trigger a modification on the orphan file.
    write_file(&path, "@compute fn main_v2() {}");
    bump_mtime(&path);

    // The polled change has no matching label → skipped, no events, no panic.
    let events = watcher.poll_filesystem(&mut poller);
    assert!(events.is_empty(), "unmatched paths must be skipped");

    // The unrelated source is untouched.
    assert_eq!(watcher.source_version("unrelated_label"), Some(1));
}
