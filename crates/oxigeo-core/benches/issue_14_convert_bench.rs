//! Bulk raster conversion microbenchmark (GitHub issue #14).
//!
//! Answers the question the issue actually asks: is [`convert_raw_into`] — the
//! kernel behind `Dataset::read_band_into` — at least as fast as the "decode to
//! bytes, then run a separate vectorised widening map" workaround it is meant to
//! replace?
//!
//! Three implementations are timed per case:
//!
//! * `bulk` — the current [`convert_raw_into`];
//! * `legacy` — the per-sample `from_ne_slice` loop the bulk paths replaced,
//!   reconstructed from the public [`RasterElement`] surface so the comparison
//!   needs no old checkout;
//! * `map` — the ceiling: `dst[i] = convert(src[i])` over an already-typed
//!   `&[S]`, i.e. exactly the `mapv`-style pass the issue reporter measured at
//!   4.8 ms for `f32 → f64` over 16 Mpx.
//!
//! # Methodology
//!
//! The three are **interleaved** within each round and their order is **rotated**
//! between rounds, so neither ordering nor a transient system hiccup can favour
//! one of them. The reported figure is the **minimum** over the rounds (with the
//! median printed next to it): on a loaded machine the minimum is the estimator
//! least polluted by other processes' time, and every implementation gets the
//! same number of chances at it. Each case also asserts the three outputs are
//! byte-identical, so a "faster" number can never come from doing less work.
//!
//! Two scales are reported:
//!
//! * **hot** — a working set that fits in cache, repeated many times. This
//!   isolates the *kernel*: it is where auto-vectorisation shows up.
//! * **issue scale** — 4000 × 4000 = 16 Mpx, the reporter's raster. At this size
//!   a widening pass is largely memory-bandwidth bound, which caps how much any
//!   kernel improvement can show.
//!
//! Run with `cargo bench -p oxigeo-core --bench issue_14_convert_bench`.

use oxigeo_core::buffer::{RasterElement, RasterElementKind, convert_raw_into, elements_as_bytes};
use std::time::{Duration, Instant};

/// 4000 × 4000, the raster size from the issue report.
const PIXELS: usize = 4000 * 4000;
/// Samples in the cache-resident case (≤ 512 KiB source).
const HOT_PIXELS: usize = 64 * 1024;
/// Repeats of the hot kernel inside one timed run.
const HOT_REPEATS: usize = 256;
/// Rounds per case.
const ROUNDS: usize = 15;

/// One sample conversion, matching [`convert_raw_into`]'s default semantics
/// (saturating, floats rounded to nearest).
#[inline]
fn convert_one<S: RasterElement, D: RasterElement>(value: S) -> D {
    match (S::KIND, D::KIND) {
        (RasterElementKind::Integer, RasterElementKind::Integer) => {
            D::from_raster_i128(value.to_raster_i128())
        }
        _ => D::from_raster_f64(value.to_raster_f64()),
    }
}

/// The pre-bulk kernel: one sample decoded per byte chunk.
fn legacy_convert<S: RasterElement, D: RasterElement>(src: &[u8], dst: &mut [D]) {
    for (chunk, out) in src.chunks_exact(S::SIZE).zip(dst.iter_mut()) {
        *out = convert_one::<S, D>(S::from_ne_slice(chunk));
    }
}

/// The ceiling: a plain widening map over an already-typed slice.
fn map_convert<S: RasterElement, D: RasterElement>(src: &[S], dst: &mut [D]) {
    for (value, out) in src.iter().zip(dst.iter_mut()) {
        *out = convert_one::<S, D>(*value);
    }
}

/// Minimum and median of a set of samples.
fn stats(mut samples: Vec<Duration>) -> (Duration, Duration) {
    samples.sort_unstable();
    (
        samples.first().copied().unwrap_or(Duration::ZERO),
        samples
            .get(samples.len() / 2)
            .copied()
            .unwrap_or(Duration::ZERO),
    )
}

/// Milliseconds, for printing.
fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

/// Timing results for one implementation.
struct Timing {
    /// Best observed wall time.
    best: Duration,
    /// Median observed wall time.
    median: Duration,
}

