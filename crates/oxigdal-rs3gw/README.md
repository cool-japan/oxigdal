# oxigdal-rs3gw

High-performance cloud storage backend for OxiGDAL powered by [rs3gw](https://github.com/cool-japan/rs3gw).

## Features

- **Multi-Backend Support**: Local filesystem, S3, MinIO, Google Cloud Storage, Azure Blob Storage
- **High Performance**:
  - Zero-copy operations for large data transfers
  - ML-based predictive caching for COG tile access
  - Content-based deduplication (30-70% storage savings)
- **Cloud-Optimized**:
  - Optimized for COG (Cloud-Optimized GeoTIFF) tile access patterns
  - Efficient Zarr array chunk storage
- **Security**:
  - Optional AES-256-GCM encryption at rest
  - PBKDF2 key derivation support
- **Pure Rust**: No C/C++ dependencies (COOLJAPAN Policy compliant)

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-rs3gw = "0.1"

# Enable specific features
oxigdal-rs3gw = { version = "0.1", features = ["async", "ml-cache", "dedup"] }
```

### Feature Flags

- `local` (default) - Local filesystem backend
- `s3` - AWS S3 backend support
- `minio` - MinIO backend support (includes `s3`)
- `gcs` - Google Cloud Storage backend
- `azure` - Azure Blob Storage backend
- `all-backends` - Enable all storage backends
- `async` - Async I/O support
- `ml-cache` - ML-based predictive caching
- `dedup` - Content-based deduplication
- `encryption` - Encryption at rest
- `zarr` - Zarr store integration (requires `oxigdal-zarr`)

## Quick Start

### Reading a COG from S3

```rust
use oxigdal_rs3gw::{OxigdalBackend, Rs3gwDataSource};
use oxigdal_core::io::DataSource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure S3 backend
    let backend = OxigdalBackend::S3 {
        region: "us-west-2".to_string(),
        bucket: "my-cog-bucket".to_string(),
        endpoint: None,
        access_key: None, // Uses AWS SDK default credentials
        secret_key: None,
    };

    // Create storage backend
    let storage = backend.create_storage().await?;

    // Open data source
    let source = Rs3gwDataSource::new(
        storage,
        "my-cog-bucket".to_string(),
        "images/landsat.cog.tif".to_string()
    ).await?;

    // Read tile data
    let tile_range = ByteRange::new(0, 65536); // First 64KB
    let data = source.read_range(tile_range)?;

    println!("Read {} bytes", data.len());
    Ok(())
}
```

### Using MinIO

```rust
use oxigdal_rs3gw::MinioBackendBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = MinioBackendBuilder::new(
        "http://localhost:9000",
        "geospatial-data",
        "minioadmin",
        "minioadmin"
    )
    .region("us-east-1")
    .build();

    let storage = backend.create_storage().await?;

    // Use with DataSource or Zarr Store...
    Ok(())
}
```

### Zarr Array Storage

```rust
use oxigdal_rs3gw::{OxigdalBackend, Rs3gwStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = OxigdalBackend::Local {
        root: "/data/zarr".into(),
    };

    let storage = backend.create_storage().await?;

    let store = Rs3gwStore::new(
        storage,
        "local".to_string(),
        "temperature.zarr".to_string()
    );

    // Use with oxigdal-zarr array operations
    // let array = ZarrArray::open(store).await?;

    Ok(())
}
```

## Advanced Features

### Concurrent Read Optimizations

Dramatically improve read performance with configurable concurrent tile fetching:

```rust
use oxigdal_rs3gw::datasource::{Rs3gwDataSource, ConcurrentReadConfig};
use oxigdal_core::io::{DataSource, ByteRange};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let storage = /* ... create storage backend ... */;

    // Configure concurrent reads
    let config = ConcurrentReadConfig::new()
        .with_concurrency_limit(8)      // 8 concurrent requests
        .with_max_retries(3)             // Retry failed reads
        .with_backoff(100, 2.0)          // 100ms base, 2x multiplier
        .with_cache(true)                // Enable LRU cache
        .with_cache_config(5000, 3600)   // 5000 tiles, 1 hour TTL
        .with_prefetch_radius(2);        // Prefetch nearby tiles

    let source = Rs3gwDataSource::new_with_config(
        storage,
        "bucket".to_string(),
        "large-cog.tif".to_string(),
        config,
    ).await?;

    // Read multiple tiles concurrently
    let ranges = vec![
        ByteRange::new(0, 256 * 1024),
        ByteRange::new(256 * 1024, 512 * 1024),
        ByteRange::new(512 * 1024, 768 * 1024),
        ByteRange::new(768 * 1024, 1024 * 1024),
    ];

    // All tiles are fetched concurrently!
    let tiles = source.read_ranges(&ranges)?;

    println!("Read {} tiles", tiles.len());
    Ok(())
}
```

**Performance Benefits:**
- **2-4x faster** for batch reads with concurrency=4
- **Automatic retry** with exponential backoff for transient failures
- **LRU caching** reduces repeated S3 requests
- **Spatial prefetching** pre-loads nearby tiles before they're requested

**Configuration Guidelines:**
- **Local storage**: concurrency=2-4 (limited by disk I/O)
- **S3/Cloud**: concurrency=8-16 (network bandwidth limited)
- **Large files**: Enable cache with high TTL
- **Sequential access**: Use prefetch_radius=3-4

### COG-Optimized Caching

Configure cache based on your access pattern:

```rust
use oxigdal_rs3gw::features::{CogCacheConfig, CogAccessPattern};

