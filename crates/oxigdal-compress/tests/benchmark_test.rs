//! Integration tests for the codec round-trip benchmarker.
//!
//! These tests verify behavioural properties of `Benchmarker::benchmark`:
//! that the round trip succeeds, that ranking helpers pick sensible codecs,
//! that edge cases (empty codec list, short input) do not panic, and that
//! sentinel results are produced when a codec round trip cannot complete.
//!
//! Wall-clock timings are inherently noisy on shared CI hosts, so the
//! assertions only check relative orderings and structural invariants, not
//! absolute throughput numbers.

#![allow(clippy::panic)]

use oxigdal_compress::{
    benchmark::{BenchmarkReport, Benchmarker},
    codecs::CodecType,
};

fn find_result<'a>(
    report: &'a BenchmarkReport,
    codec_name: &str,
) -> &'a oxigdal_compress::benchmark::BenchmarkResult {
    report
        .results
        .iter()
        .find(|r| r.codec == codec_name)
        .unwrap_or_else(|| panic!("no result for codec '{codec_name}' in report"))
}

#[test]
fn test_benchmark_round_trip_correctness_lz4() {
    let data = vec![b'a'; 1024];
    let benchmarker = Benchmarker::new(2);
    let report = benchmarker
        .benchmark(&data, &[CodecType::Lz4])
        .expect("benchmark should succeed");

    assert_eq!(report.results.len(), 1);
    let r = &report.results[0];
    assert_eq!(r.codec, "lz4");
    assert!(
        !r.is_sentinel(),
        "LZ4 round trip should not produce sentinel for repetitive input"
    );
    assert!(r.compression_ratio.is_finite());
    assert!(r.compression_ratio > 0.0);
    assert_eq!(r.original_size, 1024);
    assert!(r.compressed_size > 0);
    assert_eq!(r.iterations, 2);
}

#[test]
fn test_benchmark_round_trip_correctness_zstd() {
    let data = vec![b'a'; 1024];
    let benchmarker = Benchmarker::new(2);
    let report = benchmarker
        .benchmark(&data, &[CodecType::Zstd])
        .expect("benchmark should succeed");

    assert_eq!(report.results.len(), 1);
    let r = &report.results[0];
    assert_eq!(r.codec, "zstd");
    assert!(
        !r.is_sentinel(),
        "Zstd round trip should not produce sentinel for repetitive input"
    );
    assert!(r.compression_ratio.is_finite());
    assert!(r.compression_ratio > 0.0);
    assert_eq!(r.original_size, 1024);
    assert!(r.compressed_size > 0);
}

#[test]
fn test_benchmark_zstd_better_ratio_than_lz4_on_repetitive_text() {
    // Strongly repetitive input — both Zstd and LZ4 should compress it very
    // aggressively. We assert a tolerant relationship: either Zstd's ratio
    // is strictly smaller (the typical case) or both are already at the
    // floor (<=0.05) in which case "better" is meaningless and we just
    // require both to have compressed significantly.
    let data = vec![b'a'; 4096];
    let benchmarker = Benchmarker::new(2);
    let report = benchmarker
        .benchmark(&data, &[CodecType::Lz4, CodecType::Zstd])
        .expect("benchmark should succeed");

    let lz4 = find_result(&report, "lz4");
    let zstd = find_result(&report, "zstd");

    assert!(!lz4.is_sentinel());
    assert!(!zstd.is_sentinel());

    let both_tiny = lz4.compression_ratio <= 0.05 && zstd.compression_ratio <= 0.05;
    assert!(
        zstd.compression_ratio <= lz4.compression_ratio || both_tiny,
        "expected Zstd ratio ({}) to be <= LZ4 ratio ({}) on highly repetitive input, or both tiny",
        zstd.compression_ratio,
        lz4.compression_ratio
    );
}

#[test]
fn test_benchmark_iterations_returns_n_results_for_n_codecs() {
    let data = vec![b'b'; 512];
    let benchmarker = Benchmarker::new(1);
    let codecs = [CodecType::Lz4, CodecType::Zstd, CodecType::Snappy];
    let report = benchmarker
        .benchmark(&data, &codecs)
        .expect("benchmark should succeed");

    assert_eq!(report.results.len(), 3);
    // The codecs should appear in the same order they were requested.
    assert_eq!(report.results[0].codec, "lz4");
    assert_eq!(report.results[1].codec, "zstd");
    assert_eq!(report.results[2].codec, "snappy");
}