/// Times the three implementations of `S → D` over `values` and prints one row.
///
/// `offset` bytes of padding are inserted in front of the raw source, so
/// `offset = 1` measures the misaligned (staged) path. `repeats` is how many
/// times each implementation runs inside one timed sample.
fn run_case<S: RasterElement, D: RasterElement>(
    label: &str,
    values: &[S],
    offset: usize,
    repeats: usize,
) {
    run_case_chunked::<S, D>(label, values, offset, repeats, 0);
}

/// [`run_case`], but each implementation is invoked over `chunk`-sample
/// sub-slices instead of the whole band (`chunk = 0` means "whole band").
///
/// This is how the GeoTIFF driver really calls the conversion: one call per
/// tile row, i.e. 256 samples at a time on a 256×256 tiled raster. It is the
/// only way per-call overhead — validation, the twelve-way data-type dispatch,
/// the vector prologue/epilogue — can show up in a measurement.
fn run_case_chunked<S: RasterElement, D: RasterElement>(
    label: &str,
    values: &[S],
    offset: usize,
    repeats: usize,
    chunk: usize,
) {
    let mut raw = vec![0xA5u8; offset];
    raw.extend_from_slice(elements_as_bytes(values));
    let src = raw.get(offset..).unwrap_or(&[]);

    let mut outputs: Vec<Vec<D>> = (0..3).map(|_| vec![D::default(); values.len()]).collect();
    let mut samples: Vec<Vec<Duration>> = vec![Vec::new(); 3];
    let mut failed = false;

    for round in 0..ROUNDS {
        // Rotate which implementation runs first.
        for slot in 0..3 {
            let which = (slot + round) % 3;
            let Some(out) = outputs.get_mut(which) else {
                failed = true;
                break;
            };
            let start = Instant::now();
            for _ in 0..repeats {
                if chunk == 0 {
                    match which {
                        0 => {
                            if convert_raw_into(src, S::DATA_TYPE, out).is_err() {
                                failed = true;
                            }
                        }
                        1 => legacy_convert::<S, D>(src, out),
                        _ => map_convert::<S, D>(values, out),
                    }
                } else {
                    // The driver learns the source type from the file header, so
                    // it is a *runtime* value at the call site. `black_box` keeps
                    // it one here as well: with a constant the length validation
                    // folds to shifts on its own, which would hide exactly the
                    // per-call cost this case exists to measure.
                    let src_type = std::hint::black_box(S::DATA_TYPE);
                    for (index, part) in out.chunks_mut(chunk).enumerate() {
                        let start_byte = index * chunk * S::SIZE;
                        let piece = src
                            .get(start_byte..start_byte + part.len() * S::SIZE)
                            .unwrap_or(&[]);
                        let typed = values
                            .get(index * chunk..index * chunk + part.len())
                            .unwrap_or(&[]);
                        match which {
                            0 => {
                                if convert_raw_into(piece, src_type, part).is_err() {
                                    failed = true;
                                }
                            }
                            1 => legacy_convert::<S, D>(piece, part),
                            _ => map_convert::<S, D>(typed, part),
                        }
                    }
                }
            }
            let elapsed = start.elapsed() / (repeats.max(1) as u32);
            if let Some(bucket) = samples.get_mut(which) {
                bucket.push(elapsed);
            }
        }
        if failed {
            break;
        }
    }

    if failed {
        eprintln!("{label}: conversion rejected the input");
        return;
    }

    // A faster number is only worth having if it is the same number.
    let identical = outputs
        .first()
        .map(|first| {
            outputs
                .iter()
                .all(|other| elements_as_bytes(other) == elements_as_bytes(first))
        })
        .unwrap_or(false);
    if !identical {
        eprintln!("{label}: MISMATCH between conversion paths");
        return;
    }

    let timings: Vec<Timing> = samples
        .into_iter()
        .map(|bucket| {
            let (best, median) = stats(bucket);
            Timing { best, median }
        })
        .collect();
    let Some(((bulk, legacy), map)) = timings.first().zip(timings.get(1)).zip(timings.get(2))
    else {
        return;
    };

    println!(
        "{label:<32} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.2}x {:>8.2}x",
        ms(bulk.best),
        ms(bulk.median),
        ms(legacy.best),
        ms(legacy.median),
        ms(map.best),
        ms(map.median),
        ms(legacy.best) / ms(bulk.best),
        ms(map.best) / ms(bulk.best),
    );
}

