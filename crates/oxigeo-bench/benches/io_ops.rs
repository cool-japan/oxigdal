//! I/O operations benchmarks using Criterion.

#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::io::Read;

fn bench_sequential_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_read");

    for size in [4096, 8192, 16384, 65536].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            size,
            |b, &buffer_size| {
                b.iter(|| {
                    // Simulate sequential read
                    let data = vec![0u8; buffer_size];
                    black_box(data);
                });
            },
        );
    }

    group.finish();
}

fn bench_sequential_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_write");

    for size in [4096, 8192, 16384, 65536].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            size,
            |b, &buffer_size| {
                let data = vec![0u8; buffer_size];
                b.iter(|| {
                    // Simulate sequential write
                    black_box(&data);
                });
            },
        );
    }

    group.finish();
}

fn bench_random_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_access");

    group.sample_size(50);

    for chunk_size in [512, 1024, 4096].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(chunk_size),
            chunk_size,
            |b, &size| {
                let data = vec![0u8; 1024 * 1024]; // 1MB buffer
                b.iter(|| {
                    // Simulate random access pattern
                    let mut sum: u64 = 0;
                    for i in (0..data.len()).step_by(size) {
                        if i + size <= data.len() {
                            sum = sum.wrapping_add(data[i] as u64);
                        }
                    }
                    black_box(sum);
                });
            },
        );
    }

    group.finish();
}

fn bench_buffered_vs_unbuffered(c: &mut Criterion) {
    use std::io::BufReader;

    let mut group = c.benchmark_group("buffered_io");

    let data = vec![0u8; 1024 * 1024]; // 1MB of data

    group.bench_function("unbuffered", |b| {
        b.iter(|| {
            let cursor = std::io::Cursor::new(&data);
            let mut reader = cursor;
            let mut buffer = vec![0u8; 8192];
            let mut total = 0;

            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => total += n,
                    Err(_) => break,
                }
            }
            black_box(total);
        });
    });

    group.bench_function("buffered", |b| {
        b.iter(|| {
            let cursor = std::io::Cursor::new(&data);
            let mut reader = BufReader::new(cursor);
            let mut buffer = vec![0u8; 8192];
            let mut total = 0;

            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => total += n,
                    Err(_) => break,
                }
            }
            black_box(total);
        });
    });

    group.finish();
}

fn bench_chunked_io(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunked_io");

    let data = vec![0u8; 1024 * 1024]; // 1MB of data

    for chunk_size in [512, 1024, 4096, 8192, 16384].iter() {
        group.throughput(Throughput::Bytes(*chunk_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(chunk_size),
            chunk_size,
            |b, &size| {
                b.iter(|| {
                    let mut offset = 0;
                    while offset < data.len() {
                        let end = (offset + size).min(data.len());
                        black_box(&data[offset..end]);
                        offset = end;
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    io_benches,
    bench_sequential_read,
    bench_sequential_write,
    bench_random_access,
    bench_buffered_vs_unbuffered,
    bench_chunked_io
);

criterion_main!(io_benches);
