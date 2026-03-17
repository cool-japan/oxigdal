//! Integration tests for performance-oriented streaming modules:
//! `io_coalescing`, `mmap`, and `arrow_ipc`.

use std::io::Write;

use oxigdal_streaming::arrow_ipc::{
    ARROW_MAGIC, ARROW_MAGIC_LEN, ArrowIpcReader, ArrowIpcWriter, IpcBuffer, align_to,
};
use oxigdal_streaming::io_coalescing::{
    ByteRange, CoalescingConfig, coalesce_ranges, compute_stats,
};
use oxigdal_streaming::mmap::{MappedFile, PrefetchHint, PrefetchPriority, PrefetchScheduler};

// ── ByteRange / Coalescing tests ──────────────────────────────────────────────

#[test]
fn byte_range_new_and_len() {
    let r = ByteRange::new(10, 20);
    assert_eq!(r.start, 10);
    assert_eq!(r.end, 20);
    assert_eq!(r.len(), 10);
}

#[test]
fn byte_range_is_empty_true() {
    let r = ByteRange::new(5, 5);
    assert!(r.is_empty());
}

#[test]
fn byte_range_is_empty_false() {
    let r = ByteRange::new(5, 6);
    assert!(!r.is_empty());
}

#[test]
fn byte_range_overlaps_or_adjoins_same() {
    let r = ByteRange::new(10, 20);
    assert!(r.overlaps_or_adjoins(&ByteRange::new(10, 20)));
}

#[test]
fn byte_range_overlaps_or_adjoins_adjacent() {
    let a = ByteRange::new(0, 10);
    let b = ByteRange::new(10, 20);
    assert!(a.overlaps_or_adjoins(&b));
}

#[test]
fn byte_range_overlaps_or_adjoins_gap() {
    let a = ByteRange::new(0, 10);
    let b = ByteRange::new(11, 20);
    assert!(!a.overlaps_or_adjoins(&b));
}

#[test]
fn byte_range_overlaps_or_adjoins_overlap() {
    let a = ByteRange::new(0, 15);
    let b = ByteRange::new(10, 20);
    assert!(a.overlaps_or_adjoins(&b));
}

#[test]
fn byte_range_merge() {
    let a = ByteRange::new(0, 10);
    let b = ByteRange::new(15, 25);
    let m = a.merge(&b);
    assert_eq!(m.start, 0);
    assert_eq!(m.end, 25);
}

#[test]
fn byte_range_gap_to() {
    let a = ByteRange::new(0, 10);
    let b = ByteRange::new(15, 25);
    assert_eq!(a.gap_to(&b), 5);
}

#[test]
fn byte_range_gap_to_adjacent() {
    let a = ByteRange::new(0, 10);
    let b = ByteRange::new(10, 20);
    assert_eq!(a.gap_to(&b), 0);
}

#[test]
fn coalesce_empty_list_returns_empty() {
    let result = coalesce_ranges(vec![], &CoalescingConfig::default());
    assert!(result.is_empty());
}

#[test]
fn coalesce_single_range_returns_one_request() {
    let ranges = vec![ByteRange::new(0, 100)];
    let result = coalesce_ranges(ranges, &CoalescingConfig::default());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].fetch_range, ByteRange::new(0, 100));
}

#[test]
fn coalesce_small_gap_merges_ranges() {
    let config = CoalescingConfig {
        max_gap_bytes: 16,
        max_merged_size: 1024 * 1024,
        max_parallel_requests: 8,
    };
    let ranges = vec![ByteRange::new(0, 100), ByteRange::new(110, 200)];
    let result = coalesce_ranges(ranges, &config);
    assert_eq!(result.len(), 1, "gap=10 < max_gap=16 → should merge");
    assert_eq!(result[0].fetch_range, ByteRange::new(0, 200));
    assert_eq!(result[0].sub_ranges.len(), 2);
}

#[test]
fn coalesce_large_gap_keeps_separate() {
    let config = CoalescingConfig {
        max_gap_bytes: 8,
        max_merged_size: 1024 * 1024,
        max_parallel_requests: 8,
    };
    let ranges = vec![ByteRange::new(0, 100), ByteRange::new(200, 300)];
    let result = coalesce_ranges(ranges, &config);
    assert_eq!(result.len(), 2, "gap=100 > max_gap=8 → must stay separate");
}

#[test]
fn coalesce_exceeds_max_merged_size_splits() {
    let config = CoalescingConfig {
        max_gap_bytes: 1024,
        max_merged_size: 50,
        max_parallel_requests: 8,
    };
    // The merged size would be 100 bytes, exceeding max_merged_size=50.
    let ranges = vec![ByteRange::new(0, 40), ByteRange::new(45, 100)];
    let result = coalesce_ranges(ranges, &config);
    assert_eq!(result.len(), 2, "merged=100 > max=50 → must split");
}

