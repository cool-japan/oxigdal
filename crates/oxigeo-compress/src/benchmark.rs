//! Built-in compression benchmarking
//!
//! Provides utilities for benchmarking compression codecs.
//!
//! Timings are wall-clock measurements collected over a small number of
//! iterations after a short warmup. They are indicative, not authoritative —
//! CI noise, thermal throttling, and concurrent work on the same host can
//! perturb absolute numbers. Relative orderings (e.g. "ratio of Zstd is
//! smaller than ratio of LZ4 on repetitive text") tend to be stable across
//! environments.

use crate::{
    codecs::{
        BrotliCodec, CodecType, DeflateCodec, DeltaCodec, DictionaryCodec, Lz4Codec, RleCodec,
        SnappyCodec, ZstdCodec,
    },
    error::Result,
    metadata::CompressionMetadata,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Benchmark result for a single codec
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Codec name
    pub codec: String,

    /// Original size
    pub original_size: usize,

    /// Compressed size
    pub compressed_size: usize,

    /// Compression ratio (compressed_size / original_size, smaller is better)
    pub compression_ratio: f64,

    /// Space savings percentage (1.0 - ratio, times 100)
    pub space_savings: f64,

    /// Compression time (mean across iterations)
    pub compression_time: Duration,

    /// Decompression time (mean across iterations)
    pub decompression_time: Duration,

    /// Compression throughput (MB/s, MB = 1_000_000 bytes)
    pub compression_throughput: f64,

    /// Decompression throughput (MB/s, MB = 1_000_000 bytes)
    pub decompression_throughput: f64,

    /// Number of iterations used to compute the timings (0 indicates a
    /// sentinel result for a codec whose round trip failed or whose
    /// construction was not wired into the benchmarker)
    pub iterations: usize,
}

impl BenchmarkResult {
    /// Create from compression metadata and decompression time
    pub fn from_metadata(metadata: CompressionMetadata, decompression_time: Duration) -> Self {
        let decompression_throughput = if decompression_time.as_secs_f64() > 0.0 {
            (metadata.original_size as f64 / decompression_time.as_secs_f64()) / 1_048_576.0
        } else {
            0.0
        };

        Self {
            codec: metadata.codec,
            original_size: metadata.original_size,
            compressed_size: metadata.compressed_size,
            compression_ratio: metadata.compression_ratio,
            space_savings: metadata.space_savings,
            compression_time: metadata.duration.unwrap_or(Duration::ZERO),
            decompression_time,
            compression_throughput: metadata.throughput.unwrap_or(0.0),
            decompression_throughput,
            iterations: 1,
        }
    }

    /// Format as human-readable string
    pub fn format_summary(&self) -> String {
        format!(
            "{}: {:.2}x ratio ({:.1}% savings), compress: {:.2} MB/s, decompress: {:.2} MB/s",
            self.codec,
            self.compression_ratio,
            self.space_savings,
            self.compression_throughput,
            self.decompression_throughput
        )
    }

    /// Returns true if this result is a sentinel placeholder (round-trip
    /// failure or codec construction error)
    pub fn is_sentinel(&self) -> bool {
        self.iterations == 0 || !self.compression_ratio.is_finite()
    }
}

/// Benchmark comparison report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// All benchmark results
    pub results: Vec<BenchmarkResult>,

    /// Best codec for compression ratio (smallest ratio wins)
    pub best_ratio: String,

    /// Best codec for compression speed (largest throughput wins)
    pub best_compression_speed: String,

    /// Best codec for decompression speed (largest throughput wins)
    pub best_decompression_speed: String,

    /// Best codec for balanced performance
    pub best_balanced: String,
}

impl BenchmarkReport {
    /// Create from benchmark results
    pub fn new(results: Vec<BenchmarkResult>) -> Self {
        let best_ratio = best_by(&results, |r| r.compression_ratio, true);
        let best_compression_speed = best_by(&results, |r| r.compression_throughput, false);
        let best_decompression_speed = best_by(&results, |r| r.decompression_throughput, false);
        let best_balanced = best_by(
            &results,
            |r| r.compression_throughput * (1.0 - r.compression_ratio).max(0.0),
            false,
        );

        Self {
            results,
            best_ratio,
            best_compression_speed,
            best_decompression_speed,
            best_balanced,
        }
    }

    /// Format as human-readable report
    pub fn format_report(&self) -> String {
        let mut report = String::new();

        report.push_str("Compression Benchmark Report\n");
        report.push_str("=============================\n\n");

        for result in &self.results {
            report.push_str(&result.format_summary());
            report.push('\n');
        }

        report.push_str("\nSummary:\n");
        report.push_str(&format!("  Best Ratio: {}\n", self.best_ratio));
        report.push_str(&format!(
            "  Best Compression Speed: {}\n",
            self.best_compression_speed
        ));
        report.push_str(&format!(
            "  Best Decompression Speed: {}\n",
            self.best_decompression_speed
        ));
        report.push_str(&format!("  Best Balanced: {}\n", self.best_balanced));

        report
    }
}