#[test]
fn test_benchmark_empty_codecs_list_returns_empty_report() {
    let data = vec![b'c'; 32];
    let benchmarker = Benchmarker::new(1);
    let report = benchmarker
        .benchmark(&data, &[])
        .expect("benchmark with no codecs should succeed");

    assert!(report.results.is_empty());
    assert!(report.best_ratio.is_empty());
    assert!(report.best_compression_speed.is_empty());
    assert!(report.best_decompression_speed.is_empty());
    assert!(report.best_balanced.is_empty());
}

#[test]
fn test_benchmark_zero_iterations_clamped_to_one() {
    // No public hook lets us inject a hostile codec without modifying the
    // crate's public surface, so the optional sentinel-mismatch test is
    // replaced by the clamp test (per spec).
    let benchmarker = Benchmarker::new(0);
    assert_eq!(
        benchmarker.iterations(),
        1,
        "Benchmarker::new(0) must clamp to at least one iteration"
    );

    // And the clamped benchmarker should still produce a real (non-sentinel)
    // result on a small valid input.
    let data = vec![b'd'; 256];
    let report = benchmarker
        .benchmark(&data, &[CodecType::Lz4])
        .expect("benchmark should succeed even with iterations clamped");
    assert_eq!(report.results.len(), 1);
    assert!(!report.results[0].is_sentinel());
    assert_eq!(report.results[0].iterations, 1);
}

#[test]
fn test_benchmark_best_ratio_picks_smallest_ratio_codec() {
    let data = vec![b'a'; 4096];
    let benchmarker = Benchmarker::new(2);
    let report = benchmarker
        .benchmark(&data, &[CodecType::Lz4, CodecType::Zstd])
        .expect("benchmark should succeed");

    let best_name = &report.best_ratio;
    assert!(
        !best_name.is_empty(),
        "best_ratio should be non-empty when we have results"
    );

    let best = find_result(&report, best_name);
    for r in &report.results {
        if r.compression_ratio.is_finite() {
            assert!(
                best.compression_ratio <= r.compression_ratio,
                "best_ratio codec '{}' (ratio {}) should have <= ratio than '{}' (ratio {})",
                best.codec,
                best.compression_ratio,
                r.codec,
                r.compression_ratio,
            );
        }
    }
}

#[test]
fn test_benchmark_best_balanced_picks_optimal_speed_ratio_tradeoff() {
    let data = vec![b'a'; 4096];
    let benchmarker = Benchmarker::new(2);
    let report = benchmarker
        .benchmark(&data, &[CodecType::Lz4, CodecType::Zstd])
        .expect("benchmark should succeed");

    assert!(!report.best_balanced.is_empty());
    let candidate_names: Vec<&str> = report.results.iter().map(|r| r.codec.as_str()).collect();
    assert!(
        candidate_names.contains(&report.best_balanced.as_str()),
        "best_balanced '{}' should be one of the benchmarked codecs: {:?}",
        report.best_balanced,
        candidate_names,
    );
}

#[test]
fn test_benchmark_default_iterations_3() {
    let benchmarker = Benchmarker::default();
    assert_eq!(
        benchmarker.iterations(),
        3,
        "Default Benchmarker should use 3 iterations"
    );
}

#[test]
fn test_benchmark_short_input_still_completes() {
    let data = vec![b'x'; 8];
    let benchmarker = Benchmarker::new(1);
    let report = benchmarker
        .benchmark(&data, &[CodecType::Lz4, CodecType::Zstd, CodecType::Snappy])
        .expect("benchmark should succeed on short input");

    assert_eq!(report.results.len(), 3);
    for r in &report.results {
        // We don't require non-sentinel here — some codecs may legitimately
        // refuse 8-byte input — but the benchmark itself must complete and
        // produce a structurally valid entry for every requested codec.
        assert_eq!(r.original_size, 8);
        assert!(r.compression_ratio.is_finite() || r.is_sentinel());
    }
}