#[test]
fn coalesce_deduplicates_identical_ranges() {
    let ranges = vec![ByteRange::new(0, 100), ByteRange::new(0, 100)];
    let result = coalesce_ranges(ranges, &CoalescingConfig::default());
    // After dedup: only one range.
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].sub_ranges.len(), 1);
}

#[test]
fn coalesced_request_extract_correct_slice() {
    let config = CoalescingConfig {
        max_gap_bytes: 20,
        max_merged_size: 1024,
        max_parallel_requests: 8,
    };
    let ranges = vec![ByteRange::new(0, 10), ByteRange::new(15, 25)];
    let result = coalesce_ranges(ranges, &config);
    assert_eq!(result.len(), 1);

    // Build a synthetic merged buffer: bytes 0..25 filled with index value.
    let merged: Vec<u8> = (0u8..25).collect();
    let cr = &result[0];

    let slice = cr.extract(&merged, &ByteRange::new(0, 10));
    assert!(slice.is_some());
    assert_eq!(slice.unwrap(), &merged[0..10]);

    let slice2 = cr.extract(&merged, &ByteRange::new(15, 25));
    assert!(slice2.is_some());
    assert_eq!(slice2.unwrap(), &merged[15..25]);
}

#[test]
fn coalesced_request_extract_out_of_range_returns_none() {
    let ranges = vec![ByteRange::new(10, 20)];
    let result = coalesce_ranges(ranges, &CoalescingConfig::default());
    let merged = vec![0u8; 10];
    // Sub-range 0..5 is before fetch_range.start=10 → None.
    let cr = &result[0];
    assert!(cr.extract(&merged, &ByteRange::new(0, 5)).is_none());
}

#[test]
fn compute_stats_request_reduction() {
    let original = vec![
        ByteRange::new(0, 100),
        ByteRange::new(200, 300),
        ByteRange::new(400, 500),
        ByteRange::new(600, 700),
    ];
    let config = CoalescingConfig {
        max_gap_bytes: 150,
        max_merged_size: 1024 * 1024,
        max_parallel_requests: 8,
    };
    let coalesced = coalesce_ranges(original.clone(), &config);
    let stats = compute_stats(&original, &coalesced);

    assert!(stats.request_reduction() > 0.0);
    assert!(stats.coalesced_requests < stats.original_requests);
}

#[test]
fn compute_stats_overhead_ratio_zero_when_no_gaps() {
    // Two adjacent ranges: no gap fill-in → overhead should be 0.
    let original = vec![ByteRange::new(0, 50), ByteRange::new(50, 100)];
    let config = CoalescingConfig {
        max_gap_bytes: 0,
        max_merged_size: 1024,
        max_parallel_requests: 8,
    };
    let coalesced = coalesce_ranges(original.clone(), &config);
    let stats = compute_stats(&original, &coalesced);
    assert_eq!(stats.overhead_ratio(), 0.0);
}

#[test]
fn coalesce_output_ranges_are_sorted() {
    // Input is provided in reverse order to verify sorting.
    let ranges = vec![
        ByteRange::new(900, 1000),
        ByteRange::new(0, 100),
        ByteRange::new(500, 600),
    ];
    let result = coalesce_ranges(ranges, &CoalescingConfig::default());
    // Verify fetch ranges are in ascending start order.
    for w in result.windows(2) {
        assert!(w[0].fetch_range.start <= w[1].fetch_range.start);
    }
}

// ── MappedFile tests ──────────────────────────────────────────────────────────

fn write_temp_file(content: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().expect("tempfile");
    f.write_all(content).expect("write");
    f
}

#[test]
fn mapped_file_open_nonexistent_returns_error() {
    let result = MappedFile::open("/tmp/oxigdal_nonexistent_xyz_12345.bin");
    assert!(result.is_err());
}

#[test]
fn mapped_file_read_range_within_bounds() {
    let data = b"Hello, World!";
    let f = write_temp_file(data);
    let mf = MappedFile::open(f.path()).expect("open");
    let slice = mf.read_range(0, 5).expect("read_range");
    assert_eq!(slice, b"Hello");
}

#[test]
fn mapped_file_read_range_past_end_returns_error() {
    let data = b"short";
    let f = write_temp_file(data);
    let mf = MappedFile::open(f.path()).expect("open");
    assert!(mf.read_range(0, 100).is_err());
}

