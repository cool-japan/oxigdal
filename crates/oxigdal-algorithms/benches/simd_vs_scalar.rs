//! Comprehensive SIMD vs Scalar Benchmarks
//!
//! This benchmark suite compares SIMD-optimized implementations against equivalent
//! scalar (naive loop) implementations across all four hot-path modules:
//!
//! - **raster**: Element-wise arithmetic, type conversion, masking
//! - **statistics**: Reductions (sum, min, max, variance), Welford, covariance
//! - **math**: sqrt, abs, floor, ceil, round, exp, ln, trig
//! - **filters**: Gaussian blur, Sobel magnitude, separable convolution
//!
//! Each benchmark group contains both a "simd" and a "scalar" variant at identical
//! data sizes so that criterion can produce direct comparisons.

#![allow(missing_docs, clippy::unnecessary_cast)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::simd::{filters, math, raster, statistics};
use std::hint::black_box;

// ============================================================================
//  Inline scalar baselines (intentionally NOT using SIMD intrinsics)
// ============================================================================
#[allow(dead_code)]
mod scalar {
    // ---- raster element-wise ----
    #[inline(never)]
    pub fn add_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i] + b[i];
        }
    }

    #[inline(never)]
    pub fn sub_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i] - b[i];
        }
    }

    #[inline(never)]
    pub fn mul_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i] * b[i];
        }
    }

    #[inline(never)]
    pub fn div_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i] / b[i];
        }
    }

    #[inline(never)]
    pub fn fma_f32(a: &[f32], b: &[f32], c: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i].mul_add(b[i], c[i]);
        }
    }

    #[inline(never)]
    pub fn min_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i].min(b[i]);
        }
    }

    #[inline(never)]
    pub fn max_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i].max(b[i]);
        }
    }

    #[inline(never)]
    pub fn clamp_f32(data: &[f32], lo: f32, hi: f32, out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].clamp(lo, hi);
        }
    }

    #[inline(never)]
    pub fn threshold_f32(data: &[f32], thresh: f32, out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = if data[i] >= thresh { data[i] } else { 0.0 };
        }
    }

    #[inline(never)]
    pub fn scale_offset_f32(data: &[f32], scale: f32, offset: f32, out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i] * scale + offset;
        }
    }

    #[inline(never)]
    pub fn u8_to_f32_normalized(data: &[u8], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i] as f32 / 255.0;
        }
    }

    #[inline(never)]
    pub fn f32_to_u8_normalized(data: &[f32], out: &mut [u8]) {
        for i in 0..data.len() {
            out[i] = (data[i] * 255.0).clamp(0.0, 255.0) as u8;
        }
    }

    #[inline(never)]
    pub fn apply_mask_f32(data: &[f32], mask: &[u8], fill: f32, out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = if mask[i] != 0 { data[i] } else { fill };
        }
    }

    // ---- statistics ----
    #[inline(never)]
    pub fn sum_f32(data: &[f32]) -> f32 {
        let mut acc = 0.0_f32;
        for &v in data {
            acc += v;
        }
        acc
    }

    #[inline(never)]
    pub fn min_stat_f32(data: &[f32]) -> f32 {
        let mut m = f32::INFINITY;
        for &v in data {
            if v < m {
                m = v;
            }
        }
        m
    }

    #[inline(never)]
    pub fn max_stat_f32(data: &[f32]) -> f32 {
        let mut m = f32::NEG_INFINITY;
        for &v in data {
            if v > m {
                m = v;
            }
        }
        m
    }

    #[inline(never)]
    pub fn minmax_f32(data: &[f32]) -> (f32, f32) {
        let mut mn = f32::INFINITY;
        let mut mx = f32::NEG_INFINITY;
        for &v in data {
            if v < mn {
                mn = v;
            }
            if v > mx {
                mx = v;
            }
        }
        (mn, mx)
    }

    #[inline(never)]
    pub fn variance_f32(data: &[f32]) -> f32 {
        let n = data.len() as f32;
        let mean = sum_f32(data) / n;
        let mut acc = 0.0_f32;
        for &v in data {
            let d = v - mean;
            acc += d * d;
        }
        acc / n
    }

    #[inline(never)]
    pub fn welford_variance_f32(data: &[f32]) -> (f32, f32) {
        let mut mean = 0.0_f32;
        let mut m2 = 0.0_f32;
        let mut count = 0_usize;
        for &v in data {
            count += 1;
            let delta = v - mean;
            mean += delta / count as f32;
            let delta2 = v - mean;
            m2 += delta * delta2;
        }
        let variance = if count > 1 { m2 / count as f32 } else { 0.0 };
        (mean, variance)
    }

    #[inline(never)]
    pub fn covariance_f32(a: &[f32], b: &[f32]) -> f32 {
        let n = a.len() as f32;
        let mean_a = sum_f32(a) / n;
        let mean_b = sum_f32(b) / n;
        let mut acc = 0.0_f32;
        for i in 0..a.len() {
            acc += (a[i] - mean_a) * (b[i] - mean_b);
        }
        acc / n
    }

    // ---- math ----
    #[inline(never)]
    pub fn sqrt_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].sqrt();
        }
    }

    #[inline(never)]
    pub fn abs_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].abs();
        }
    }

    #[inline(never)]
    pub fn floor_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].floor();
        }
    }

    #[inline(never)]
    pub fn ceil_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].ceil();
        }
    }

    #[inline(never)]
    pub fn round_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].round();
        }
    }

    #[inline(never)]
    pub fn exp_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].exp();
        }
    }

    #[inline(never)]
    pub fn ln_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].ln();
        }
    }

    #[inline(never)]
    pub fn sin_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].sin();
        }
    }

    #[inline(never)]
    pub fn cos_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].cos();
        }
    }

    #[inline(never)]
    pub fn fract_f32(data: &[f32], out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].fract();
        }
    }

    // ---- filters ----
    #[inline(never)]
    pub fn gaussian_blur_3x3(input: &[u8], output: &mut [u8], w: usize, h: usize) {
        // Direct 3x3 kernel (non-separable)
        #[rustfmt::skip]
        let kernel: [f32; 9] = [
            0.0625, 0.125, 0.0625,
            0.125,  0.25,  0.125,
            0.0625, 0.125, 0.0625,
        ];
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                let mut sum = 0.0_f32;
                for ky in 0..3_usize {
                    for kx in 0..3_usize {
                        let iy = y + ky - 1;
                        let ix = x + kx - 1;
                        sum += input[iy * w + ix] as f32 * kernel[ky * 3 + kx];
                    }
                }
                output[y * w + x] = sum.clamp(0.0, 255.0) as u8;
            }
        }
    }

    #[inline(never)]
    pub fn sobel_magnitude_scalar(gx: &[i16], gy: &[i16], mag: &mut [u8]) {
        for i in 0..gx.len() {
            let fx = gx[i] as f32;
            let fy = gy[i] as f32;
            let m = (fx * fx + fy * fy).sqrt();
            mag[i] = m.clamp(0.0, 255.0) as u8;
        }
    }

    #[inline(never)]
    pub fn separable_convolve_f32(
        data: &[f32],
        output: &mut [f32],
        w: usize,
        h: usize,
        row_kernel: &[f32],
        col_kernel: &[f32],
    ) {
        let kr = row_kernel.len() / 2;
        let kc = col_kernel.len() / 2;
        let mut tmp = vec![0.0_f32; w * h];

        // Horizontal pass
        for y in 0..h {
            for x in kr..w.saturating_sub(kr) {
                let mut sum = 0.0_f32;
                for (k, &rk) in row_kernel.iter().enumerate() {
                    let ix = x + k - kr;
                    sum += data[y * w + ix] * rk;
                }
                tmp[y * w + x] = sum;
            }
        }

        // Vertical pass
        for y in kc..h.saturating_sub(kc) {
            for x in 0..w {
                let mut sum = 0.0_f32;
                for (k, &ck) in col_kernel.iter().enumerate() {
                    let iy = y + k - kc;
                    sum += tmp[iy * w + x] * ck;
                }
                output[y * w + x] = sum;
            }
        }
    }
}

