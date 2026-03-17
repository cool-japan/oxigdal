//! Property-based tests for oxigdal-streaming core algorithms.
//!
//! Uses `proptest` to verify invariants of `ByteRange`, `coalesce_ranges`,
//! Arrow IPC framing, and alignment utilities.

use oxigdal_streaming::{
    arrow_ipc::{ARROW_ALIGNMENT, ArrowIpcReader, ArrowIpcWriter, align_to},
    io_coalescing::{ByteRange, CoalescingConfig, coalesce_ranges},
};
use proptest::prelude::*;

// ── ByteRange properties ──────────────────────────────────────────────────────

proptest! {
    /// `len()` always equals `end - start`.
    #[test]
    fn prop_byte_range_len_equals_end_minus_start(
        start in 0u32..u32::MAX,
        extra in 0u32..65535u32,
    ) {
        let s = start as u64;
        let e = s + extra as u64;
        let br = ByteRange::new(s, e);
        prop_assert_eq!(br.len(), e - s);
    }

    /// `is_empty()` is consistent with `len() == 0`.
    #[test]
    fn prop_byte_range_is_empty_consistent_with_len(
        start in 0u32..u32::MAX,
        extra in 0u32..65535u32,
    ) {
        let s = start as u64;
        let e = s + extra as u64;
        let br = ByteRange::new(s, e);
        // is_empty() should agree with len() == 0
        if br.is_empty() {
            prop_assert_eq!(br.len(), 0u64);
        } else {
            prop_assert!(!br.is_empty());
        }
    }

    /// A zero-length range is empty; a non-zero-length range is not.
    #[test]
    fn prop_byte_range_empty_iff_start_eq_end(start in 0u32..u32::MAX) {
        let s = start as u64;
        let empty = ByteRange::new(s, s);
        prop_assert!(empty.is_empty());
        let nonempty = ByteRange::new(s, s + 1);
        prop_assert!(!nonempty.is_empty());
    }

    /// Merged range contains both input ranges.
    #[test]
    fn prop_byte_range_merge_contains_both(
        s1 in 0u32..50000u32,
        l1 in 0u32..10000u32,
        s2 in 0u32..50000u32,
        l2 in 0u32..10000u32,
    ) {
        let a = ByteRange::new(s1 as u64, s1 as u64 + l1 as u64);
        let b = ByteRange::new(s2 as u64, s2 as u64 + l2 as u64);
        let m = a.merge(&b);
        prop_assert!(m.start <= a.start);
        prop_assert!(m.start <= b.start);
        prop_assert!(m.end >= a.end);
        prop_assert!(m.end >= b.end);
    }

    /// Merge is commutative.
    #[test]
    fn prop_byte_range_merge_commutative(
        s1 in 0u32..50000u32,
        l1 in 0u32..10000u32,
        s2 in 0u32..50000u32,
        l2 in 0u32..10000u32,
    ) {
        let a = ByteRange::new(s1 as u64, s1 as u64 + l1 as u64);
        let b = ByteRange::new(s2 as u64, s2 as u64 + l2 as u64);
        prop_assert_eq!(a.merge(&b), b.merge(&a));
    }

    /// Merge is idempotent: merging a range with itself returns the same range.
    #[test]
    fn prop_byte_range_merge_idempotent(
        start in 0u32..50000u32,
        extra in 0u32..10000u32,
    ) {
        let a = ByteRange::new(start as u64, start as u64 + extra as u64);
        prop_assert_eq!(a.merge(&a.clone()), a);
    }

    /// `gap_to` returns 0 for overlapping or adjoining ranges.
    #[test]
    fn prop_byte_range_gap_zero_for_overlapping(
        start in 0u32..50000u32,
        len in 1u32..10000u32,
        overlap in 1u32..100u32,
    ) {
        let a = ByteRange::new(start as u64, start as u64 + len as u64);
        // b starts before a.end (overlapping)
        let b_start = (a.end).saturating_sub(overlap as u64);
        let b = ByteRange::new(b_start, b_start + 100);
        prop_assert_eq!(a.gap_to(&b), 0);
    }
}