#[test]
fn mapped_file_read_range_zero_length() {
    let data = b"anything";
    let f = write_temp_file(data);
    let mf = MappedFile::open(f.path()).expect("open");
    let slice = mf.read_range(3, 0).expect("zero-length read");
    assert!(slice.is_empty());
}

#[test]
fn mapped_file_file_size_correct() {
    let data = b"1234567890";
    let f = write_temp_file(data);
    let mf = MappedFile::open(f.path()).expect("open");
    assert_eq!(mf.file_size(), 10);
}

#[test]
fn mapped_file_as_slice_returns_all_data() {
    let data = b"abcdefgh";
    let f = write_temp_file(data);
    let mf = MappedFile::open(f.path()).expect("open");
    assert_eq!(mf.as_slice(), data.as_ref());
}

#[test]
fn mapped_file_read_ranges_multiple() {
    let data = b"0123456789";
    let f = write_temp_file(data);
    let mf = MappedFile::open(f.path()).expect("open");
    let ranges = vec![(0u64, 3usize), (5u64, 3usize)];
    let results = mf.read_ranges(&ranges);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].as_deref().expect("r0"), b"012");
    assert_eq!(results[1].as_deref().expect("r1"), b"567");
}

#[test]
fn prefetch_hint_construction() {
    let hint = PrefetchHint {
        offset: 1024,
        length: 4096,
        priority: PrefetchPriority::High,
    };
    assert_eq!(hint.offset, 1024);
    assert_eq!(hint.length, 4096);
    assert_eq!(hint.priority, PrefetchPriority::High);
}

#[test]
fn prefetch_scheduler_add_and_count() {
    let mut sched = PrefetchScheduler::new(64 * 1024);
    sched.add_hint(PrefetchHint {
        offset: 0,
        length: 1024,
        priority: PrefetchPriority::Normal,
    });
    sched.add_hint(PrefetchHint {
        offset: 2048,
        length: 512,
        priority: PrefetchPriority::Low,
    });
    assert_eq!(sched.hint_count(), 2);
    assert_eq!(sched.total_bytes_hinted(), 1536);
}

#[test]
fn prefetch_scheduler_sorted_hints_respects_priority_order() {
    let mut sched = PrefetchScheduler::new(1024 * 1024);
    sched.add_hint(PrefetchHint {
        offset: 100,
        length: 10,
        priority: PrefetchPriority::Low,
    });
    sched.add_hint(PrefetchHint {
        offset: 200,
        length: 10,
        priority: PrefetchPriority::High,
    });
    sched.add_hint(PrefetchHint {
        offset: 300,
        length: 10,
        priority: PrefetchPriority::Normal,
    });
    let sorted = sched.sorted_hints();
    assert_eq!(sorted.len(), 3);
    assert_eq!(sorted[0].priority, PrefetchPriority::High);
    assert_eq!(sorted[1].priority, PrefetchPriority::Normal);
    assert_eq!(sorted[2].priority, PrefetchPriority::Low);
}

// ── ArrowIPC tests ────────────────────────────────────────────────────────────

fn make_arrow_buffer(messages: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut writer = ArrowIpcWriter::new();
    for (meta, body) in messages {
        writer.write_message(meta, body);
    }
    writer.finish()
}

#[test]
fn arrow_ipc_is_arrow_file_with_magic() {
    let buf = make_arrow_buffer(&[]);
    let reader = ArrowIpcReader::new(buf);
    assert!(reader.is_arrow_file());
}

#[test]
fn arrow_ipc_is_arrow_file_without_magic_returns_false() {
    let buf = vec![0u8; 32];
    let reader = ArrowIpcReader::new(buf);
    assert!(!reader.is_arrow_file());
}

#[test]
fn arrow_ipc_writer_finish_contains_magic() {
    let buf = make_arrow_buffer(&[]);
    assert!(buf.starts_with(ARROW_MAGIC));
    assert!(buf.ends_with(ARROW_MAGIC));
}

#[test]
fn align_to_zero() {
    assert_eq!(align_to(0, ARROW_MAGIC_LEN), 0);
}

#[test]
fn align_to_already_aligned() {
    assert_eq!(align_to(8, 8), 8);
    assert_eq!(align_to(16, 8), 16);
}

#[test]
fn align_to_rounds_up() {
    assert_eq!(align_to(1, 8), 8);
    assert_eq!(align_to(9, 8), 16);
    assert_eq!(align_to(7, 8), 8);
}

