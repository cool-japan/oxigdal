# OxiGDAL Performance Guide

This guide covers performance optimization techniques for OxiGDAL applications.

## Table of Contents

- [Quick Wins](#quick-wins)
- [Profiling](#profiling)
- [Memory Optimization](#memory-optimization)
- [Parallelization](#parallelization)
- [SIMD](#simd-vectorization)
- [I/O Optimization](#io-optimization)
- [Caching Strategies](#caching-strategies)
- [Cloud Performance](#cloud-performance)

## Quick Wins

### 1. Always Build in Release Mode

```bash
# Development (slow, debug info)
cargo build

# Production (fast, optimized)
cargo build --release
```

**Impact**: 10-100x speedup

### 2. Enable Native CPU Features

```bash
# Use all CPU features available
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

**Impact**: 10-30% speedup for SIMD operations

### 3. Link-Time Optimization

```toml
# Cargo.toml
[profile.release]
lto = true
codegen-units = 1
```

**Impact**: 5-15% speedup, longer compile time

## Profiling

### CPU Profiling with Flamegraph

```bash
# Install
cargo install flamegraph

# Profile
cargo flamegraph --example my_example

# Output: flamegraph.svg
```

### Memory Profiling

```bash
# Install valgrind
cargo install cargo-valgrind

# Profile
cargo valgrind run --example my_example
```

### Benchmarking

```rust
// benches/my_bench.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_ndvi(c: &mut Criterion) {
    let nir = create_test_data();
    let red = create_test_data();

    c.bench_function("ndvi_calculation", |b| {
        b.iter(|| {
            calculate_ndvi(black_box(&nir), black_box(&red))
        });
    });
}

criterion_group!(benches, benchmark_ndvi);
criterion_main!(benches);
```

```bash
cargo bench
```

## Memory Optimization

### 1. Reuse Buffers

**Bad:**
```rust
for i in 0..1000 {
    let buffer = RasterBuffer::zeros(1024, 1024, RasterDataType::Float32);
    process(&buffer)?;
    // buffer dropped and reallocated every iteration
}
```

**Good:**
```rust
let mut buffer = RasterBuffer::zeros(1024, 1024, RasterDataType::Float32);

for i in 0..1000 {
    // Reuse buffer
    fill_buffer(&mut buffer, i)?;
    process(&buffer)?;
}
```

**Impact**: 10-50x speedup for small buffers

### 2. Use Appropriate Data Types

```rust
// Oversized
let buffer = RasterBuffer::zeros(256, 256, RasterDataType::Float64); // 512 KB

// Right-sized
let buffer = RasterBuffer::zeros(256, 256, RasterDataType::UInt8);  // 64 KB
```

### 3. Memory Pooling

```rust
use std::sync::Arc;
use dashmap::DashMap;

struct BufferPool {
    pool: DashMap<(u32, u32), Vec<RasterBuffer>>,
}

impl BufferPool {
    fn acquire(&self, width: u32, height: u32) -> RasterBuffer {
        self.pool
            .get_mut(&(width, height))
            .and_then(|mut vec| vec.pop())
            .unwrap_or_else(|| {
                RasterBuffer::zeros(width, height, RasterDataType::Float32)
            })
    }

    fn release(&self, buffer: RasterBuffer) {
        let key = (buffer.width(), buffer.height());
        self.pool.entry(key)
            .or_insert_with(Vec::new)
            .push(buffer);
    }
}
```

## Parallelization

### 1. Rayon for Data Parallelism

```rust
use rayon::prelude::*;

// Serial
for tile in tiles.iter() {
    process(tile)?;
}

// Parallel
tiles.par_iter()
    .for_each(|tile| {
        process(tile).unwrap();
    });
```

**Impact**: Near-linear speedup with CPU cores

### 2. Chunking for Better Load Balance

```rust
// Process in chunks to balance work
tiles.par_chunks(100)
    .for_each(|chunk| {
        for tile in chunk {
            process(tile).unwrap();
        }
    });
```

### 3. Avoiding False Sharing

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

// Bad: Cache line sharing
struct Counter {
    count1: AtomicUsize,
    count2: AtomicUsize,
}

// Good: Pad to prevent false sharing
#[repr(align(64))]
struct PaddedCounter {
    count: AtomicUsize,
}

struct Counters {
    counter1: PaddedCounter,
    counter2: PaddedCounter,
}
```

## SIMD Vectorization

### 1. Use SimdBuffer

```rust
use oxigdal_core::simd_buffer::SimdBuffer;

// Convert to SIMD
let simd_buffer = SimdBuffer::from_buffer(&buffer)?;

// Vectorized operations
let result = simd_buffer
    .multiply_scalar(2.0)?
    .add_scalar(10.0)?;

// Convert back
let output = result.to_buffer()?;
```

**Impact**: 4-8x speedup for arithmetic operations

### 2. Ensure Alignment

```rust
// Aligned allocation
let mut data = vec![0.0f32; 1024];
assert_eq!(data.as_ptr() as usize % 32, 0);  // 32-byte aligned for AVX
```

### 3. Use Chunked Processing

```rust
// Process in SIMD-friendly chunks
const CHUNK_SIZE: usize = 8;  // Process 8 floats at once

for chunk in data.chunks_exact_mut(CHUNK_SIZE) {
    // SIMD operations on chunk
}

// Handle remainder
for value in data.chunks_exact_mut(CHUNK_SIZE).remainder() {
    // Scalar operations
}
```

## I/O Optimization

### 1. Buffered I/O

```rust
use std::io::BufReader;
use std::fs::File;

// Unbuffered (slow)
let file = File::open("data.tif")?;

// Buffered (fast)
let file = File::open("data.tif")?;
let buffered = BufReader::with_capacity(1024 * 1024, file); // 1MB buffer
```

### 2. Async I/O

```rust
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::open("data.tif").await?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;

    Ok(())
}
```

### 3. Memory-Mapped I/O

```rust
use memmap2::Mmap;
use std::fs::File;

let file = File::open("large_file.tif")?;
let mmap = unsafe { Mmap::map(&file)? };

// Access data without reading entire file
let data = &mmap[offset..offset+size];
```

**When to use:**
- Random access patterns
- Large files
- Read-heavy workloads

### 4. Parallel I/O

```rust
use rayon::prelude::*;

let files = vec!["file1.tif", "file2.tif", "file3.tif"];

let data: Vec<_> = files.par_iter()
    .map(|filename| {
        read_file(filename).unwrap()
    })
    .collect();
```

## Caching Strategies

### 1. LRU Cache

```rust
use lru::LruCache;
use std::sync::Mutex;

struct TileCache {
    cache: Mutex<LruCache<(u32, u32, u32), RasterBuffer>>,
}

impl TileCache {
    fn new(capacity: usize) -> Self {
        Self {
            cache: Mutex::new(LruCache::new(capacity)),
        }
    }

    fn get(&self, z: u32, x: u32, y: u32) -> Option<RasterBuffer> {
        self.cache.lock().unwrap().get(&(z, x, y)).cloned()
    }

    fn put(&self, z: u32, x: u32, y: u32, tile: RasterBuffer) {
        self.cache.lock().unwrap().put((z, x, y), tile);
    }
}
```

### 2. Prefetching

```rust
// Predict next tiles based on access pattern
fn prefetch_tiles(current: (u32, u32), direction: Direction) -> Vec<(u32, u32)> {
    match direction {
        Direction::North => vec![
            (current.0, current.1 - 1),
            (current.0 - 1, current.1 - 1),
            (current.0 + 1, current.1 - 1),
        ],
        // ... other directions
    }
}

// Async prefetch in background
tokio::spawn(async move {
    for tile_coord in prefetch_tiles(current, direction) {
        if !cache.contains(tile_coord) {
            let tile = fetch_tile(tile_coord).await?;
            cache.insert(tile_coord, tile);
        }
    }
});
```

### 3. Hierarchical Caching

```rust
struct HierarchicalCache {
    l1: LruCache<Key, Value>,      // Fast, small (memory)
    l2: LruCache<Key, Value>,      // Medium (disk)
    l3: RemoteCache,               // Slow, large (S3/etc)
}

impl HierarchicalCache {
    fn get(&mut self, key: &Key) -> Option<Value> {
        // Check L1
        if let Some(value) = self.l1.get(key) {
            return Some(value.clone());
        }

        // Check L2
        if let Some(value) = self.l2.get(key) {
            self.l1.put(key.clone(), value.clone());
            return Some(value);
        }

        // Check L3
        if let Some(value) = self.l3.get(key) {
            self.l2.put(key.clone(), value.clone());
            self.l1.put(key.clone(), value.clone());
            return Some(value);
        }

        None
    }
}
```

## Cloud Performance

### 1. Range Requests

```rust
use reqwest::header::{RANGE, HeaderValue};

let range = format!("bytes={}-{}", start, end);
let response = client
    .get(url)
    .header(RANGE, HeaderValue::from_str(&range)?)
    .send()
    .await?;
```

**Impact**: Only download needed data

### 2. Connection Pooling

```rust
use reqwest::Client;
use std::time::Duration;

let client = Client::builder()
    .pool_max_idle_per_host(10)
    .timeout(Duration::from_secs(30))
    .build()?;

// Reuse client for multiple requests
let resp1 = client.get(url1).send().await?;
let resp2 = client.get(url2).send().await?;
```

### 3. Parallel Downloads

```rust
use futures::future::join_all;

let urls = vec![url1, url2, url3];

let futures = urls.iter().map(|url| {
    fetch_tile(url)
});

let tiles = join_all(futures).await;
```

### 4. Compression

```rust
use flate2::read::GzDecoder;

// Download compressed data
let compressed = download_compressed(url).await?;

// Decompress
let mut decoder = GzDecoder::new(&compressed[..]);
let mut decompressed = Vec::new();
decoder.read_to_end(&mut decompressed)?;
```

**Impact**: 3-10x bandwidth reduction

## Performance Checklist

- [ ] Build with `--release`
- [ ] Enable `target-cpu=native`
- [ ] Profile before optimizing
- [ ] Use appropriate data types
- [ ] Reuse buffers
- [ ] Enable parallelization
- [ ] Use SIMD for arithmetic
- [ ] Buffer I/O operations
- [ ] Implement caching
- [ ] Compress network data
- [ ] Use range requests
- [ ] Connection pooling
- [ ] Benchmark changes

## Performance Targets

| Operation | Target | Excellent |
|-----------|--------|-----------|
| Tile read (256x256) | <10ms | <1ms |
| NDVI (4096x4096) | <100ms | <20ms |
| Reprojection (1024x1024) | <50ms | <10ms |
| HTTP tile fetch | <200ms | <50ms |
| S3 range request | <100ms | <20ms |

## Common Bottlenecks

### 1. Memory Allocations

**Symptom**: High time in `malloc`/`free`

**Solution**: Reuse buffers, use pooling

### 2. Cache Misses

**Symptom**: High L3 cache miss rate

**Solution**: Process data in tiles, improve locality

### 3. Network Latency

**Symptom**: Waiting on I/O

**Solution**: Prefetch, parallel downloads, caching

### 4. Disk I/O

**Symptom**: High iowait

**Solution**: Use SSD, memory mapping, buffering

## Monitoring in Production

```rust
use prometheus::{Counter, Histogram};

lazy_static! {
    static ref TILE_READS: Counter = Counter::new("tile_reads_total", "Total tile reads").unwrap();
    static ref TILE_LATENCY: Histogram = Histogram::new("tile_read_latency_seconds", "Tile read latency").unwrap();
}

fn read_tile() -> Result<RasterBuffer> {
    let timer = TILE_LATENCY.start_timer();

    let tile = do_read_tile()?;

    timer.observe_duration();
    TILE_READS.inc();

    Ok(tile)
}
```

## Resources

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Criterion.rs](https://github.com/bheisler/criterion.rs)
- [Flamegraph](https://github.com/flamegraph-rs/flamegraph)

## Next Steps

1. Profile your application
2. Identify bottlenecks
3. Apply relevant optimizations
4. Measure improvements
5. Repeat

Remember: **Measure, don't guess!**