// Use preset for your access pattern
let cache = CogAccessPattern::Sequential.recommended_config();

// Or customize
let cache = CogCacheConfig::new()
    .with_max_size_mb(512)
    .with_max_tiles(10_000)
    .with_tile_ttl(3600)
    .with_ml_prefetch(true)
    .with_prefetch_radius(2);

println!("Cache: {} MB, {} tiles", cache.max_size_mb, cache.max_tiles);
```

Available access patterns:
- `Sequential` - Linear tile access (rendering)
- `Random` - Random tile access (panning/zooming)
- `Regional` - Viewing specific areas
- `Pyramid` - Multi-scale overview access

### Zarr Deduplication

Reduce storage costs for Zarr arrays with redundant chunks:

```rust
use oxigdal_rs3gw::features::{ZarrDedupConfig, ZarrDedupPresets};

// Use preset for your chunk size
let dedup = ZarrDedupPresets::medium_chunks(); // 256 KB chunks

// Or customize for your exact chunk size
let dedup = ZarrDedupConfig::for_chunk_size(512 * 1024)
    .with_aggressive_nodata(true);

// Estimate savings
let savings = oxigdal_rs3gw::features::dedup::estimate_savings(
    10_000,  // total chunks
    3_000    // unique chunks (from sampling)
);
println!("Estimated storage savings: {:.1}%", savings * 100.0);
```

### Encryption

Protect sensitive geospatial data:

```rust
use oxigdal_rs3gw::features::{EncryptionConfig, generate_key};

// Generate a secure key
let key = generate_key();

// Or derive from password
let key = oxigdal_rs3gw::features::encryption::derive_key_from_password(
    "my_secure_password",
    b"unique_salt_per_dataset",
    100_000  // PBKDF2 iterations
);

// Configure encryption
let encryption = EncryptionConfig::new()
    .with_key(key)
    .with_metadata_encryption(true);

encryption.validate()?;
```

**Security Note**: In production, use a key management system (e.g., AWS KMS, HashiCorp Vault) instead of generating keys in code.

## Performance Characteristics

### COG Tile Access

With ML caching enabled:
- **First access**: ~100-200ms (network latency)
- **Cached access**: <1ms (memory)
- **Predicted prefetch**: Ready before request

### Zarr Chunk Access

With deduplication:
- **Storage savings**: 30-70% (typical for nodata-heavy datasets)
- **Read performance**: No overhead (dedup transparent)
- **Write performance**: ~10-15% overhead (hash computation)

### Supported Backends

| Backend | Read | Write | Range Requests | Multipart Upload |
|---------|------|-------|----------------|------------------|
| Local   | ✓    | ✓     | ✓              | ✓                |
| S3      | ✓    | ✓     | ✓              | ✓                |
| MinIO   | ✓    | ✓     | ✓              | ✓                |
| GCS     | ✓    | ✓     | ✓              | ✓                |
| Azure   | ✓    | ✓     | ✓              | ✓                |

## Architecture

```
┌─────────────────────────────────────┐
│     OxiGDAL Drivers                 │
│  (GeoTIFF, Zarr, NetCDF, etc.)      │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│   oxigdal-rs3gw (This Crate)        │
│  ┌──────────┐      ┌──────────┐    │
│  │DataSource│      │ZarrStore │    │
│  └──────────┘      └──────────┘    │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│           rs3gw                     │
│  ┌─────────────────────────────┐   │
│  │   StorageBackend Trait      │   │
│  └─────────────────────────────┘   │
│     │     │      │      │      │    │
│  Local  S3  MinIO  GCS  Azure  │    │
└─────────────────────────────────────┘
```

## Examples

See the [examples](../../examples) directory for complete examples:

- `cog_s3.rs` - Reading COG from S3
- `zarr_minio.rs` - Zarr array on MinIO
- `encryption.rs` - Encrypted storage
- `dedup_benchmark.rs` - Deduplication benchmarks

## Environment Variables

Configure backends using environment variables:

```bash
# S3 Configuration
export AWS_REGION=us-west-2
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...

# MinIO Configuration
export MINIO_ENDPOINT=http://localhost:9000
export MINIO_ACCESS_KEY=minioadmin
export MINIO_SECRET_KEY=minioadmin

# GCS Configuration
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/credentials.json

# Azure Configuration
export AZURE_STORAGE_ACCOUNT=myaccount
export AZURE_STORAGE_KEY=...
```

## Benchmarks

Run benchmarks to measure concurrent read performance:

```bash
# Run all benchmarks
cargo bench -p oxigdal-rs3gw

# Run specific benchmark suite
cargo bench -p oxigdal-rs3gw --bench datasource_benchmarks
```

**Benchmark Suites:**
- `sequential_reads` - Baseline single-threaded performance
- `concurrent_reads` - Measure speedup with different concurrency levels
- `cache_performance` - Compare cache hit vs miss latency
- `tile_sizes` - Performance across different tile sizes (64KB - 4MB)
- `batch_sizes` - Scalability with different batch sizes (1-100 tiles)

**Typical Results** (Local SSD):
- Sequential: ~5-10 MB/s
- Concurrent (4 threads): ~15-25 MB/s
- Concurrent (8 threads): ~20-35 MB/s
- Cache hit: <0.1ms latency

## License

Apache-2.0

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

## Links

- [OxiGDAL](https://github.com/cool-japan/oxigdal) - Pure Rust geospatial library
- [rs3gw](https://github.com/cool-japan/rs3gw) - S3-compatible storage gateway
- [COOLJAPAN Ecosystem](https://github.com/cool-japan)