// ============================================================================
//  Helper: generate realistic test data
// ============================================================================
fn make_ramp_f32(n: usize) -> Vec<f32> {
    (0..n).map(|i| (i as f32) * 0.01 + 0.5).collect()
}

fn make_positive_f32(n: usize) -> Vec<f32> {
    (0..n).map(|i| (i as f32) * 0.1 + 1.0).collect()
}

fn make_nonzero_f32(n: usize) -> Vec<f32> {
    (0..n).map(|i| (i as f32) * 0.1 + 1.0).collect()
}

fn make_trig_f32(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (i as f32) * 0.01 - (n as f32 * 0.005))
        .collect()
}

fn make_image_u8(w: usize, h: usize) -> Vec<u8> {
    (0..w * h).map(|i| ((i * 37) % 256) as u8).collect()
}

fn make_gradient_i16(w: usize, h: usize) -> Vec<i16> {
    (0..w * h)
        .map(|i| ((i as i32 * 17 - 500) % 512) as i16)
        .collect()
}

// ============================================================================
//  1. RASTER element-wise benchmarks: SIMD vs Scalar
// ============================================================================

fn bench_raster_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_add_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let a = make_ramp_f32(size);
        let b = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = raster::add_f32(black_box(&a), black_box(&b), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::add_f32(black_box(&a), black_box(&b), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_raster_mul(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_mul_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let a = make_ramp_f32(size);
        let b = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = raster::mul_f32(black_box(&a), black_box(&b), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::mul_f32(black_box(&a), black_box(&b), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_raster_div(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_div_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let a = make_ramp_f32(size);
        let b = make_nonzero_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = raster::div_f32(black_box(&a), black_box(&b), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::div_f32(black_box(&a), black_box(&b), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_raster_fma(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_fma_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let a = make_ramp_f32(size);
        let b = make_ramp_f32(size);
        let cv = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = raster::fma_f32(
                    black_box(&a),
                    black_box(&b),
                    black_box(&cv),
                    black_box(&mut out),
                );
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::fma_f32(
                    black_box(&a),
                    black_box(&b),
                    black_box(&cv),
                    black_box(&mut out),
                );
            });
        });
    }
    group.finish();
}

fn bench_raster_clamp(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_clamp_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = raster::clamp_f32(black_box(&data), 0.5, 5.0, black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::clamp_f32(black_box(&data), 0.5, 5.0, black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_raster_threshold(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_threshold_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = raster::threshold_f32(black_box(&data), 5.0, black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::threshold_f32(black_box(&data), 5.0, black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_raster_scale_offset(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_scale_offset_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = raster::scale_offset_f32(black_box(&data), 2.5, 10.0, black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::scale_offset_f32(black_box(&data), 2.5, 10.0, black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_raster_type_convert(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_type_conversion");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let u8_data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        let f32_data: Vec<f32> = (0..size).map(|i| (i % 256) as f32 / 255.0).collect();
        let mut f32_out = vec![0.0_f32; size];
        let mut u8_out = vec![0_u8; size];

        group.bench_with_input(
            BenchmarkId::new("u8_to_f32_simd", size),
            &size,
            |bench, _| {
                bench.iter(|| {
                    let _ =
                        raster::u8_to_f32_normalized(black_box(&u8_data), black_box(&mut f32_out));
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("u8_to_f32_scalar", size),
            &size,
            |bench, _| {
                bench.iter(|| {
                    scalar::u8_to_f32_normalized(black_box(&u8_data), black_box(&mut f32_out));
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("f32_to_u8_simd", size),
            &size,
            |bench, _| {
                bench.iter(|| {
                    let _ =
                        raster::f32_to_u8_normalized(black_box(&f32_data), black_box(&mut u8_out));
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("f32_to_u8_scalar", size),
            &size,
            |bench, _| {
                bench.iter(|| {
                    scalar::f32_to_u8_normalized(black_box(&f32_data), black_box(&mut u8_out));
                });
            },
        );
    }
    group.finish();
}

fn bench_raster_mask(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_apply_mask_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);
        let mask: Vec<u8> = (0..size).map(|i| if i % 3 == 0 { 0 } else { 1 }).collect();
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = raster::apply_mask_f32(
                    black_box(&data),
                    black_box(&mask),
                    -9999.0,
                    black_box(&mut out),
                );
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::apply_mask_f32(
                    black_box(&data),
                    black_box(&mask),
                    -9999.0,
                    black_box(&mut out),
                );
            });
        });
    }
    group.finish();
}

// ============================================================================
//  2. STATISTICS reduction benchmarks: SIMD vs Scalar
// ============================================================================

fn bench_stats_sum(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats_sum_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                black_box(statistics::sum_f32(black_box(&data)));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                black_box(scalar::sum_f32(black_box(&data)));
            });
        });
    }
    group.finish();
}

fn bench_stats_minmax(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats_minmax_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = black_box(statistics::minmax_f32(black_box(&data)));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                black_box(scalar::minmax_f32(black_box(&data)));
            });
        });
    }
    group.finish();
}

fn bench_stats_variance(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats_variance_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = black_box(statistics::variance_f32(black_box(&data)));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                black_box(scalar::variance_f32(black_box(&data)));
            });
        });
    }
    group.finish();
}

