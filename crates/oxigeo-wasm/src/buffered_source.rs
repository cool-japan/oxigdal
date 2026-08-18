//! An error-driven pull loop that lets the **synchronous**
//! [`oxigeo_geotiff::CogReader`] run on top of **asynchronous** HTTP range
//! requests.
//!
//! # The problem
//!
//! `CogReader` parses a COG through [`DataSource`], a synchronous trait. In a
//! browser there is no synchronous read: `fetch()` is a promise, and WASM
//! cannot block on it. [`crate::fetch::FetchBackend`] therefore answered
//! [`DataSource::read_range`] with `NotSupported`, which meant
//! `CogReader::open(backend)` failed at the very first header read — the
//! advanced viewer's URL path could never work in a browser at all.
//!
//! # The shape of the fix
//!
//! Rather than making the parser async (it is not ours to change), the loop is
//! inverted: the parser is run repeatedly against a *buffering* source that
//! knows only what has already been downloaded, and every read it cannot serve
//! is **recorded** rather than performed.
//!
//! 1. [`BufferedRangeSource`] implements [`DataSource`] over a cache of byte
//!    ranges. A read fully covered by the cache is served from it; a read that
//!    is not is appended to a *miss log* and answered with a distinct error.
//! 2. [`pull_until_ready`] runs the synchronous operation, and if the miss log
//!    came back non-empty it fetches the missed ranges asynchronously, inserts
//!    them into the cache and runs the operation again — from the top, which is
//!    safe because `CogReader`'s reads are deterministic.
//!
//! This is the error-driven variant of OxiGIS's pull-based
//! `CogOpen::poll -> Need(range) / Ready` state machine: same idea, expressed
//! with the trait the reader already has.
//!
//! # Why the loop keys on the miss log, never on the error
//!
//! **`Ok` is not proof of success.** `CogReader::open` reads several things
//! best-effort and swallows the failure:
//!
//! * `if let Ok(info) = ImageInfo::from_ifd(..)` — a level whose info will not
//!   parse is simply not counted, so a missing range silently *drops an
//!   overview*;
//! * `GeoKeyDirectory::from_ifd(..).ok().flatten()` — a missing range silently
//!   costs the file its CRS (`epsgCode` becomes `null`);
//! * `BlockIndex::try_parse(..) -> Option` — a missing range silently demotes a
//!   level to the slow per-lookup path.
//!
//! A loop that retried only on `Err` would return a *degraded* reader for all
//! three. So the driver's contract is: **`Ok` is returned only when the
//! operation succeeded and the miss log is empty and no round stalled.**
//!
//! Conversely the error a failed round returns is not necessarily the one this
//! module produced — `TiffFile::read_header_bytes`, for instance, catches the
//! miss, asks the source for its size and re-reads, so what reaches the driver
//! is a *header parse* error even though a miss was recorded. Branching on the
//! miss log rather than on error identity is what makes both cases work; the
//! recognisable error text exists for diagnostics only.
//!
//! # Testability
//!
//! The loop is generic over [`AsyncRangeFetcher`], a one-method seam. Native
//! tests drive the whole machine against an in-memory fetcher over synthetic
//! TIFF bytes (see this module's test section). The browser implementation of
//! the seam — `impl AsyncRangeFetcher for FetchBackend` in [`crate::fetch`] —
//! is the one piece that **cannot** be exercised natively, because it calls
//! `web_sys::window().fetch_with_request()`; it is deliberately a thin
//! translation of one `fetch()` response into [`RangeResponse`] so that
//! everything with logic in it stays under test here.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use oxigeo_core::error::{IoError, OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};

/// Granularity every network fetch is rounded up to.
///
/// A COG read asks for a few bytes at a time (an IFD entry count, a tag's
/// out-of-line array, one tile). Issuing an HTTP request per read would cost a
/// round trip each; rounding up to 64 KiB and coalescing what is left turns a
/// whole header walk into a single request on any normally laid-out file, at
/// the price of downloading at most 64 KiB more than was asked for.
pub(crate) const FETCH_BLOCK_SIZE: u64 = 64 * 1024;

/// Maximum number of fetch/retry rounds one driven operation may take.
///
/// Each round downloads at least one new byte (a round that does not is caught
/// as a stall), so this is a guard against pathological files — an IFD chain
/// scattered across hundreds of blocks — rather than against divergence.
pub(crate) const MAX_PULL_ROUNDS: usize = 64;

/// Prefix of the message every "recorded, not resident" error carries.
///
/// Diagnostics only: the driver decides what to do from the miss log, never
/// from this text. It is matched in exactly one place, to avoid reporting the
/// sentinel back to the caller as if it were the underlying failure.
const NOT_RESIDENT: &str = "buffered range source: range not resident";

/// Builds the error [`BufferedRangeSource::read_range`] returns for a range it
/// has recorded as a miss.
fn not_resident_error(range: ByteRange) -> OxiGeoError {
    OxiGeoError::Io(IoError::Read {
        message: format!(
            "{NOT_RESIDENT}: {}..{} (queued for fetch)",
            range.start, range.end
        ),
    })
}

/// Reports whether `error` is this module's "recorded, not resident" sentinel.
fn is_not_resident(error: &OxiGeoError) -> bool {
    matches!(
        error,
        OxiGeoError::Io(IoError::Read { message }) if message.starts_with(NOT_RESIDENT)
    )
}

/// Builds the end-of-file error every in-tree [`DataSource`] reports for a read
/// that starts past the end of the object.
fn eof_error(offset: u64) -> OxiGeoError {
    OxiGeoError::Io(IoError::UnexpectedEof { offset })
}

/// One response to an asynchronous range request.
///
/// Carries what the *response* said rather than what the request asked for, so
/// a server that ignores `Range` (answering `200` with the whole body) or that
/// clamps a range at end-of-file is handled by the same code path as a
/// well-behaved `206`.
#[derive(Debug, Clone)]
pub(crate) struct RangeResponse {
    /// File offset the returned bytes start at.
    ///
    /// For a `206` this is the `Content-Range` start, which is the request's
    /// start for any conforming server; for a `200` it is `0`.
    pub offset: u64,
    /// The bytes themselves.
    pub bytes: Vec<u8>,
    /// Total object size, when the response revealed it (`Content-Range`'s
    /// `/total`, or the body length of a `200`).
    pub total_size: Option<u64>,
    /// `true` when the server ignored `Range` and returned the whole object.
    ///
    /// The body is then authoritative for the object's size, and every later
    /// read can be served from it without another request.
    pub whole_object: bool,
}

