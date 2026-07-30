//! cool-japan/oxigeo#14 — hard evidence that the I/O layer's per-block heap
//! allocation is gone.
//!
//! [`DataSource::read_range`] hands back an owned `Vec`, so a band walk paid one
//! `malloc` + `free` per tile or strip before decoding started.
//! [`DataSource::read_range_into`] writes into a buffer the caller already owns;
//! this file proves, by counting every call into the global allocator, that the
//! new entry point allocates **nothing** while the old one allocates once per
//! call.
//!
//! # Why this file holds exactly one test
//!
//! The counters below are process-wide. Under `cargo nextest` each test gets its
//! own process, but under plain `cargo test` the built-in harness runs a file's
//! tests on parallel threads, and a second test allocating concurrently would
//! corrupt the measurement. Keeping a single test in the file makes the
//! measurement exact under both runners.
//!
//! # Unsafe
//!
//! `GlobalAlloc` is an unsafe trait, so a counting allocator cannot avoid
//! `unsafe`. The obligations are discharged trivially: every method forwards its
//! arguments unchanged to [`std::alloc::System`], which is the allocator that
//! would have served the request anyway, and the only added work is two relaxed
//! atomic increments. No pointer is created, adjusted or interpreted here.

#![allow(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::env::temp_dir;
use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use oxigeo_core::io::{ByteRange, DataSource, FileDataSource, MmapDataSource};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// Forwarding allocator that counts every allocating call.
struct CountingAllocator;

// SAFETY: every method delegates verbatim to `System`, whose implementation
// already upholds the `GlobalAlloc` contract for these exact arguments. The
// wrapper neither synthesises nor rewrites pointers or layouts; it only bumps a
// relaxed counter, which cannot allocate or panic.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.  Construction and cleanup both sit outside the measured regions, so
/// neither perturbs the allocation counts this file asserts on.
struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(temp_dir().join(format!(
            "oxigeo_issue14_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for TempPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Number of block-sized reads each measured loop performs.
const BLOCKS: usize = 512;
/// Bytes per simulated block.
const BLOCK: usize = 1024;

#[test]
fn test_issue_14_read_range_into_allocates_nothing_per_block() {
    // ---- setup (deliberately outside every measured region) --------------
    let data: Vec<u8> = (0..BLOCKS * BLOCK).map(|i| (i % 251) as u8).collect();
    let path = TempPath::new("zero_alloc.bin");
    {
        let mut f = fs::File::create(&path).expect("create temp file");
        f.write_all(&data).expect("write temp file");
        f.flush().expect("flush temp file");
    }

    let file = FileDataSource::open(&path).expect("open FileDataSource");
    let mmap = MmapDataSource::open(&path).expect("open MmapDataSource");
    let mut scratch = vec![0u8; BLOCK];
    let ranges: Vec<ByteRange> = (0..BLOCKS)
        .map(|b| ByteRange::new((b * BLOCK) as u64, ((b + 1) * BLOCK) as u64))
        .collect();

    // Warm everything up so no lazy one-off allocation lands inside a region.
    let mut checksum = 0u64;
    for range in &ranges {
        checksum += file
            .read_range_into(*range, &mut scratch)
            .expect("warm-up read") as u64;
        checksum += mmap
            .read_range_into(*range, &mut scratch)
            .expect("warm-up read") as u64;
    }

    // ---- measured region 1: the new entry point, file-backed -------------
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for range in &ranges {
        checksum += file
            .read_range_into(*range, &mut scratch)
            .expect("read_range_into") as u64;
    }
    let file_into_allocs = ALLOCATIONS.load(Ordering::Relaxed) - before;

    // ---- measured region 2: the new entry point, mmap-backed -------------
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for range in &ranges {
        checksum += mmap
            .read_range_into(*range, &mut scratch)
            .expect("read_range_into") as u64;
    }
    let mmap_into_allocs = ALLOCATIONS.load(Ordering::Relaxed) - before;

    // ---- measured region 3: borrowing straight from the mapping ----------
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for range in &ranges {
        checksum += mmap.range_slice(*range).map_or(0, |s| s.len() as u64);
    }
    let mmap_slice_allocs = ALLOCATIONS.load(Ordering::Relaxed) - before;

    // ---- measured region 4: the old owning entry point --------------------
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for range in &ranges {
        checksum += file.read_range(*range).expect("read_range").len() as u64;
    }
    let file_owned_allocs = ALLOCATIONS.load(Ordering::Relaxed) - before;

    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for range in &ranges {
        checksum += mmap.read_range(*range).expect("read_range").len() as u64;
    }
    let mmap_owned_allocs = ALLOCATIONS.load(Ordering::Relaxed) - before;

    // ---- assertions (allocating freely again) ----------------------------
    assert!(checksum > 0, "the reads must not have been optimised away");

    assert_eq!(
        file_into_allocs, 0,
        "FileDataSource::read_range_into must not allocate: {BLOCKS} reads made \
         {file_into_allocs} allocations"
    );
    assert_eq!(
        mmap_into_allocs, 0,
        "MmapDataSource::read_range_into must not allocate: {BLOCKS} reads made \
         {mmap_into_allocs} allocations"
    );
    assert_eq!(
        mmap_slice_allocs, 0,
        "MmapDataSource::range_slice must not allocate: {BLOCKS} lookups made \
         {mmap_slice_allocs} allocations"
    );

    // And the baseline it replaces: one allocation per block, every block.
    assert!(
        file_owned_allocs >= BLOCKS,
        "read_range is expected to allocate once per block ({BLOCKS} blocks made \
         {file_owned_allocs} allocations) — if this ever reaches 0 the test above \
         has stopped proving anything"
    );
    assert!(
        mmap_owned_allocs >= BLOCKS,
        "MmapDataSource::read_range is expected to allocate once per block \
         ({BLOCKS} blocks made {mmap_owned_allocs} allocations)"
    );

    eprintln!(
        "issue#14 io allocations for {BLOCKS} x {BLOCK}-byte blocks: \
         file read_range {file_owned_allocs} -> read_range_into {file_into_allocs}; \
         mmap read_range {mmap_owned_allocs} -> read_range_into {mmap_into_allocs} \
         -> range_slice {mmap_slice_allocs}"
    );
}