fn bench_stats_welford(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats_welford_variance_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = black_box(statistics::welford_variance_f32(black_box(&data)));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                black_box(scalar::welford_variance_f32(black_box(&data)));
            });
        });
    }
    group.finish();
}

fn bench_stats_covariance(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats_covariance_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let a = make_ramp_f32(size);
        let b: Vec<f32> = a.iter().map(|&v| v * 2.0 + 1.0).collect();

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = black_box(statistics::covariance_f32(black_box(&a), black_box(&b)));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                black_box(scalar::covariance_f32(black_box(&a), black_box(&b)));
            });
        });
    }
    group.finish();
}

// ============================================================================
//  3. MATH operation benchmarks: SIMD vs Scalar
// ============================================================================

fn bench_math_sqrt(c: &mut Criterion) {
    let mut group = c.benchmark_group("math_sqrt_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_positive_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = math::sqrt_f32(black_box(&data), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::sqrt_f32(black_box(&data), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_math_abs(c: &mut Criterion) {
    let mut group = c.benchmark_group("math_abs_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_trig_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = math::abs_f32(black_box(&data), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::abs_f32(black_box(&data), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_math_floor(c: &mut Criterion) {
    let mut group = c.benchmark_group("math_floor_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = math::floor_f32(black_box(&data), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::floor_f32(black_box(&data), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_math_ceil(c: &mut Criterion) {
    let mut group = c.benchmark_group("math_ceil_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = math::ceil_f32(black_box(&data), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::ceil_f32(black_box(&data), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_math_round(c: &mut Criterion) {
    let mut group = c.benchmark_group("math_round_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = math::round_f32(black_box(&data), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::round_f32(black_box(&data), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_math_exp(c: &mut Criterion) {
    let mut group = c.benchmark_group("math_exp_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data: Vec<f32> = (0..size).map(|i| (i as f32 * 0.001) - 5.0).collect();
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = math::exp_f32(black_box(&data), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::exp_f32(black_box(&data), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_math_ln(c: &mut Criterion) {
    let mut group = c.benchmark_group("math_ln_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_positive_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = math::ln_f32(black_box(&data), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::ln_f32(black_box(&data), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_math_sin(c: &mut Criterion) {
    let mut group = c.benchmark_group("math_sin_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_trig_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = math::sin_f32(black_box(&data), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::sin_f32(black_box(&data), black_box(&mut out));
            });
        });
    }
    group.finish();
}

fn bench_math_fract(c: &mut Criterion) {
    let mut group = c.benchmark_group("math_fract_f32");
    for &size in &[1_000, 10_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size as u64));
        let data = make_ramp_f32(size);
        let mut out = vec![0.0_f32; size];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = math::fract_f32(black_box(&data), black_box(&mut out));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::fract_f32(black_box(&data), black_box(&mut out));
            });
        });
    }
    group.finish();
}