/// Benchmark runner
pub struct Benchmarker {
    /// Number of measured iterations per codec (always >= 1)
    iterations: usize,
}

impl Benchmarker {
    /// Create new benchmarker.
    ///
    /// `iterations` is clamped to a minimum of 1 — zero iterations would
    /// produce a divide-by-zero in throughput computation.
    pub fn new(iterations: usize) -> Self {
        Self {
            iterations: iterations.max(1),
        }
    }

    /// Number of measured iterations this benchmarker uses per codec
    pub fn iterations(&self) -> usize {
        self.iterations
    }

    /// Run a real round-trip benchmark across the given codecs against the
    /// given data.
    ///
    /// Each codec is constructed from its `CodecType`, warmed up with a few
    /// throwaway compress/decompress cycles, and then timed across the
    /// configured number of iterations. The compressed bytes are decompressed
    /// and verified byte-for-byte against the input on every iteration; a
    /// single mismatch (or a compress/decompress error, or a codec that the
    /// benchmarker does not yet know how to construct) is recorded as a
    /// sentinel result with `compression_ratio = f64::INFINITY` and zero
    /// throughputs rather than aborting the whole benchmark.
    ///
    /// Timings are wall-clock and are best treated as indicative.
    pub fn benchmark(&self, data: &[u8], codecs: &[CodecType]) -> Result<BenchmarkReport> {
        let mut results = Vec::with_capacity(codecs.len());
        for &ct in codecs {
            let name = ct.name().to_string();
            let res = match make_bench_codec(ct) {
                Ok(codec) => self.benchmark_one(codec.as_ref(), &name, data),
                Err(_) => sentinel_result(&name, data.len()),
            };
            results.push(res);
        }

        let best_ratio = best_by(&results, |r| r.compression_ratio, true);
        let best_compression_speed = best_by(&results, |r| r.compression_throughput, false);
        let best_decompression_speed = best_by(&results, |r| r.decompression_throughput, false);
        let best_balanced = best_by(
            &results,
            |r| r.compression_throughput * (1.0 - r.compression_ratio).max(0.0),
            false,
        );

        Ok(BenchmarkReport {
            results,
            best_ratio,
            best_compression_speed,
            best_decompression_speed,
            best_balanced,
        })
    }

    fn benchmark_one(&self, codec: &dyn BenchCodec, name: &str, data: &[u8]) -> BenchmarkResult {
        // Warmup — a few unmeasured round trips to let allocators/caches warm.
        for _ in 0..3 {
            if let Ok(c) = codec.compress_bytes(data) {
                let _ = codec.decompress_bytes(&c, data.len());
            }
        }

        let mut compress_durs = Vec::with_capacity(self.iterations);
        let mut decompress_durs = Vec::with_capacity(self.iterations);
        let mut compressed_len = 0usize;

        for _ in 0..self.iterations {
            let t = Instant::now();
            let compressed = match codec.compress_bytes(data) {
                Ok(c) => c,
                Err(_) => return sentinel_result(name, data.len()),
            };
            compress_durs.push(t.elapsed());
            compressed_len = compressed.len();

            let t = Instant::now();
            let decompressed = match codec.decompress_bytes(&compressed, data.len()) {
                Ok(d) => d,
                Err(_) => return sentinel_result(name, data.len()),
            };
            decompress_durs.push(t.elapsed());

            if decompressed != data {
                return sentinel_result(name, data.len());
            }
        }

        let mean_dur_c = mean_duration(&compress_durs);
        let mean_dur_d = mean_duration(&decompress_durs);
        let mb = data.len() as f64 / 1_000_000.0;
        let c_mbps = if mean_dur_c.as_secs_f64() > 0.0 {
            mb / mean_dur_c.as_secs_f64()
        } else {
            0.0
        };
        let d_mbps = if mean_dur_d.as_secs_f64() > 0.0 {
            mb / mean_dur_d.as_secs_f64()
        } else {
            0.0
        };
        let ratio = if data.is_empty() {
            1.0
        } else {
            compressed_len as f64 / data.len() as f64
        };
        let space_savings = ((1.0 - ratio) * 100.0).max(0.0);

        BenchmarkResult {
            codec: name.to_string(),
            original_size: data.len(),
            compressed_size: compressed_len,
            compression_ratio: ratio,
            space_savings,
            compression_time: mean_dur_c,
            decompression_time: mean_dur_d,
            compression_throughput: c_mbps,
            decompression_throughput: d_mbps,
            iterations: self.iterations,
        }
    }
}