/// Prints the column header.
fn header(scale: &str) {
    println!("\n=== {scale} ===");
    println!(
        "{:<32} {:>17} {:>17} {:>17} {:>9} {:>9}",
        "case", "bulk best/med ms", "legacy best/med", "map best/med", "vs legacy", "vs map"
    );
}

/// Runs the whole matrix at one scale.
fn run_matrix(count: usize, repeats: usize) {
    let f32_values: Vec<f32> = (0..count).map(|i| (i % 4096) as f32 * 0.5).collect();
    run_case::<f32, f64>("f32 -> f64 (aligned)", &f32_values, 0, repeats);
    run_case::<f32, f64>("f32 -> f64 (misaligned src)", &f32_values, 1, repeats);
    run_case::<f32, f32>("f32 -> f32 (identity memcpy)", &f32_values, 0, repeats);
    run_case::<f32, i16>("f32 -> i16 (saturating)", &f32_values, 0, repeats);
    drop(f32_values);

    let u8_values: Vec<u8> = (0..count).map(|i| (i % 251) as u8).collect();
    run_case::<u8, f32>("u8   -> f32", &u8_values, 0, repeats);
    run_case::<u8, f64>("u8   -> f64", &u8_values, 0, repeats);
    run_case::<u8, u16>("u8   -> u16", &u8_values, 0, repeats);
    drop(u8_values);

    let u16_values: Vec<u16> = (0..count).map(|i| (i % 65_521) as u16).collect();
    run_case::<u16, f32>("u16  -> f32", &u16_values, 0, repeats);
    run_case::<u16, f64>("u16  -> f64", &u16_values, 0, repeats);
    run_case::<u16, f64>("u16  -> f64 (misaligned src)", &u16_values, 1, repeats);
    drop(u16_values);

    let i16_values: Vec<i16> = (0..count).map(|i| (i % 32_749) as i16 - 16_000).collect();
    run_case::<i16, f32>("i16  -> f32", &i16_values, 0, repeats);
    run_case::<i16, f64>("i16  -> f64", &i16_values, 0, repeats);
    drop(i16_values);

    let f64_values: Vec<f64> = (0..count).map(|i| (i % 4096) as f64 * 0.5).collect();
    run_case::<f64, f32>("f64  -> f32", &f64_values, 0, repeats);
    run_case::<f64, i32>("f64  -> i32 (saturating)", &f64_values, 0, repeats);
}

fn main() {
    println!("min/median of {ROUNDS} interleaved, order-rotated rounds");

    header(&format!(
        "hot kernel: {HOT_PIXELS} samples x {HOT_REPEATS} repeats (cache resident)"
    ));
    run_matrix(HOT_PIXELS, HOT_REPEATS);

    header(&format!(
        "issue scale: {PIXELS} samples (4000x4000), memory bound"
    ));
    run_matrix(PIXELS, 1);

    // The call pattern the GeoTIFF driver actually produces on a 256x256 tiled
    // raster: 62 500 calls of 256 samples each, not one call of 16 Mpx.
    header("driver call pattern: 256 samples per call, 16 Mpx total");
    let f32_values: Vec<f32> = (0..PIXELS).map(|i| (i % 4096) as f32 * 0.5).collect();
    run_case_chunked::<f32, f64>("f32 -> f64 (256/call)", &f32_values, 0, 1, 256);
    run_case_chunked::<f32, f32>("f32 -> f32 (256/call)", &f32_values, 0, 1, 256);
    drop(f32_values);
    let u16_values: Vec<u16> = (0..PIXELS).map(|i| (i % 65_521) as u16).collect();
    run_case_chunked::<u16, f32>("u16  -> f32 (256/call)", &u16_values, 0, 1, 256);
}