/// The asynchronous half of the pull loop: fetch one byte range.
///
/// A one-method seam so the loop can be driven natively in tests. The browser
/// implementation lives on [`crate::fetch::FetchBackend`].
pub(crate) trait AsyncRangeFetcher {
    /// Fetches `range`, returning whatever the server actually sent.
    ///
    /// Implementations must not silently substitute a different range: report
    /// what came back in [`RangeResponse::offset`] / [`RangeResponse::bytes`]
    /// and let the caller reconcile it.
    fn fetch_range(&self, range: ByteRange) -> impl Future<Output = Result<RangeResponse>>;
}

/// A set of downloaded byte ranges, kept merged and disjoint.
///
/// Keyed by start offset; because overlapping and *adjacent* blocks are merged
/// on insert, a lookup only ever has to consider the one block that starts at
/// or before the requested offset.
#[derive(Debug, Default)]
struct ByteCache {
    /// Start offset → contiguous bytes from that offset.
    blocks: BTreeMap<u64, Vec<u8>>,
}

impl ByteCache {
    /// Returns the bytes of `range` when one block covers it in full.
    fn get(&self, range: ByteRange) -> Option<&[u8]> {
        let (&block_start, block) = self.blocks.range(..=range.start).next_back()?;
        let block_end = block_start.saturating_add(block.len() as u64);
        if range.end > block_end {
            return None;
        }
        let from = usize::try_from(range.start - block_start).ok()?;
        let to = usize::try_from(range.end - block_start).ok()?;
        block.get(from..to)
    }

    /// Inserts `data` at `offset`, merging it with any block it overlaps or
    /// touches. Later bytes win, so re-fetching a range is harmless.
    fn insert(&mut self, offset: u64, data: Vec<u8>) {
        if data.is_empty() {
            return;
        }
        let incoming_end = offset.saturating_add(data.len() as u64);

        // Every block that overlaps or is adjacent to the incoming one. `<=` on
        // both sides is deliberate: touching blocks merge, so the cache never
        // accumulates a run of abutting fragments.
        let neighbours: Vec<u64> = self
            .blocks
            .range(..=incoming_end)
            .filter(|(start, block)| {
                start.saturating_add(block.len() as u64) >= offset && **start <= incoming_end
            })
            .map(|(start, _)| *start)
            .collect();

        let mut merged_start = offset;
        let mut merged_end = incoming_end;
        for start in &neighbours {
            merged_start = merged_start.min(*start);
            let end = self
                .blocks
                .get(start)
                .map_or(*start, |block| start.saturating_add(block.len() as u64));
            merged_end = merged_end.max(end);
        }

        let Ok(merged_len) = usize::try_from(merged_end - merged_start) else {
            // A merged span wider than this target's address space cannot be
            // materialised; keep the incoming block on its own rather than
            // losing it.
            self.blocks.insert(offset, data);
            return;
        };

        let mut merged = vec![0u8; merged_len];
        // Existing bytes first, then the incoming ones on top: a server that
        // re-sends a range must not be able to resurrect stale bytes.
        for start in neighbours {
            if let Some(block) = self.blocks.remove(&start)
                && let Ok(at) = usize::try_from(start - merged_start)
                && let Some(slot) = merged.get_mut(at..at + block.len())
            {
                slot.copy_from_slice(&block);
            }
        }
        if let Ok(at) = usize::try_from(offset - merged_start)
            && let Some(slot) = merged.get_mut(at..at + data.len())
        {
            slot.copy_from_slice(&data);
        }
        self.blocks.insert(merged_start, merged);
    }

    /// Total number of bytes held.
    fn len(&self) -> usize {
        self.blocks.values().map(Vec::len).sum()
    }

    /// Drops every cached byte.
    fn clear(&mut self) {
        self.blocks.clear();
    }
}

/// Everything the source mutates, behind one lock so the cache, the miss log
/// and the learned object size can never be observed out of step.
#[derive(Debug, Default)]
struct SourceState {
    /// Bytes downloaded so far.
    cache: ByteCache,
    /// Ranges asked for since the last [`BufferedRangeSource::take_misses`].
    misses: Vec<ByteRange>,
    /// Object size, once a `HEAD` or a response header revealed it.
    total_size: Option<u64>,
}

/// A [`DataSource`] that serves only what has already been downloaded and
/// records what it was asked for and could not serve.
///
/// Cloning is O(1) and shares the same cache and miss log, which is what lets
/// the driver keep a handle on the state after the source has been moved into
/// a `CogReader`.
#[derive(Clone, Default)]
pub(crate) struct BufferedRangeSource {
    /// Shared mutable state. `Arc<Mutex<_>>` rather than `Rc<RefCell<_>>`
    /// because [`DataSource`] is `Send + Sync`; on `wasm32` (single-threaded)
    /// the lock is uncontended and effectively free.
    state: Arc<Mutex<SourceState>>,
}

impl std::fmt::Debug for BufferedRangeSource {
    /// Summarises the buffer instead of dumping it: a `CogReader<Self>` derives
    /// `Debug`, and a COG's worth of cached bytes is not a useful diagnostic.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state();
        f.debug_struct("BufferedRangeSource")
            .field("cached_bytes", &state.cache.len())
            .field("pending_misses", &state.misses.len())
            .field("total_size", &state.total_size)
            .finish()
    }
}

impl BufferedRangeSource {
    /// Creates an empty source.
    ///
    /// `total_size` is the object size if it is already known (from the `HEAD`
    /// request the fetch backend performs). Pass `None` when it is not — a
    /// server may omit `Content-Length`, and answering "0 bytes" would make
    /// every read look like it started past end-of-file. The size is learned
    /// from the first response that reveals it.
    pub(crate) fn new(total_size: Option<u64>) -> Self {
        let source = Self::default();
        source.state().total_size = total_size.filter(|size| *size > 0);
        source
    }