impl Default for Benchmarker {
    fn default() -> Self {
        Self::new(3)
    }
}

// ---------------------------------------------------------------------------
// Internal codec adapter
// ---------------------------------------------------------------------------

/// Private adapter trait — uniformises the slight signature differences
/// between concrete codecs (some `decompress` variants take a hint).
trait BenchCodec {
    fn compress_bytes(&self, data: &[u8]) -> Result<Vec<u8>>;
    fn decompress_bytes(&self, data: &[u8], original_size_hint: usize) -> Result<Vec<u8>>;
}

impl BenchCodec for Lz4Codec {
    fn compress_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.compress(data)
    }
    fn decompress_bytes(&self, data: &[u8], original_size_hint: usize) -> Result<Vec<u8>> {
        self.decompress(data, Some(original_size_hint))
    }
}

impl BenchCodec for ZstdCodec {
    fn compress_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.compress(data)
    }
    fn decompress_bytes(&self, data: &[u8], original_size_hint: usize) -> Result<Vec<u8>> {
        self.decompress(data, Some(original_size_hint))
    }
}

impl BenchCodec for SnappyCodec {
    fn compress_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.compress(data)
    }
    fn decompress_bytes(&self, data: &[u8], _original_size_hint: usize) -> Result<Vec<u8>> {
        self.decompress(data)
    }
}

impl BenchCodec for BrotliCodec {
    fn compress_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.compress(data)
    }
    fn decompress_bytes(&self, data: &[u8], _original_size_hint: usize) -> Result<Vec<u8>> {
        self.decompress(data)
    }
}

impl BenchCodec for DeflateCodec {
    fn compress_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.compress(data)
    }
    fn decompress_bytes(&self, data: &[u8], _original_size_hint: usize) -> Result<Vec<u8>> {
        self.decompress(data)
    }
}

impl BenchCodec for DeltaCodec {
    fn compress_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.compress(data)
    }
    fn decompress_bytes(&self, data: &[u8], _original_size_hint: usize) -> Result<Vec<u8>> {
        self.decompress(data)
    }
}

impl BenchCodec for RleCodec {
    fn compress_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.compress(data)
    }
    fn decompress_bytes(&self, data: &[u8], _original_size_hint: usize) -> Result<Vec<u8>> {
        self.decompress(data)
    }
}

impl BenchCodec for DictionaryCodec {
    fn compress_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.compress(data)
    }
    fn decompress_bytes(&self, data: &[u8], _original_size_hint: usize) -> Result<Vec<u8>> {
        self.decompress(data)
    }
}

fn make_bench_codec(ct: CodecType) -> Result<Box<dyn BenchCodec>> {
    Ok(match ct {
        CodecType::Lz4 => Box::new(Lz4Codec::new()),
        CodecType::Zstd => Box::new(ZstdCodec::new()),
        CodecType::Snappy => Box::new(SnappyCodec::new()),
        CodecType::Brotli => Box::new(BrotliCodec::new()),
        CodecType::Deflate => Box::new(DeflateCodec::new()),
        CodecType::Delta => Box::new(DeltaCodec::new()),
        CodecType::Rle => Box::new(RleCodec::new()),
        CodecType::Dictionary => Box::new(DictionaryCodec::new()),
    })
}

fn mean_duration(durs: &[Duration]) -> Duration {
    if durs.is_empty() {
        return Duration::ZERO;
    }
    let total: Duration = durs.iter().copied().sum();
    let n = u32::try_from(durs.len()).unwrap_or(u32::MAX);
    if n == 0 { Duration::ZERO } else { total / n }
}

fn sentinel_result(name: &str, original_size: usize) -> BenchmarkResult {
    BenchmarkResult {
        codec: name.to_string(),
        original_size,
        compressed_size: 0,
        compression_ratio: f64::INFINITY,
        space_savings: 0.0,
        compression_time: Duration::ZERO,
        decompression_time: Duration::ZERO,
        compression_throughput: 0.0,
        decompression_throughput: 0.0,
        iterations: 0,
    }
}

fn best_by<F: Fn(&BenchmarkResult) -> f64>(
    results: &[BenchmarkResult],
    key: F,
    smaller_is_better: bool,
) -> String {
    let mut best: Option<(&BenchmarkResult, f64)> = None;
    for r in results {
        let k = key(r);
        if !k.is_finite() {
            continue;
        }
        best = Some(match best {
            None => (r, k),
            Some((cur, cur_k)) => {
                let new_better = if smaller_is_better {
                    k < cur_k
                } else {
                    k > cur_k
                };
                if new_better { (r, k) } else { (cur, cur_k) }
            }
        });
    }
    best.map(|(r, _)| r.codec.clone()).unwrap_or_default()
}