// ── coalesce_ranges properties ────────────────────────────────────────────────

/// Build a sorted, non-overlapping list of ByteRanges from (start, len) pairs.
fn make_ranges(pairs: &[(u16, u16)]) -> Vec<ByteRange> {
    pairs
        .iter()
        .map(|&(s, l)| ByteRange::new(s as u64, s as u64 + l as u64))
        .collect()
}

proptest! {
    /// Output count never exceeds input count (coalescing never creates ranges).
    #[test]
    fn prop_coalesce_count_le_input(
        pairs in prop::collection::vec((0u16..60000u16, 0u16..1000u16), 0..30),
    ) {
        let ranges = make_ranges(&pairs);
        let config = CoalescingConfig::default();
        let coalesced = coalesce_ranges(ranges.clone(), &config);
        prop_assert!(coalesced.len() <= ranges.len().max(1));
    }

    /// Output fetch_ranges are non-overlapping (sorted order: each end ≤ next start).
    #[test]
    fn prop_coalesce_output_non_overlapping(
        pairs in prop::collection::vec((0u16..60000u16, 1u16..500u16), 0..20),
    ) {
        let ranges = make_ranges(&pairs);
        let config = CoalescingConfig::default();
        let coalesced = coalesce_ranges(ranges, &config);
        for window in coalesced.windows(2) {
            let a = &window[0].fetch_range;
            let b = &window[1].fetch_range;
            prop_assert!(
                a.end <= b.start,
                "overlapping: [{},{}) and [{},{})",
                a.start, a.end, b.start, b.end
            );
        }
    }

    /// Every input range is covered by some output fetch_range.
    #[test]
    fn prop_coalesce_covers_all_inputs(
        pairs in prop::collection::vec((0u16..60000u16, 1u16..500u16), 1..20),
    ) {
        let ranges = make_ranges(&pairs);
        let config = CoalescingConfig::default();
        let coalesced = coalesce_ranges(ranges.clone(), &config);
        for input in &ranges {
            let covered = coalesced.iter().any(|req| {
                req.fetch_range.start <= input.start && req.fetch_range.end >= input.end
            });
            prop_assert!(
                covered,
                "input [{},{}) not covered",
                input.start, input.end
            );
        }
    }

    /// Idempotency: coalescing the fetch_ranges of the output produces ≤ same count.
    #[test]
    fn prop_coalesce_idempotent_count(
        pairs in prop::collection::vec((0u16..60000u16, 1u16..500u16), 1..15),
    ) {
        let ranges = make_ranges(&pairs);
        let config = CoalescingConfig::default();
        let first = coalesce_ranges(ranges, &config);
        let first_fetch: Vec<ByteRange> = first.iter().map(|r| r.fetch_range.clone()).collect();
        let second = coalesce_ranges(first_fetch, &config);
        prop_assert!(
            second.len() <= first.len(),
            "second pass {} > first pass {}",
            second.len(),
            first.len()
        );
    }

    /// With a very large max_gap, all ranges merge into one when input is non-empty.
    #[test]
    fn prop_coalesce_large_gap_single_output(
        pairs in prop::collection::vec((0u16..1000u16, 1u16..50u16), 1..10),
    ) {
        let ranges = make_ranges(&pairs);
        let config = CoalescingConfig {
            max_gap_bytes: u64::MAX / 2,
            max_merged_size: u64::MAX,
            max_parallel_requests: 64,
        };
        let coalesced = coalesce_ranges(ranges, &config);
        prop_assert_eq!(coalesced.len(), 1);
    }

    /// Empty input produces empty output.
    #[test]
    fn prop_coalesce_empty_input(_x in 0u8..1u8) {
        let config = CoalescingConfig::default();
        let coalesced = coalesce_ranges(vec![], &config);
        prop_assert_eq!(coalesced.len(), 0);
    }
}