// ============================================================================
//  4. FILTERS benchmarks: SIMD vs Scalar
// ============================================================================

fn bench_filters_gaussian(c: &mut Criterion) {
    let mut group = c.benchmark_group("filters_gaussian_blur_3x3");
    for &size in &[128, 256, 512, 1024] {
        let pixels = size * size;
        group.throughput(Throughput::Elements(pixels as u64));
        let input = make_image_u8(size, size);
        let mut output = vec![0_u8; pixels];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = filters::gaussian_blur_3x3(
                    black_box(&input),
                    black_box(&mut output),
                    size,
                    size,
                );
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::gaussian_blur_3x3(black_box(&input), black_box(&mut output), size, size);
            });
        });
    }
    group.finish();
}

fn bench_filters_sobel_magnitude(c: &mut Criterion) {
    let mut group = c.benchmark_group("filters_sobel_magnitude");
    for &size in &[128, 256, 512, 1024] {
        let pixels = size * size;
        group.throughput(Throughput::Elements(pixels as u64));
        let gx = make_gradient_i16(size, size);
        let gy = make_gradient_i16(size, size);
        let mut mag = vec![0_u8; pixels];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ =
                    filters::sobel_magnitude(black_box(&gx), black_box(&gy), black_box(&mut mag));
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::sobel_magnitude_scalar(black_box(&gx), black_box(&gy), black_box(&mut mag));
            });
        });
    }
    group.finish();
}

