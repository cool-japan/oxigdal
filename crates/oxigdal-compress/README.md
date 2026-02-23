# oxigdal-compress

Advanced compression codecs and auto-selection for geospatial data.

## Features

- **Standard Codecs**: LZ4, Zstandard, Brotli, Snappy, DEFLATE
- **Geospatial-Specific Codecs**: Delta encoding, RLE, Dictionary compression
- **Floating-Point Compression**: ZFP and SZ-style with configurable error bounds
- **Auto-Selection Engine**: Intelligent codec selection based on data characteristics
- **Parallel Processing**: Multi-threaded compression/decompression with rayon
- **Benchmarking**: Built-in performance measurement tools

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-compress = "0.1"
```

## Usage

### Basic Compression

```rust
use oxigdal_compress::codecs::{Lz4Codec, ZstdCodec};

// LZ4 compression
let codec = Lz4Codec::new();
let data = b"Hello, world!".repeat(1000);
let compressed = codec.compress(&data)?;
let decompressed = codec.decompress(&compressed, Some(data.len()))?;

// Zstandard compression with custom level
let config = ZstdConfig::with_level(15)?;
let codec = ZstdCodec::with_config(config);
let compressed = codec.compress(&data)?;
```

### Auto-Selection

```rust
use oxigdal_compress::auto_select::*;

let selector = AutoSelector::new(CompressionGoal::Balanced);

let characteristics = DataCharacteristics {
    data_type: DataType::Categorical,
    size: 100000,
    entropy: 0.2,
    unique_count: Some(10),
    value_range: None,
    run_length_ratio: Some(100.0),
};

let recommendations = selector.recommend(&characteristics);
println!("Best codec: {:?}", recommendations[0].codec);
```

### Parallel Compression

```rust
use oxigdal_compress::parallel::ParallelCompressor;

let compressor = ParallelCompressor::new();
let data = vec![42u8; 10_000_000]; // 10 MB

let (compressed, metadata) = compressor.compress_lz4(&data)?;
println!("Compression ratio: {:.2}x", metadata.compression_ratio);

let decompressed = compressor.decompress_lz4(&compressed)?;
```

### Floating-Point Compression

```rust
use oxigdal_compress::floating_point::*;

let config = ZfpConfig::with_mode(ZfpMode::FixedAccuracy(0.001));
let codec = ZfpCodec::with_config(config);

let data: Vec<f64> = (0..10000).map(|i| i as f64 * 0.1).collect();
let compressed = codec.compress_f64(&data)?;
let decompressed = codec.decompress_f64(&compressed, data.len())?;
```

## Codecs

### Standard Codecs

- **LZ4**: Extremely fast compression and decompression
- **Zstandard**: Excellent compression ratios with good speed
- **Brotli**: Best compression ratios, slower speed
- **Snappy**: Very fast, moderate compression
- **DEFLATE**: Widely compatible (gzip/zlib)

### Geospatial Codecs

- **Delta**: Efficient for coordinate data and time series
- **RLE**: Excellent for categorical rasters
- **Dictionary**: Great for data with limited unique values

### Floating-Point Codecs

- **ZFP**: Fixed-rate, precision, or accuracy modes
- **SZ**: Error-bounded lossy compression

## Performance

Typical performance on modern hardware:

| Codec | Speed | Ratio | Use Case |
|-------|-------|-------|----------|
| Snappy | 500 MB/s | 1.8x | Real-time processing |
| LZ4 | 450 MB/s | 2.0x | Fast compression |
| Zstd | 400 MB/s | 3.0x | Balanced performance |
| Deflate | 200 MB/s | 2.5x | Wide compatibility |
| Brotli | 100 MB/s | 3.5x | Maximum compression |
| Delta | 600 MB/s | 2.5x | Coordinate data |
| RLE | 550 MB/s | 8.0x | Categorical rasters |

## License

Apache-2.0

## Author

COOLJAPAN OU (Team Kitasan)