#[test]
fn arrow_ipc_parse_file_header_advances_offset() {
    let buf = make_arrow_buffer(&[]);
    let mut reader = ArrowIpcReader::new(buf);
    reader.parse_file_header().expect("header");
    // Should have advanced past 6-byte magic + 2-byte padding.
    assert_eq!(reader.current_offset(), ARROW_MAGIC_LEN + 2);
}

#[test]
fn arrow_ipc_parse_file_header_invalid_returns_error() {
    let buf = vec![0xFFu8; 32];
    let mut reader = ArrowIpcReader::new(buf);
    assert!(reader.parse_file_header().is_err());
}

#[test]
fn arrow_ipc_next_message_empty_buffer_returns_none() {
    let buf = vec![0u8; 0];
    let mut reader = ArrowIpcReader::new(buf);
    let msg = reader.next_message().expect("no error on empty");
    assert!(msg.is_none());
}

#[test]
fn arrow_ipc_next_message_eos_marker_returns_none() {
    // EOS marker: continuation(0xFFFFFFFF) + metadata_length(0).
    let mut buf = Vec::new();
    buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    let mut reader = ArrowIpcReader::new(buf);
    let msg = reader.next_message().expect("no error on EOS");
    assert!(msg.is_none());
}

#[test]
fn arrow_ipc_read_buffer_correct_slice() {
    // Build a simple file with one message.
    let metadata = vec![0u8; 8]; // 8-byte placeholder metadata.
    let body = b"BODYDATA";
    let buf = make_arrow_buffer(&[(&metadata, body)]);

    let mut reader = ArrowIpcReader::new(buf);
    reader.parse_file_header().expect("header");
    let hdr = reader.next_message().expect("message").expect("some");

    let ipc_buf = IpcBuffer {
        offset: 0,
        length: body.len() as i64,
    };
    let slice = reader.read_buffer(hdr.body_offset, &ipc_buf);
    assert!(slice.is_some());
    assert_eq!(slice.unwrap(), body);
}

#[test]
fn arrow_ipc_writer_round_trip_single_message() {
    let metadata = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
    let body = b"test_body_payload";
    let buf = make_arrow_buffer(&[(&metadata, body)]);

    let mut reader = ArrowIpcReader::new(buf);
    reader.parse_file_header().expect("header");
    let hdr = reader
        .next_message()
        .expect("no error")
        .expect("message present");

    assert_eq!(hdr.metadata_length, 8);
    assert_eq!(hdr.body_length, body.len() as i64);

    let ipc_buf = IpcBuffer {
        offset: 0,
        length: body.len() as i64,
    };
    assert_eq!(
        reader.read_buffer(hdr.body_offset, &ipc_buf).expect("body"),
        body
    );
}

#[test]
fn arrow_ipc_body_offset_computed_correctly() {
    // After magic(8) + continuation(4) + meta_len(4) + aligned_meta + body_len(8)
    // = body_offset.
    let metadata = vec![0u8; 8]; // exactly aligned
    let body = b"HELLO";
    let buf = make_arrow_buffer(&[(&metadata, body)]);

    let mut reader = ArrowIpcReader::new(buf);
    reader.parse_file_header().expect("header");
    let hdr = reader.next_message().expect("msg").expect("some");

    // Layout: magic(6)+pad(2) + continuation(4) + meta_len(4) + aligned_meta(8) + body_len(8) = 32.
    assert!(hdr.body_offset >= 32, "body_offset={}", hdr.body_offset);
}

#[test]
fn arrow_ipc_multiple_messages_round_trip() {
    let messages: Vec<(Vec<u8>, &[u8])> = vec![
        (vec![0u8; 8], b"FIRST"),
        (vec![1u8; 8], b"SECOND"),
        (vec![2u8; 8], b"THIRD"),
    ];

    let mut writer = ArrowIpcWriter::new();
    for (meta, body) in &messages {
        writer.write_message(meta, body);
    }
    let buf = writer.finish();

    let mut reader = ArrowIpcReader::new(buf);
    reader.parse_file_header().expect("header");

    let mut count = 0usize;
    while let Some(hdr) = reader.next_message().expect("no error") {
        let (_, expected_body) = &messages[count];
        let ipc_buf = IpcBuffer {
            offset: 0,
            length: hdr.body_length,
        };
        let got = reader
            .read_buffer(hdr.body_offset, &ipc_buf)
            .expect("body slice");
        assert_eq!(got, *expected_body);
        count += 1;
    }
    assert_eq!(count, 3);
}

#[test]
fn arrow_ipc_writer_default_produces_valid_magic() {
    let buf = ArrowIpcWriter::default().finish();
    assert!(buf.starts_with(ARROW_MAGIC));
    assert!(buf.ends_with(ARROW_MAGIC));
}