    /// Locks the shared state, recovering from a poisoned lock.
    ///
    /// The state holds no invariant that a panicking thread could have broken
    /// half-way — a partially inserted block is just bytes — so recovering it
    /// is sound, and it keeps a panic in one tile read from bricking the whole
    /// viewer. Mirrors what `CogReader::with_block_bytes` does with its scratch
    /// buffer.
    fn state(&self) -> MutexGuard<'_, SourceState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns the object size, if it is known yet.
    pub(crate) fn total_size(&self) -> Option<u64> {
        self.state().total_size
    }

    /// Returns how many bytes are currently cached.
    ///
    /// The driver uses this to detect a round that made no progress.
    pub(crate) fn cached_bytes(&self) -> usize {
        self.state().cache.len()
    }

    /// Empties the miss log.
    pub(crate) fn clear_misses(&self) {
        self.state().misses.clear();
    }

    /// Empties the miss log and returns what was in it.
    pub(crate) fn take_misses(&self) -> Vec<ByteRange> {
        std::mem::take(&mut self.state().misses)
    }

    /// Folds one fetched response into the cache.
    ///
    /// A `200` (the server ignored `Range`) replaces the cache with the whole
    /// body and pins the object size to the body length: it is the most
    /// authoritative answer available, and every subsequent read is served from
    /// it without another request.
    pub(crate) fn absorb(&self, response: RangeResponse) {
        let mut state = self.state();
        if response.whole_object {
            state.total_size = Some(response.bytes.len() as u64);
            state.cache.clear();
            state.cache.insert(0, response.bytes);
            return;
        }
        if let Some(total) = response.total_size {
            state.total_size = Some(total);
        }
        state.cache.insert(response.offset, response.bytes);
    }
}

impl DataSource for BufferedRangeSource {
    /// Returns the object size, or `0` while it is still unknown.
    ///
    /// Only one caller consults it — `TiffFile::read_header_bytes`, to decide
    /// whether a failed 16-byte header read means "short file" — and a `0` there
    /// merely costs one extra pull round.
    fn size(&self) -> Result<u64> {
        Ok(self.total_size().unwrap_or(0))
    }

    /// Serves `range` from the cache, or records it as a miss and fails.
    ///
    /// Reads that run past a known end-of-file are served **short** rather than
    /// recorded, matching what an HTTP server does with an over-long `Range`
    /// and what [`crate::fetch::FetchBackend`] has always returned; recording
    /// them would be a miss that no fetch could ever satisfy.
    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        if range.end < range.start {
            return Err(eof_error(range.start));
        }
        if range.end == range.start {
            return Ok(Vec::new());
        }

        let mut state = self.state();
        if let Some(bytes) = state.cache.get(range) {
            return Ok(bytes.to_vec());
        }

        // Clamp to the object's end once it is known.
        let wanted = match state.total_size {
            Some(total) if range.start >= total => return Err(eof_error(range.start)),
            Some(total) => ByteRange::new(range.start, range.end.min(total)),
            None => range,
        };
        if let Some(bytes) = state.cache.get(wanted) {
            return Ok(bytes.to_vec());
        }

        state.misses.push(wanted);
        Err(not_resident_error(wanted))
    }

    /// Always `true`: this source exists precisely to serve range requests, and
    /// many servers honour `Range` on `GET` while omitting `Accept-Ranges` from
    /// a `HEAD`. A server that really ignores it is handled by the `200`
    /// whole-body path instead.
    fn supports_range_requests(&self) -> bool {
        true
    }

    // `read_range_into` and `range_slice` are deliberately left at their trait
    // defaults. Lending is impossible from behind a `Mutex`, and the default
    // `read_range_into` funnels through `read_range` above — which is what keeps
    // the miss log complete for `CogReader::read_tile`'s uncompressed fast path,
    // the one caller that uses `read_range_into`.
}

/// Rounds `range` out to whole [`FETCH_BLOCK_SIZE`] blocks, clamped to the
/// object's end when it is known.
fn align_to_blocks(range: ByteRange, total_size: Option<u64>) -> Option<ByteRange> {
    if range.end <= range.start {
        return None;
    }
    let start = (range.start / FETCH_BLOCK_SIZE) * FETCH_BLOCK_SIZE;
    let end = range
        .end
        .checked_next_multiple_of(FETCH_BLOCK_SIZE)
        .unwrap_or(u64::MAX);
    let end = match total_size {
        // The tail block of an object is short; asking beyond the end wastes a
        // little of the server's time and confuses strict range parsers.
        Some(total) => end.min(total).max(range.end.min(total)),
        None => end,
    };
    if end <= start {
        return None;
    }
    Some(ByteRange::new(start, end))
}

/// Turns a round's miss log into the smallest set of block-aligned fetches that
/// covers it.
///
/// Overlapping and adjacent aligned ranges are coalesced, so a header walk that
/// missed five times inside one block issues one request, and two misses in
/// neighbouring blocks issue one request spanning both.
fn plan_fetches(misses: &[ByteRange], total_size: Option<u64>) -> Vec<ByteRange> {
    let mut aligned: Vec<ByteRange> = misses
        .iter()
        .filter_map(|miss| align_to_blocks(*miss, total_size))
        .collect();
    aligned.sort_by_key(|range| (range.start, range.end));

    let mut planned: Vec<ByteRange> = Vec::with_capacity(aligned.len());
    for range in aligned {
        match planned.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => planned.push(range),
        }
    }
    planned
}

/// Builds the error returned when a round downloaded nothing new.
fn stalled_error(last_error: Option<OxiGeoError>, planned: &[ByteRange]) -> OxiGeoError {
    if let Some(error) = last_error
        && !is_not_resident(&error)
    {
        return error;
    }
    let ranges = planned
        .iter()
        .map(|range| format!("{}..{}", range.start, range.end))
        .collect::<Vec<_>>()
        .join(", ");
    OxiGeoError::Io(IoError::Read {
        message: format!(
            "range fetch made no progress: the server returned no new bytes for [{ranges}]; \
             the object may be truncated, or the response body did not match the request"
        ),
    })
}