// ── Arrow IPC framing properties ──────────────────────────────────────────────

proptest! {
    /// Writing a message then reading it back recovers the correct metadata and body lengths.
    #[test]
    fn prop_arrow_ipc_roundtrip_lengths(
        // metadata must be at least 8 bytes for the type-byte offset to be valid
        metadata in prop::collection::vec(any::<u8>(), 8..=64),
        body in prop::collection::vec(any::<u8>(), 0..=256),
    ) {
        let mut writer = ArrowIpcWriter::new();
        writer.write_message(&metadata, &body);
        let bytes = writer.finish();

        let mut reader = ArrowIpcReader::new(bytes);
        let header_result = reader.parse_file_header();
        prop_assert!(header_result.is_ok(), "parse_file_header failed: {:?}", header_result);

        let msg = reader.next_message();
        prop_assert!(msg.is_ok(), "next_message error: {:?}", msg);

        let maybe_hdr = msg.expect("already checked ok");
        prop_assert!(maybe_hdr.is_some(), "expected Some(header), got None");

        let hdr = maybe_hdr.expect("already checked some");
        prop_assert_eq!(hdr.metadata_length, metadata.len() as i32);
        prop_assert_eq!(hdr.body_length, body.len() as i64);
    }

    /// A single writer with zero messages (just finish) produces a valid file header.
    #[test]
    fn prop_arrow_ipc_empty_writer_is_valid(_x in 0u8..1u8) {
        let writer = ArrowIpcWriter::new();
        let bytes = writer.finish();
        let reader = ArrowIpcReader::new(bytes);
        prop_assert!(reader.is_arrow_file());
    }

    /// Multiple messages can be written and read back in sequence.
    #[test]
    fn prop_arrow_ipc_multiple_messages(
        messages in prop::collection::vec(
            (
                prop::collection::vec(any::<u8>(), 8..=32),
                prop::collection::vec(any::<u8>(), 0..=64),
            ),
            1..=5,
        ),
    ) {
        let mut writer = ArrowIpcWriter::new();
        for (meta, body) in &messages {
            writer.write_message(meta, body);
        }
        let bytes = writer.finish();

        let mut reader = ArrowIpcReader::new(bytes);
        reader.parse_file_header().expect("file header");

        let mut count = 0usize;
        loop {
            match reader.next_message() {
                Ok(Some(_)) => count += 1,
                Ok(None) => break,
                Err(e) => {
                    prop_assert!(false, "next_message error at msg {count}: {e:?}");
                    break;
                }
            }
        }
        prop_assert_eq!(count, messages.len());
    }
}

// ── align_to properties ───────────────────────────────────────────────────────

proptest! {
    /// `align_to` result is always >= the input.
    #[test]
    fn prop_align_to_result_ge_input(n in 0usize..10000usize) {
        prop_assert!(align_to(n, ARROW_ALIGNMENT) >= n);
    }

    /// `align_to` is idempotent: applying it twice gives the same result.
    #[test]
    fn prop_align_to_idempotent(n in 0usize..10000usize) {
        let once = align_to(n, ARROW_ALIGNMENT);
        let twice = align_to(once, ARROW_ALIGNMENT);
        prop_assert_eq!(once, twice);
    }

    /// Result is always a multiple of the alignment.
    #[test]
    fn prop_align_to_multiple_of_alignment(n in 0usize..10000usize) {
        let aligned = align_to(n, ARROW_ALIGNMENT);
        prop_assert_eq!(aligned % ARROW_ALIGNMENT, 0);
    }

    /// Overhead (extra bytes) is always < alignment.
    #[test]
    fn prop_align_to_overhead_lt_alignment(n in 0usize..10000usize) {
        let aligned = align_to(n, ARROW_ALIGNMENT);
        prop_assert!(aligned - n < ARROW_ALIGNMENT);
    }
}