fn bench_filters_separable_convolve(c: &mut Criterion) {
    let mut group = c.benchmark_group("filters_separable_convolve_f32");
    let row_kernel = vec![0.25_f32, 0.5, 0.25];
    let col_kernel = vec![0.25_f32, 0.5, 0.25];

    for &size in &[128, 256, 512, 1024] {
        let pixels = size * size;
        group.throughput(Throughput::Elements(pixels as u64));
        let data: Vec<f32> = (0..pixels).map(|i| (i % 256) as f32).collect();
        let mut output = vec![0.0_f32; pixels];

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |bench, _| {
            bench.iter(|| {
                let _ = filters::separable_convolve_f32(
                    black_box(&data),
                    black_box(&mut output),
                    size,
                    size,
                    black_box(&row_kernel),
                    black_box(&col_kernel),
                );
            });
        });
        group.bench_with_input(BenchmarkId::new("scalar", size), &size, |bench, _| {
            bench.iter(|| {
                scalar::separable_convolve_f32(
                    black_box(&data),
                    black_box(&mut output),
                    size,
                    size,
                    black_box(&row_kernel),
                    black_box(&col_kernel),
                );
            });
        });
    }
    group.finish();
}

// ============================================================================
//  Criterion groups and main
// ============================================================================

criterion_group!(
    raster_benches,
    bench_raster_add,
    bench_raster_mul,
    bench_raster_div,
    bench_raster_fma,
    bench_raster_clamp,
    bench_raster_threshold,
    bench_raster_scale_offset,
    bench_raster_type_convert,
    bench_raster_mask,
);

criterion_group!(
    stats_benches,
    bench_stats_sum,
    bench_stats_minmax,
    bench_stats_variance,
    bench_stats_welford,
    bench_stats_covariance,
);

criterion_group!(
    math_benches,
    bench_math_sqrt,
    bench_math_abs,
    bench_math_floor,
    bench_math_ceil,
    bench_math_round,
    bench_math_exp,
    bench_math_ln,
    bench_math_sin,
    bench_math_fract,
);

criterion_group!(
    filter_benches,
    bench_filters_gaussian,
    bench_filters_sobel_magnitude,
    bench_filters_separable_convolve,
);

criterion_main!(raster_benches, stats_benches, math_benches, filter_benches);