/// Runs a synchronous `CogReader` operation to completion, fetching whatever
/// byte ranges it turns out to need.
///
/// `op` is re-run from the top after every fetch round; `CogReader`'s reads are
/// deterministic, so each round either resolves the operation or reveals the
/// next ranges it wants.
///
/// # Success contract
///
/// `Ok` is returned **only** when `op` returned `Ok` *and* the miss log came
/// back empty — see this module's header for why an `Ok` with pending misses is
/// a silently degraded result rather than a success.
///
/// # Concurrent drivers
///
/// Several tile reads may be in flight over one source at once — JavaScript is
/// free to await a grid of them together. Each round runs `op` and drains the
/// miss log with **no suspension point between the two**, so a driver can only
/// ever take the misses its own `op` recorded. A concurrent driver's inserts can
/// at worst make this one retry a round it could otherwise have skipped, which
/// the round cap bounds.
///
/// # Errors
///
/// * whatever `op` returned, when it failed without recording a miss (a genuine
///   format or bounds error);
/// * whatever the fetcher returned, when the network failed;
/// * a "made no progress" error when a round downloaded nothing new — a
///   truncated object, or a server whose body does not match its headers;
/// * a "too many rounds" error after [`MAX_PULL_ROUNDS`].
pub(crate) async fn pull_until_ready<T, F, Op>(
    source: &BufferedRangeSource,
    fetcher: &F,
    mut op: Op,
) -> Result<T>
where
    F: AsyncRangeFetcher,
    Op: FnMut() -> Result<T>,
{
    source.clear_misses();

    for _round in 0..MAX_PULL_ROUNDS {
        let outcome = op();
        let misses = source.take_misses();

        // Only this round's genuine (non-sentinel) failure is worth reporting if
        // the next fetch stalls; a sentinel says nothing the driver does not
        // already know from the miss log.
        let mut last_error = None;
        match outcome {
            Ok(value) if misses.is_empty() => return Ok(value),
            Ok(_) => {}
            Err(error) => {
                if misses.is_empty() {
                    return Err(error);
                }
                last_error = Some(error);
            }
        }

        let planned = plan_fetches(&misses, source.total_size());
        let before = source.cached_bytes();
        for range in &planned {
            let response = fetcher.fetch_range(*range).await?;
            source.absorb(response);
        }
        if source.cached_bytes() <= before {
            return Err(stalled_error(last_error, &planned));
        }
    }

    Err(OxiGeoError::Io(IoError::Read {
        message: format!(
            "range fetch did not converge after {MAX_PULL_ROUNDS} rounds; \
             the file's directory chain is scattered beyond what this reader will follow"
        ),
    }))
}

/// Synthetic COG fixture and in-memory transport shared by this crate's tests.
///
/// Lives here rather than in a test module because more than one module's tests
/// need a *real* parsed COG over a *fake* network: this module's tests exercise
/// the pull loop itself, and `crate`'s exercise what the advanced viewer derives
/// from the reader the loop produces.
#[cfg(test)]
pub(crate) mod fixture {
    use super::*;
    use std::cell::RefCell;

    // -----------------------------------------------------------------------
    // Synthetic COG fixture
    //
    // A real little-endian classic TIFF: two levels, uncompressed 8-bit single
    // band, laid out so that the header, the tile data and the overview's tile
    // data land in *different* 64 KiB blocks. That is what makes the fetch
    // assertions below meaningful — on a fixture that fits in one block every
    // possible driver would pass.
    // -----------------------------------------------------------------------

    pub(crate) const TIFF_SHORT: u16 = 3;
    pub(crate) const TIFF_LONG: u16 = 4;

    /// Offset of the first IFD.
    pub(crate) const IFD0_AT: u32 = 16;
    /// Offset of level 0's out-of-line `TileOffsets` array (4 tiles).
    pub(crate) const TILE_OFFSETS_AT: u32 = 192;
    /// Offset of level 0's out-of-line `TileByteCounts` array (4 tiles).
    pub(crate) const TILE_COUNTS_AT: u32 = 208;
    /// Offset of the `GeoKeyDirectory` array when it is kept near the header.
    pub(crate) const GEO_KEYS_NEAR: u32 = 224;
    /// Offset of the overview IFD when it is kept near the header.
    pub(crate) const IFD1_NEAR: u32 = 512;
    /// An offset comfortably inside the *second* 64 KiB block.
    pub(crate) const SECOND_BLOCK: u32 = 70_000;
    /// Where level 0's four tiles live (third block).
    pub(crate) const LEVEL0_TILES_AT: u32 = 200_000;
    /// Where the overview's single tile lives (seventh block).
    pub(crate) const LEVEL1_TILE_AT: u32 = 400_000;
    /// Bytes per tile: 16 x 16 pixels, one 8-bit sample.
    pub(crate) const TILE_BYTES: u32 = 256;
    /// EPSG code the fixture's GeoKeyDirectory declares.
    pub(crate) const FIXTURE_EPSG: u16 = 32633;

    /// Where the fixture puts the two structures the tests move around.
    #[derive(Clone, Copy)]
    pub(crate) struct FixtureLayout {
        /// Offset of the overview IFD.
        pub(crate) overview_ifd_at: u32,
        /// Offset of the GeoKeyDirectory value array.
        pub(crate) geo_keys_at: u32,
    }

    impl Default for FixtureLayout {
        fn default() -> Self {
            Self {
                overview_ifd_at: IFD1_NEAR,
                geo_keys_at: GEO_KEYS_NEAR,
            }
        }
    }

