//! Regression tests for cool-japan/oxigeo#14 — the I/O layer's per-block
//! allocation.
//!
//! `DataSource::read_range` returns an owned `Vec`, so a reader walking a band
//! paid one heap allocation *per block* before a single byte was decoded: 8000
//! allocations, 8000 `seek` calls and 8000 mutex round-trips for an 8000-strip
//! file. [`DataSource::read_range_into`] reads straight into a caller-owned
//! buffer, and `FileDataSource` now serves it with a positional read (`pread` /
//! `ReadFile` + `OVERLAPPED`) that needs neither the seek nor the lock.
//!
//! Everything asserted here is deterministic: byte-for-byte equivalence with
//! `read_range`, the error each malformed request produces, and cross-thread
//! correctness. Allocation counting lives in `issue_14_zero_alloc_io.rs`, which
//! needs a global allocator all to itself.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::env::temp_dir;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource, FileDataSource, MmapDataSource, MmapDataSourceRw};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 4 KiB of non-repeating bytes, enough to catch an off-by-one in either
/// direction.
fn payload() -> Vec<u8> {
    (0..4096u32).map(|i| (i % 251) as u8).collect()
}

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(temp_dir().join(format!(
            "oxigeo_issue14_io_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn write_temp_file(name: &str, data: &[u8]) -> TempPath {
    let path = TempPath::new(name);
    let mut f = fs::File::create(&path).expect("create temp file");
    f.write_all(data).expect("write temp file");
    f.flush().expect("flush temp file");
    path
}

/// A source that implements only the two required methods, so it exercises the
/// *default* `read_range_into` / `range_slice` bodies on the trait — the ones
/// every out-of-tree implementor inherits.
#[derive(Debug)]
struct DefaultImplSource(Vec<u8>);

impl DataSource for DefaultImplSource {
    fn size(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let start = range.start as usize;
        let end = range.end as usize;
        if start > end || end > self.0.len() {
            return Err(OxiGeoError::OutOfBounds {
                message: format!("range {start}..{end} outside {}-byte source", self.0.len()),
            });
        }
        Ok(self.0[start..end].to_vec())
    }
}

/// A source whose `read_range` clamps to its own end instead of failing, which is
/// how several in-tree sources behave. The default `read_range_into` must report
/// the clamped length rather than pretending the whole range arrived.
#[derive(Debug)]
struct ClampingSource(Vec<u8>);

impl DataSource for ClampingSource {
    fn size(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let start = (range.start as usize).min(self.0.len());
        let end = (range.end as usize).min(self.0.len());
        Ok(self.0[start..end].to_vec())
    }
}

/// Runs the whole equivalence battery against one source.
///
/// `len` is the source's own length; ranges are chosen relative to it so the same
/// battery works for a file, a mapping and an in-memory buffer.
fn assert_read_range_into_matches_read_range<S: DataSource>(label: &str, source: &S, data: &[u8]) {
    let len = data.len() as u64;

    // --- ordinary interior range ------------------------------------------
    for &(start, end) in &[
        (0u64, 1u64),
        (0, len),
        (17, 1000),
        (len - 1, len),
        (900, 900),
    ] {
        let range = ByteRange::new(start, end);
        let owned = source
            .read_range(range)
            .unwrap_or_else(|e| panic!("{label}: read_range({start}..{end}) failed: {e}"));

        let mut buf = vec![0xA5u8; owned.len().max((end - start) as usize)];
        let n = source
            .read_range_into(range, &mut buf)
            .unwrap_or_else(|e| panic!("{label}: read_range_into({start}..{end}) failed: {e}"));

        assert_eq!(
            n,
            owned.len(),
            "{label}: returned count must equal read_range's length for {start}..{end}"
        );
        assert_eq!(
            &buf[..n],
            owned.as_slice(),
            "{label}: bytes differ for {start}..{end}"
        );
        assert_eq!(
            &buf[..n],
            &data[start as usize..start as usize + n],
            "{label}: bytes differ from the ground truth for {start}..{end}"
        );
    }

    // --- empty range, empty destination -----------------------------------
    let mut nothing: [u8; 0] = [];
    let n = source
        .read_range_into(ByteRange::new(64, 64), &mut nothing)
        .unwrap_or_else(|e| panic!("{label}: empty range must succeed: {e}"));
    assert_eq!(n, 0, "{label}: empty range writes nothing");

    // --- destination longer than the range: tail untouched ----------------
    let range = ByteRange::new(10, 26);
    let mut roomy = vec![0x5Au8; 64];
    let n = source
        .read_range_into(range, &mut roomy)
        .unwrap_or_else(|e| panic!("{label}: oversized dst must be accepted: {e}"));
    assert_eq!(
        n, 16,
        "{label}: oversized dst still reports the range length"
    );
    assert_eq!(
        &roomy[..16],
        &data[10..26],
        "{label}: prefix must be filled"
    );
    assert!(
        roomy[16..].iter().all(|&b| b == 0x5A),
        "{label}: the tail beyond the range must not be touched"
    );

    // --- destination shorter than the range: rejected, dst untouched ------
    let mut cramped = [0xC3u8; 8];
    let err = source
        .read_range_into(ByteRange::new(0, 9), &mut cramped)
        .expect_err(&format!("{label}: undersized dst must be rejected"));
    assert!(
        matches!(err, OxiGeoError::InvalidParameter { .. }),
        "{label}: undersized dst must be an InvalidParameter error, got {err}"
    );
    assert_eq!(
        cramped, [0xC3u8; 8],
        "{label}: no I/O may happen when dst is too small"
    );
}

/// Both entry points must fail identically on a range that runs past the end.
fn assert_past_eof_errors_match<S: DataSource>(label: &str, source: &S, len: u64) {
    let range = ByteRange::new(len - 4, len + 64);
    let owned = source.read_range(range);
    let mut buf = vec![0u8; 68];
    let into = source.read_range_into(range, &mut buf);

    match (owned, into) {
        (Err(a), Err(b)) => assert_eq!(
            format!("{a}"),
            format!("{b}"),
            "{label}: the two entry points must report the same error past EOF"
        ),
        (Ok(a), Ok(n)) => assert_eq!(
            a.len(),
            n,
            "{label}: a clamping source must report the same short length both ways"
        ),
        (a, b) => panic!("{label}: read_range and read_range_into disagreed: {a:?} vs {b:?}"),
    }
}

// ---------------------------------------------------------------------------
// Equivalence, per implementor
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_read_range_into_matches_file_data_source() {
    let data = payload();
    let path = write_temp_file("file_equiv.bin", &data);
    let source = FileDataSource::open(&path).expect("open FileDataSource");

    assert_read_range_into_matches_read_range("FileDataSource", &source, &data);
    assert_past_eof_errors_match("FileDataSource", &source, data.len() as u64);

    // A file cannot lend out its bytes, so it must decline the zero-copy path.
    assert!(
        source.range_slice(ByteRange::new(0, 16)).is_none(),
        "FileDataSource must not claim to serve borrowed slices"
    );
}

#[test]
fn test_issue_14_read_range_into_matches_mmap_data_source() {
    let data = payload();
    let path = write_temp_file("mmap_equiv.bin", &data);
    let source = MmapDataSource::open(&path).expect("open MmapDataSource");

    assert_read_range_into_matches_read_range("MmapDataSource", &source, &data);
    assert_past_eof_errors_match("MmapDataSource", &source, data.len() as u64);
}

#[test]
fn test_issue_14_read_range_into_matches_mmap_rw_data_source() {
    let data = payload();
    let path = TempPath::new("mmap_rw_equiv.bin");
    {
        let mut rw = MmapDataSourceRw::create(&path, data.len()).expect("create MmapDataSourceRw");
        rw.write_at(0, &data).expect("fill mapping");
        rw.flush().expect("flush mapping");
    }
    let source = MmapDataSourceRw::open(&path).expect("open MmapDataSourceRw");

    assert_read_range_into_matches_read_range("MmapDataSourceRw", &source, &data);
    assert_past_eof_errors_match("MmapDataSourceRw", &source, data.len() as u64);
}

#[test]
fn test_issue_14_read_range_into_default_impl_matches_read_range() {
    let data = payload();
    let source = DefaultImplSource(data.clone());

    assert_read_range_into_matches_read_range("default impl", &source, &data);
    assert_past_eof_errors_match("default impl", &source, data.len() as u64);

    // The default `range_slice` declines, so every caller keeps its copying path.
    assert!(
        source.range_slice(ByteRange::new(0, 16)).is_none(),
        "the default range_slice must return None"
    );
}

#[test]
fn test_issue_14_read_range_into_reports_a_clamping_sources_short_read() {
    let data = payload();
    let source = ClampingSource(data.clone());
    let past_end = ByteRange::new(data.len() as u64 - 10, data.len() as u64 + 90);

    let owned = source.read_range(past_end).expect("clamped read_range");
    assert_eq!(owned.len(), 10, "the source clamps to its own end");

    let mut buf = vec![0xEEu8; 100];
    let n = source
        .read_range_into(past_end, &mut buf)
        .expect("clamped read_range_into");
    assert_eq!(
        n, 10,
        "the clamped length must be reported, not the range's"
    );
    assert_eq!(&buf[..10], owned.as_slice());
    assert!(
        buf[10..].iter().all(|&b| b == 0xEE),
        "bytes the source never produced must stay as the caller left them"
    );
}

// ---------------------------------------------------------------------------
// range_slice (T3)
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_mmap_range_slice_is_zero_copy_and_declines_bad_ranges() {
    let data = payload();
    let path = write_temp_file("mmap_slice.bin", &data);
    let source = MmapDataSource::open(&path).expect("open MmapDataSource");

    let range = ByteRange::new(128, 256);
    let borrowed = source
        .range_slice(range)
        .expect("a mapping must serve an in-bounds range without copying");
    assert_eq!(borrowed, &data[128..256]);
    assert_eq!(
        borrowed,
        source.read_range(range).expect("read_range").as_slice(),
        "range_slice must agree with read_range byte for byte"
    );

    // The slice really points into the mapping, not into a copy.
    let base = source.as_bytes().as_ptr() as usize;
    let got = borrowed.as_ptr() as usize;
    assert_eq!(got, base + 128, "range_slice must borrow from the mapping");

    // Ranges a mapping cannot serve in full must decline so the caller's fallback
    // reports the error, rather than silently returning a short slice.
    let past_end = ByteRange::new(data.len() as u64 - 4, data.len() as u64 + 4);
    assert!(
        source.range_slice(past_end).is_none(),
        "past EOF must decline"
    );
    assert!(
        source.read_range(past_end).is_err(),
        "and the copying path must still error"
    );

    // An inverted range must decline rather than panic on the `end - start`.
    assert!(
        source.range_slice(ByteRange::new(400, 100)).is_none(),
        "an inverted range must decline"
    );
    // Whole-file and empty ranges are both legitimate.
    assert_eq!(
        source
            .range_slice(ByteRange::new(0, data.len() as u64))
            .expect("whole file")
            .len(),
        data.len()
    );
    assert_eq!(
        source
            .range_slice(ByteRange::new(7, 7))
            .expect("empty range")
            .len(),
        0
    );
}

// ---------------------------------------------------------------------------
// Concurrency (T2): no shared file cursor any more
// ---------------------------------------------------------------------------

/// The old `FileDataSource` serialised every read behind a mutex because `seek`
/// and `read` share the file cursor. The positional read has no cursor to race
/// on, so many threads may read different offsets of one source at once — and
/// each must get its own bytes, not another thread's.
#[test]
fn test_issue_14_file_data_source_concurrent_reads_are_correct() {
    const THREADS: usize = 8;
    const ROUNDS: usize = 200;
    const BLOCK: usize = 64;

    let data = payload();
    let path = write_temp_file("concurrent.bin", &data);
    let source = Arc::new(FileDataSource::open(&path).expect("open FileDataSource"));

    let mut handles = Vec::with_capacity(THREADS);
    for t in 0..THREADS {
        let source = Arc::clone(&source);
        let data = data.clone();
        handles.push(thread::spawn(move || {
            // Each thread owns one buffer for its whole run — the point of the
            // API — and walks the file at its own stride so the threads
            // constantly interleave at different offsets.
            let mut buf = vec![0u8; BLOCK];
            let blocks = data.len() / BLOCK;
            for round in 0..ROUNDS {
                let block = (t * 7 + round * 3) % blocks;
                let start = (block * BLOCK) as u64;
                let range = ByteRange::new(start, start + BLOCK as u64);
                let n = source
                    .read_range_into(range, &mut buf)
                    .expect("concurrent read_range_into");
                assert_eq!(n, BLOCK);
                assert_eq!(
                    &buf[..],
                    &data[block * BLOCK..(block + 1) * BLOCK],
                    "thread {t} round {round} read block {block} incorrectly — \
                     a shared cursor would show up exactly here"
                );

                // Mix in the owning path too: it must be equally unaffected.
                let owned = source.read_range(range).expect("concurrent read_range");
                assert_eq!(owned.as_slice(), &buf[..]);
            }
        }));
    }
    for handle in handles {
        handle.join().expect("worker thread panicked");
    }
}

/// Evidence, not an assertion: what a band-sized walk over a many-block file
/// costs before and after.
///
/// "Before" is the previous `FileDataSource::read_range` transcribed — lock the
/// cursor mutex, `seek`, `read_exact` into a freshly allocated `Vec` — so the
/// two numbers isolate exactly what this change removed: one allocation, one
/// lock round-trip and one syscall per block.
#[test]
fn test_issue_14_file_block_read_speed_evidence() {
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use std::sync::Mutex;
    use std::time::Instant;

    const BLOCKS: usize = 8192;
    const BLOCK: usize = 4096;

    let data: Vec<u8> = (0..BLOCKS * BLOCK).map(|i| (i % 251) as u8).collect();
    let mib = data.len() as f64 / (1024.0 * 1024.0);
    let path = write_temp_file("speed_evidence.bin", &data);

    let ranges: Vec<ByteRange> = (0..BLOCKS)
        .map(|b| ByteRange::new((b * BLOCK) as u64, ((b + 1) * BLOCK) as u64))
        .collect();

    // --- pre-fix: Mutex + seek + read_exact into a fresh Vec, per block ---
    let legacy = Mutex::new(File::open(&path).expect("open file"));
    let mut pre_fix = f64::MAX;
    let mut checksum = 0u64;
    for _ in 0..3 {
        let start = Instant::now();
        for range in &ranges {
            let mut file = legacy.lock().expect("lock cursor");
            file.seek(SeekFrom::Start(range.start)).expect("seek");
            let mut buffer = vec![0u8; BLOCK];
            file.read_exact(&mut buffer).expect("read_exact");
            checksum += buffer[0] as u64;
        }
        pre_fix = pre_fix.min(start.elapsed().as_secs_f64());
    }

    // --- post-fix: positional read into one reused buffer ------------------
    let source = FileDataSource::open(&path).expect("open FileDataSource");
    let mut scratch = vec![0u8; BLOCK];
    let mut post_fix = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        for range in &ranges {
            source
                .read_range_into(*range, &mut scratch)
                .expect("read_range_into");
            checksum += scratch[0] as u64;
        }
        post_fix = post_fix.min(start.elapsed().as_secs_f64());
    }

    assert!(checksum > 0, "the reads must not have been optimised away");
    eprintln!(
        "issue#14 file block reads, {BLOCKS} x {BLOCK} B ({mib:.0} MiB): \
         pre-fix (mutex+seek+alloc) {:.2} ms ({:.0} MiB/s)  \
         post-fix (pread into reused buffer) {:.2} ms ({:.0} MiB/s)  ({:.2}x)",
        pre_fix * 1e3,
        mib / pre_fix,
        post_fix * 1e3,
        mib / post_fix,
        pre_fix / post_fix.max(f64::EPSILON)
    );
}

/// Reading the *same* range from many threads must also be stable.
#[test]
fn test_issue_14_file_data_source_concurrent_same_range() {
    let data = payload();
    let path = write_temp_file("concurrent_same.bin", &data);
    let source = Arc::new(FileDataSource::open(&path).expect("open FileDataSource"));
    let range = ByteRange::new(1000, 1512);

    let mut handles = Vec::new();
    for _ in 0..8 {
        let source = Arc::clone(&source);
        let expected = data[1000..1512].to_vec();
        handles.push(thread::spawn(move || {
            let mut buf = vec![0u8; 512];
            for _ in 0..250 {
                let n = source.read_range_into(range, &mut buf).expect("read");
                assert_eq!(n, 512);
                assert_eq!(buf, expected);
            }
        }));
    }
    for handle in handles {
        handle.join().expect("worker thread panicked");
    }
}
