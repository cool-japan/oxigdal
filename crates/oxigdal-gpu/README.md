# oxigdal-gpu

[![Crates.io](https://img.shields.io/crates/v/oxigdal-gpu.svg)](https://crates.io/crates/oxigdal-gpu)
[![Documentation](https://docs.rs/oxigdal-gpu/badge.svg)](https://docs.rs/oxigdal-gpu)
[![License](https://img.shields.io/crates/l/oxigdal-gpu.svg)](LICENSE)

GPU-accelerated geospatial operations for OxiGDAL using [wgpu](https://wgpu.rs/) — cross-platform, pure Rust GPU compute. Part of the [OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem.

## Features

- **Cross-platform**: Vulkan, Metal, DX12, WebGPU — one API for all platforms
- **Element-wise operations**: Add, subtract, multiply, divide, power, clamp
- **Statistical operations**: Parallel reduction, histogram (256/1024 bins), min/max, mean, std-dev
- **Resampling**: Nearest neighbor, bilinear, bicubic interpolation on GPU
- **Convolution**: Gaussian blur, edge detection (Sobel), Laplacian, custom kernel
- **Raster algebra**: Pixel-wise band math expressions on GPU
- **Pipeline API**: Chain operations without CPU round-trips (zero intermediate copies)
- **Async execution**: Non-blocking GPU dispatch, works with Tokio
- **Pure Rust**: No CUDA/OpenCL/C bindings — wgpu only

## Installation

```toml
[dependencies]
oxigdal-gpu = "0.1.2"
```

Enable WebGPU on WASM targets:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
oxigdal-gpu = { version = "0.1.2", features = ["webgpu"] }
```

## Quick Start

```rust
use oxigdal_gpu::{GpuContext, ComputePipeline};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize GPU context (auto-selects best backend)
    let gpu = GpuContext::new().await?;

    // Load raster data
    let data: Vec<f32> = load_raster("elevation.tif")?;

    // Build and run GPU pipeline
    let result = ComputePipeline::from_data(&gpu, &data, 1024, 1024)?
        .gaussian_blur(2.0)?    // sigma=2.0
        .multiply(1.5)?
        .clamp(0.0, 255.0)?
        .read_blocking()?;

    println!("Processed {} pixels on GPU", result.len());
    Ok(())
}
```

## Operations

### Element-wise

```rust
let pipeline = ComputePipeline::from_data(&gpu, &band, width, height)?
    .add_scalar(10.0)?
    .multiply(0.001)?
    .power(2.0)?
    .clamp(0.0, 1.0)?;
```

### Multi-band Raster Algebra (NDVI)

```rust
use oxigdal_gpu::algebra::BandAlgebra;

let ndvi = BandAlgebra::new(&gpu)
    .with_band("nir", &nir_band, width, height)?
    .with_band("red", &red_band, width, height)?
    .expression("(nir - red) / (nir + red)")?
    .execute()?;
```

### Convolution

```rust
let blurred  = pipeline.gaussian_blur(3.0)?;     // Gaussian sigma=3.0
let edges    = pipeline.sobel_edge()?;             // Sobel edge detection
let laplacian = pipeline.laplacian()?;             // Laplacian sharpening
let custom   = pipeline.convolve(&kernel_3x3)?;   // 3x3 custom kernel
```

### Statistics (parallel GPU reduction)

```rust
use oxigdal_gpu::stats::GpuStats;

let stats = GpuStats::compute(&gpu, &data, width, height).await?;
println!("min={}, max={}, mean={:.3}, std={:.3}", stats.min, stats.max, stats.mean, stats.std_dev);

let hist = GpuStats::histogram(&gpu, &data, 256).await?;
```

### Resampling

```rust
use oxigdal_gpu::resample::{GpuResampler, ResamplingMethod};

let resampler = GpuResampler::new(&gpu, ResamplingMethod::Bilinear)?;
let downsampled = resampler.resample(&data, (4096, 4096), (1024, 1024)).await?;
```

## Performance

Benchmarks on NVIDIA RTX 4080 vs CPU (Apple M2, single thread):

| Operation (4096x4096 f32) | CPU | GPU | Speedup |
|---------------------------|-----|-----|---------|
| Element-wise multiply | 120 ms | 1.2 ms | 100x |
| Gaussian blur (sigma=3) | 580 ms | 5.5 ms | 105x |
| Bilinear resampling | 230 ms | 9 ms | 25x |
| Histogram (256 bins) | 180 ms | 2 ms | 90x |
| Pipeline (3 ops, no copies) | 900 ms | 9 ms | 100x |

Significant speedup for large rasters (>= 2048x2048). For small rasters, CPU overhead dominates.

## Requirements

- Rust 1.85+
- GPU with Vulkan 1.0+ (Linux/Windows), Metal (macOS), or DX12 (Windows)
- WebGPU (optional WASM target)

## Advanced: `oxigdal-gpu-advanced`

For complex compute shaders, custom WGSL kernels, and multi-GPU workflows,
see [`oxigdal-gpu-advanced`](../oxigdal-gpu-advanced).

## COOLJAPAN Policies

- Pure Rust — wgpu only, no CUDA/OpenCL/C bindings in default features
- No `unwrap()` — all GPU errors handled via `Result<T, GpuError>`
- Workspace dependencies via `*.workspace = true`

## License

Apache-2.0 — Copyright (c) COOLJAPAN OU (Team Kitasan)