    /// One IFD entry: tag, field type, count, and either an inline value or an
    /// offset to the value array.
    pub(crate) fn entry(tag: u16, field_type: u16, count: u32, value: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(12);
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&field_type.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        // A SHORT of count 1 occupies the first two bytes of the value field.
        if field_type == TIFF_SHORT && count == 1 {
            bytes.extend_from_slice(&(value as u16).to_le_bytes());
            bytes.extend_from_slice(&[0, 0]);
        } else {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// Serialises an IFD: entry count, entries in ascending tag order, link.
    pub(crate) fn build_ifd(entries: &[Vec<u8>], next_ifd: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for entry in entries {
            out.extend_from_slice(entry);
        }
        out.extend_from_slice(&next_ifd.to_le_bytes());
        out
    }

    /// Writes `bytes` at `at`, growing the buffer as needed.
    pub(crate) fn put(buf: &mut Vec<u8>, at: u32, bytes: &[u8]) {
        let at = at as usize;
        if buf.len() < at + bytes.len() {
            buf.resize(at + bytes.len(), 0);
        }
        buf[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// The byte value every pixel of level 0's tile `(tx, ty)` carries.
    pub(crate) fn level0_tile_value(tile_x: u32, tile_y: u32) -> u8 {
        (10 + tile_y * 2 + tile_x) as u8
    }

    /// The byte value every pixel of the overview's single tile carries.
    pub(crate) const LEVEL1_TILE_VALUE: u8 = 99;

    /// Builds the fixture: a 32x32 image in 16x16 tiles plus one 16x16
    /// overview, both uncompressed, with a GeoKeyDirectory naming
    /// [`FIXTURE_EPSG`].
    pub(crate) fn build_fixture(layout: FixtureLayout) -> Vec<u8> {
        let mut buf = vec![0u8; 16];
        buf[0..2].copy_from_slice(b"II");
        buf[2..4].copy_from_slice(&42u16.to_le_bytes());
        buf[4..8].copy_from_slice(&IFD0_AT.to_le_bytes());

        let ifd0 = build_ifd(
            &[
                entry(254, TIFF_LONG, 1, 0),                     // NewSubfileType
                entry(256, TIFF_LONG, 1, 32),                    // ImageWidth
                entry(257, TIFF_LONG, 1, 32),                    // ImageLength
                entry(258, TIFF_SHORT, 1, 8),                    // BitsPerSample
                entry(259, TIFF_SHORT, 1, 1),                    // Compression: none
                entry(262, TIFF_SHORT, 1, 1),                    // Photometric: BlackIsZero
                entry(277, TIFF_SHORT, 1, 1),                    // SamplesPerPixel
                entry(322, TIFF_SHORT, 1, 16),                   // TileWidth
                entry(323, TIFF_SHORT, 1, 16),                   // TileLength
                entry(324, TIFF_LONG, 4, TILE_OFFSETS_AT),       // TileOffsets
                entry(325, TIFF_LONG, 4, TILE_COUNTS_AT),        // TileByteCounts
                entry(339, TIFF_SHORT, 1, 1),                    // SampleFormat
                entry(34735, TIFF_SHORT, 8, layout.geo_keys_at), // GeoKeyDirectory
            ],
            layout.overview_ifd_at,
        );
        put(&mut buf, IFD0_AT, &ifd0);
        assert!(
            IFD0_AT as usize + ifd0.len() <= TILE_OFFSETS_AT as usize,
            "IFD0 overruns its slot"
        );

        // Level 0's out-of-line tile directory: four tiles laid out back to back.
        let mut offsets = Vec::new();
        let mut counts = Vec::new();
        for tile in 0..4u32 {
            offsets.extend_from_slice(&(LEVEL0_TILES_AT + tile * TILE_BYTES).to_le_bytes());
            counts.extend_from_slice(&TILE_BYTES.to_le_bytes());
        }
        put(&mut buf, TILE_OFFSETS_AT, &offsets);
        put(&mut buf, TILE_COUNTS_AT, &counts);

        // GeoKeyDirectory: version 1.1.0, one key — ProjectedCSTypeGeoKey.
        let geo_keys: [u16; 8] = [1, 1, 0, 1, 3072, 0, 1, FIXTURE_EPSG];
        let mut geo_bytes = Vec::new();
        for value in geo_keys {
            geo_bytes.extend_from_slice(&value.to_le_bytes());
        }
        put(&mut buf, layout.geo_keys_at, &geo_bytes);

        let ifd1 = build_ifd(
            &[
                entry(254, TIFF_LONG, 1, 1), // NewSubfileType: reduced resolution
                entry(256, TIFF_LONG, 1, 16),
                entry(257, TIFF_LONG, 1, 16),
                entry(258, TIFF_SHORT, 1, 8),
                entry(259, TIFF_SHORT, 1, 1),
                entry(262, TIFF_SHORT, 1, 1),
                entry(277, TIFF_SHORT, 1, 1),
                entry(322, TIFF_SHORT, 1, 16),
                entry(323, TIFF_SHORT, 1, 16),
                entry(324, TIFF_LONG, 1, LEVEL1_TILE_AT),
                entry(325, TIFF_LONG, 1, TILE_BYTES),
                entry(339, TIFF_SHORT, 1, 1),
            ],
            0,
        );
        put(&mut buf, layout.overview_ifd_at, &ifd1);

        for tile_y in 0..2u32 {
            for tile_x in 0..2u32 {
                let at = LEVEL0_TILES_AT + (tile_y * 2 + tile_x) * TILE_BYTES;
                put(
                    &mut buf,
                    at,
                    &vec![level0_tile_value(tile_x, tile_y); TILE_BYTES as usize],
                );
            }
        }
        put(
            &mut buf,
            LEVEL1_TILE_AT,
            &vec![LEVEL1_TILE_VALUE; TILE_BYTES as usize],
        );

        buf
    }

    // -----------------------------------------------------------------------
    // In-memory fetchers
    // -----------------------------------------------------------------------

    /// How the fake server answers.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ServerBehaviour {
        /// Honours `Range`, answering `206` with `Content-Range`.
        Ranges,
        /// Ignores `Range`, answering `200` with the whole body.
        IgnoresRange,
        /// Answers every request with an empty body (a stalling server).
        EmptyBodies,
        /// Fails every request.
        Fails,
    }

    /// An [`AsyncRangeFetcher`] over a byte buffer that records every request.
    pub(crate) struct TestFetcher {
        data: Vec<u8>,
        behaviour: ServerBehaviour,
        /// Whether responses reveal the object size.
        announce_total: bool,
        requests: RefCell<Vec<ByteRange>>,
    }

    impl TestFetcher {
        pub(crate) fn new(data: Vec<u8>) -> Self {
            Self {
                data,
                behaviour: ServerBehaviour::Ranges,
                announce_total: true,
                requests: RefCell::new(Vec::new()),
            }
        }

        pub(crate) fn with_behaviour(mut self, behaviour: ServerBehaviour) -> Self {
            self.behaviour = behaviour;
            self
        }

        pub(crate) fn silent_about_total(mut self) -> Self {
            self.announce_total = false;
            self
        }

        pub(crate) fn requests(&self) -> Vec<ByteRange> {
            self.requests.borrow().clone()
        }

        pub(crate) fn request_count(&self) -> usize {
            self.requests.borrow().len()
        }
    }

    impl AsyncRangeFetcher for TestFetcher {
        fn fetch_range(&self, range: ByteRange) -> impl Future<Output = Result<RangeResponse>> {
            self.requests.borrow_mut().push(range);
            let total = self.data.len() as u64;
            let announced = self.announce_total.then_some(total);
            let response = match self.behaviour {
                ServerBehaviour::Fails => Err(OxiGeoError::Io(IoError::Network {
                    message: "test server refused the connection".to_string(),
                })),
                ServerBehaviour::IgnoresRange => Ok(RangeResponse {
                    offset: 0,
                    bytes: self.data.clone(),
                    total_size: Some(total),
                    whole_object: true,
                }),
                ServerBehaviour::EmptyBodies => Ok(RangeResponse {
                    offset: range.start,
                    bytes: Vec::new(),
                    total_size: announced,
                    whole_object: false,
                }),
                ServerBehaviour::Ranges => {
                    let start = usize::try_from(range.start).unwrap_or(usize::MAX);
                    let end = usize::try_from(range.end).unwrap_or(usize::MAX);
                    let start = start.min(self.data.len());
                    let end = end.clamp(start, self.data.len());
                    Ok(RangeResponse {
                        offset: range.start,
                        bytes: self.data[start..end].to_vec(),
                        total_size: announced,
                        whole_object: false,
                    })
                }
            };
            async move { response }
        }
    }

    /// Opens the fixture through the pull loop, as the browser viewer does.
    pub(crate) fn open_reader(
        fetcher: &TestFetcher,
        known_size: Option<u64>,
    ) -> (
        BufferedRangeSource,
        Result<oxigeo_geotiff::CogReader<BufferedRangeSource>>,
    ) {
        let source = BufferedRangeSource::new(known_size);
        let reader = futures::executor::block_on(pull_until_ready(&source, fetcher, || {
            oxigeo_geotiff::CogReader::open(source.clone())
        }));
        (source, reader)
    }

    /// Every fetched range must be 64 KiB-aligned, except where the object's
    /// own end cuts the tail block short.
    pub(crate) fn assert_block_aligned(requests: &[ByteRange], total: u64) {
        for range in requests {
            assert_eq!(
                range.start % FETCH_BLOCK_SIZE,
                0,
                "fetch {}..{} does not start on a block boundary",
                range.start,
                range.end
            );
            assert!(
                range.end % FETCH_BLOCK_SIZE == 0 || range.end == total,
                "fetch {}..{} neither ends on a block boundary nor at the object's end ({total})",
                range.start,
                range.end
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;

    // -----------------------------------------------------------------------
    // The loop
    // -----------------------------------------------------------------------

    /// The whole point: a synchronous parse over an async transport converges,
    /// and on a normally laid-out file it costs exactly one request.
    #[test]
    fn open_converges_and_parses_over_range_fetches() {
        let data = build_fixture(FixtureLayout::default());
        let total = data.len() as u64;
        let fetcher = TestFetcher::new(data);
        let (_source, reader) = open_reader(&fetcher, Some(total));
        let reader = reader.expect("open converges");

        assert_eq!(reader.width(), 32);
        assert_eq!(reader.height(), 32);
        assert_eq!(reader.tile_size(), Some((16, 16)));
        assert_eq!(reader.overview_count(), 1);
        assert_eq!(reader.epsg_code(), Some(u32::from(FIXTURE_EPSG)));

        assert_eq!(
            fetcher.requests(),
            vec![ByteRange::new(0, FETCH_BLOCK_SIZE)],
            "a header that fits one block must cost one request"
        );
    }

    /// An overview IFD in a later block is still found: the chain walk fails
    /// with a recorded miss, the driver fetches the block and re-parses.
    #[test]
    fn overview_ifd_in_a_later_block_still_becomes_a_level() {
        let data = build_fixture(FixtureLayout {
            overview_ifd_at: SECOND_BLOCK,
            ..FixtureLayout::default()
        });
        let total = data.len() as u64;
        let fetcher = TestFetcher::new(data);
        let (_source, reader) = open_reader(&fetcher, Some(total));
        let reader = reader.expect("open converges");

        assert_eq!(reader.overview_count(), 1);
        assert_eq!(
            reader.level_ifd(1).map(|ifd| ifd.entries.len()),
            Some(12),
            "level 1 must resolve to the overview IFD"
        );
        assert_eq!(
            fetcher.requests(),
            vec![
                ByteRange::new(0, FETCH_BLOCK_SIZE),
                ByteRange::new(FETCH_BLOCK_SIZE, 2 * FETCH_BLOCK_SIZE),
            ]
        );
    }

    /// The guard for the retry-on-swallowed-miss rule.
    ///
    /// `CogReader::open` reads the GeoKeyDirectory best-effort
    /// (`.ok().flatten()`), so a miss there does not fail the open — it silently
    /// costs the file its CRS. A driver that retried only on `Err` would return
    /// `Ok` with `epsg_code() == None`.
    #[test]
    fn geo_keys_in_a_later_block_survive_the_swallowed_miss() {
        let data = build_fixture(FixtureLayout {
            geo_keys_at: SECOND_BLOCK,
            ..FixtureLayout::default()
        });
        let total = data.len() as u64;
        let fetcher = TestFetcher::new(data);
        let (_source, reader) = open_reader(&fetcher, Some(total));
        let reader = reader.expect("open converges");

        assert_eq!(
            reader.epsg_code(),
            Some(u32::from(FIXTURE_EPSG)),
            "the swallowed GeoKeyDirectory miss must be retried, not accepted"
        );
        assert_eq!(fetcher.request_count(), 2);
    }

    /// A tile read pulls exactly the block its bytes live in — no more, and
    /// nothing at all for a second tile that shares the block.
    #[test]
    fn tile_reads_fetch_only_the_blocks_they_need() {
        let data = build_fixture(FixtureLayout::default());
        let total = data.len() as u64;
        let fetcher = TestFetcher::new(data);
        let (source, reader) = open_reader(&fetcher, Some(total));
        let reader = reader.expect("open converges");
        assert_eq!(fetcher.request_count(), 1, "open");

        let tile = futures::executor::block_on(pull_until_ready(&source, &fetcher, || {
            reader.read_tile(0, 0, 0)
        }))
        .expect("first tile");
        assert_eq!(tile, vec![level0_tile_value(0, 0); TILE_BYTES as usize]);
        assert_eq!(fetcher.request_count(), 2, "one block for the first tile");
        assert_eq!(
            fetcher.requests()[1],
            ByteRange::new(3 * FETCH_BLOCK_SIZE, 4 * FETCH_BLOCK_SIZE)
        );

        let neighbour = futures::executor::block_on(pull_until_ready(&source, &fetcher, || {
            reader.read_tile(0, 1, 0)
        }))
        .expect("second tile");
        assert_eq!(
            neighbour,
            vec![level0_tile_value(1, 0); TILE_BYTES as usize]
        );
        assert_eq!(
            fetcher.request_count(),
            2,
            "a tile already covered by a fetched block must cost no request"
        );

        let overview = futures::executor::block_on(pull_until_ready(&source, &fetcher, || {
            reader.read_tile(1, 0, 0)
        }))
        .expect("overview tile");
        assert_eq!(overview, vec![LEVEL1_TILE_VALUE; TILE_BYTES as usize]);
        assert_eq!(
            fetcher.request_count(),
            3,
            "one block for the overview tile"
        );

        let requests = fetcher.requests();
        assert_eq!(
            requests[2],
            ByteRange::new(6 * FETCH_BLOCK_SIZE, total),
            "the object's tail block is clamped to its end"
        );
        assert_block_aligned(&requests, total);
    }

    /// A server that ignores `Range` answers `200` with the whole object; the
    /// body is cached once and everything is served from it thereafter — even
    /// though the object's size was not known up front.
    #[test]
    fn server_ignoring_range_serves_everything_from_the_full_body() {
        let data = build_fixture(FixtureLayout::default());
        let total = data.len() as u64;
        let fetcher = TestFetcher::new(data).with_behaviour(ServerBehaviour::IgnoresRange);
        let (source, reader) = open_reader(&fetcher, None);
        let reader = reader.expect("open converges");

        assert_eq!(reader.overview_count(), 1);
        assert_eq!(
            source.total_size(),
            Some(total),
            "size learned from the body"
        );
        assert_eq!(fetcher.request_count(), 1);

        for (level, tile_x, tile_y, value) in [
            (0, 0, 0, level0_tile_value(0, 0)),
            (0, 1, 1, level0_tile_value(1, 1)),
            (1, 0, 0, LEVEL1_TILE_VALUE),
        ] {
            let tile = futures::executor::block_on(pull_until_ready(&source, &fetcher, || {
                reader.read_tile(level, tile_x, tile_y)
            }))
            .expect("tile");
            assert_eq!(tile, vec![value; TILE_BYTES as usize]);
        }
        assert_eq!(
            fetcher.request_count(),
            1,
            "the whole body was cached; no tile may issue another request"
        );
    }

    /// A size the `HEAD` could not determine (`Content-Length` absent) must not
    /// be taken as "the object is empty".
    #[test]
    fn unknown_size_is_not_treated_as_end_of_file() {
        let data = build_fixture(FixtureLayout::default());
        let fetcher = TestFetcher::new(data).silent_about_total();
        let (source, reader) = open_reader(&fetcher, Some(0));
        assert!(reader.is_ok(), "a zero HEAD size must not block the open");
        assert_eq!(
            source.total_size(),
            None,
            "nothing revealed the size, so it stays unknown"
        );
    }

    // -----------------------------------------------------------------------
    // Termination
    // -----------------------------------------------------------------------

    /// A failing transport surfaces its own error, not a retry storm.
    #[test]
    fn fetch_failure_surfaces_the_underlying_error() {
        let data = build_fixture(FixtureLayout::default());
        let total = data.len() as u64;
        let fetcher = TestFetcher::new(data).with_behaviour(ServerBehaviour::Fails);
        let (_source, reader) = open_reader(&fetcher, Some(total));

        let error = reader.expect_err("a failing server cannot open a COG");
        assert!(
            matches!(&error, OxiGeoError::Io(IoError::Network { message }) if message.contains("refused")),
            "expected the transport's own error, got {error}"
        );
        assert_eq!(fetcher.request_count(), 1, "no retry storm");
    }

    /// A server that answers every request with an empty body makes no
    /// progress; the loop must notice and stop.
    #[test]
    fn stalled_fetches_terminate_instead_of_looping() {
        let data = build_fixture(FixtureLayout::default());
        let total = data.len() as u64;
        let fetcher = TestFetcher::new(data).with_behaviour(ServerBehaviour::EmptyBodies);
        let (_source, reader) = open_reader(&fetcher, Some(total));

        let error = reader.expect_err("an empty-bodied server cannot open a COG");
        assert!(
            matches!(&error, OxiGeoError::Io(IoError::Read { message })
                if message.contains("made no progress") && message.contains("0..65536")),
            "expected a no-progress error naming the range, got {error}"
        );
        assert_eq!(fetcher.request_count(), 1, "one round, then stop");
    }

    /// A file that is fully resident and simply not a TIFF fails with the
    /// parser's own verdict: no miss is recorded, so nothing is retried.
    #[test]
    fn resident_garbage_terminates_with_the_parse_error() {
        let fetcher = TestFetcher::new(vec![0x7Fu8; 4096]);
        let (_source, reader) = open_reader(&fetcher, Some(4096));

        let error = reader.expect_err("garbage is not a COG");
        assert!(
            !is_not_resident(&error),
            "the caller must see the format error, not the pull sentinel: {error}"
        );
        assert_eq!(
            fetcher.request_count(),
            1,
            "one fetch to make the bytes resident, then a verdict"
        );
    }

    /// A file too short to hold a TIFF header terminates too — the path where
    /// `TiffFile::read_header_bytes` swallows the sentinel and re-reads.
    #[test]
    fn truncated_object_terminates_with_the_header_error() {
        let fetcher = TestFetcher::new(b"II".to_vec());
        let (_source, reader) = open_reader(&fetcher, Some(2));
        let error = reader.expect_err("two bytes are not a COG");
        assert!(!is_not_resident(&error), "unexpected sentinel: {error}");
        assert!(fetcher.request_count() <= 2, "bounded");
    }

    // -----------------------------------------------------------------------
    // Cache, planning and source semantics
    // -----------------------------------------------------------------------

    #[test]
    fn cache_merges_overlapping_and_adjacent_blocks() {
        let mut cache = ByteCache::default();
        cache.insert(0, vec![1; 16]);
        cache.insert(16, vec![2; 16]); // adjacent
        assert_eq!(cache.blocks.len(), 1, "adjacent blocks merge");
        assert_eq!(cache.len(), 32);

        cache.insert(8, vec![3; 16]); // overlaps both
        assert_eq!(cache.blocks.len(), 1);
        assert_eq!(cache.len(), 32);
        assert_eq!(
            cache.get(ByteRange::new(0, 32)).map(<[u8]>::to_vec),
            Some([vec![1; 8], vec![3; 16], vec![2; 8]].concat()),
            "later bytes win"
        );

        cache.insert(64, vec![4; 8]); // disjoint
        assert_eq!(cache.blocks.len(), 2);
        assert!(cache.get(ByteRange::new(30, 66)).is_none(), "hole");
        assert_eq!(cache.get(ByteRange::new(64, 72)).map(<[u8]>::len), Some(8));
    }

    #[test]
    fn fetch_planning_aligns_and_coalesces() {
        // Three misses inside one block collapse to one aligned request.
        let planned = plan_fetches(
            &[
                ByteRange::new(8, 16),
                ByteRange::new(200, 204),
                ByteRange::new(4096, 4100),
            ],
            Some(1_000_000),
        );
        assert_eq!(planned, vec![ByteRange::new(0, FETCH_BLOCK_SIZE)]);

        // Misses in neighbouring blocks coalesce into one span.
        let planned = plan_fetches(
            &[
                ByteRange::new(8, 16),
                ByteRange::new(FETCH_BLOCK_SIZE + 4, FETCH_BLOCK_SIZE + 8),
            ],
            Some(1_000_000),
        );
        assert_eq!(planned, vec![ByteRange::new(0, 2 * FETCH_BLOCK_SIZE)]);

        // Distant misses stay separate, and the tail is clamped to the object.
        let planned = plan_fetches(
            &[ByteRange::new(8, 16), ByteRange::new(900_000, 900_016)],
            Some(900_100),
        );
        assert_eq!(
            planned,
            vec![
                ByteRange::new(0, FETCH_BLOCK_SIZE),
                ByteRange::new(13 * FETCH_BLOCK_SIZE, 900_100),
            ]
        );

        // With the size unknown there is nothing to clamp against.
        let planned = plan_fetches(&[ByteRange::new(70_000, 70_004)], None);
        assert_eq!(
            planned,
            vec![ByteRange::new(FETCH_BLOCK_SIZE, 2 * FETCH_BLOCK_SIZE)]
        );
    }

    #[test]
    fn source_records_misses_and_serves_cached_ranges() {
        let source = BufferedRangeSource::new(Some(1024));
        assert_eq!(source.size().unwrap_or_default(), 1024);

        let error = source
            .read_range(ByteRange::new(0, 16))
            .expect_err("nothing is cached yet");
        assert!(is_not_resident(&error), "{error}");
        assert_eq!(source.take_misses(), vec![ByteRange::new(0, 16)]);
        assert!(source.take_misses().is_empty(), "the log is drained");

        source.absorb(RangeResponse {
            offset: 0,
            bytes: (0u8..64).collect(),
            total_size: Some(1024),
            whole_object: false,
        });
        assert_eq!(
            source.read_range(ByteRange::new(8, 12)).unwrap_or_default(),
            vec![8, 9, 10, 11]
        );
        assert!(source.take_misses().is_empty(), "a hit records nothing");

        // Past the cached bytes but inside the object: a miss.
        assert!(source.read_range(ByteRange::new(60, 80)).is_err());
        assert_eq!(source.take_misses(), vec![ByteRange::new(60, 80)]);
    }

    #[test]
    fn source_clamps_reads_at_end_of_file_instead_of_missing_forever() {
        let source = BufferedRangeSource::new(Some(24));
        source.absorb(RangeResponse {
            offset: 0,
            bytes: (0u8..24).collect(),
            total_size: Some(24),
            whole_object: false,
        });

        // An over-long read is served short, exactly as an HTTP server would.
        assert_eq!(
            source
                .read_range(ByteRange::new(16, 64))
                .unwrap_or_default(),
            (16u8..24).collect::<Vec<u8>>()
        );
        assert!(source.take_misses().is_empty(), "no unsatisfiable miss");

        // A read that starts past the end is end-of-file, not a miss.
        let error = source
            .read_range(ByteRange::new(24, 32))
            .expect_err("past the end");
        assert!(
            matches!(error, OxiGeoError::Io(IoError::UnexpectedEof { offset }) if offset == 24),
            "{error}"
        );
        assert!(source.take_misses().is_empty());

        // Empty and inverted ranges behave like every other in-tree source.
        assert!(
            source
                .read_range(ByteRange::new(4, 4))
                .unwrap_or_else(|_| vec![0])
                .is_empty()
        );
        assert!(source.read_range(ByteRange::new(12, 4)).is_err());
    }

    #[test]
    fn cloned_sources_share_one_cache() {
        let source = BufferedRangeSource::new(None);
        let clone = source.clone();
        clone.absorb(RangeResponse {
            offset: 0,
            bytes: vec![7; 8],
            total_size: Some(8),
            whole_object: false,
        });
        assert_eq!(source.cached_bytes(), 8);
        assert_eq!(source.total_size(), Some(8));
        assert_eq!(
            source.read_range(ByteRange::new(0, 8)).unwrap_or_default(),
            vec![7; 8]
        );
    }
}
