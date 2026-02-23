# SIMD Optimization Guide for OxiGDAL Algorithms

## Overview

This guide provides comprehensive information about using SIMD (Single Instruction Multiple Data) optimizations in the OxiGDAL algorithms crate. SIMD operations can provide 2-8x speedups for many spatial analysis operations.

## Table of Contents

1. [What is SIMD?](#what-is-simd)
2. [Performance Benefits](#performance-benefits)
3. [Platform Support](#platform-support)
4. [Usage Patterns](#usage-patterns)
5. [Best Practices](#best-practices)
6. [AVX-512 Support](#avx-512-support)
7. [Migration Guide](#migration-guide)
8. [Troubleshooting](#troubleshooting)

## What is SIMD?

SIMD stands for Single Instruction Multiple Data. It allows a CPU to perform the same operation on multiple data elements simultaneously. For example, instead of adding 8 numbers one at a time, SIMD can add all 8 in a single instruction.

### Vector Widths

Different CPU architectures support different SIMD vector widths:

- **SSE2** (baseline x86-64): 128-bit vectors (4×f32, 2×f64, 16×u8)
- **AVX2**: 256-bit vectors (8×f32, 4×f64, 32×u8)
- **AVX-512**: 512-bit vectors (16×f32, 8×f64, 64×u8)
- **NEON** (ARM): 128-bit vectors (4×f32, 2×f64, 16×u8)

## Performance Benefits

Expected speedup ranges for different operation types:

| Operation Type | Speedup | Notes |
|----------------|---------|-------|
| Focal statistics | 3-5x | Horizontal/vertical passes benefit greatly |
| Texture analysis (GLCM) | 2-3x | Matrix operations and reductions |
| Terrain derivatives | 3-4x | Gradient calculations |
| Hydrology (D8) | 2-3x | Neighbor comparisons |
| Distance calculations | 3-5x | Euclidean distance with sqrt |
| Raster arithmetic | 4-8x | Element-wise operations |
| Statistics | 4-8x | Reductions (sum, min, max) |

## Platform Support

### Automatic Detection

The SIMD module automatically detects available CPU features at compile time:

```rust
use oxigdal_algorithms::simd::platform;

println!("AVX2: {}", platform::HAS_AVX2);
println!("AVX-512: {}", platform::HAS_AVX512);
println!("NEON: {}", platform::HAS_NEON);
println!("F32 lane width: {}", platform::lane_width_f32());
```

### Compile-Time Features

Enable specific SIMD instruction sets:

```toml
[dependencies]
oxigdal-algorithms = { version = "0.1", features = ["simd"] }

# For AVX2-specific optimizations
[dependencies]
oxigdal-algorithms = { version = "0.1", features = ["simd", "avx2"] }

# For AVX-512 support (requires compatible CPU)
[dependencies]
oxigdal-algorithms = { version = "0.1", features = ["simd", "avx512"] }
```

### Runtime Detection

For dynamic dispatch based on CPU capabilities:

```rust
use oxigdal_algorithms::simd::platform;

if platform::HAS_AVX512 {
    // Use AVX-512 optimized path
} else if platform::HAS_AVX2 {
    // Use AVX2 optimized path
} else {
    // Use SSE2/NEON or scalar fallback
}
```

## Usage Patterns

### Pattern 1: Direct SIMD API

Use SIMD-optimized functions directly:

```rust
use oxigdal_algorithms::simd::focal_simd;

let src = vec![1.0_f32; 10000]; // 100x100 raster
let mut dst = vec![0.0_f32; 10000];

// SIMD-optimized focal mean
focal_simd::focal_mean_separable_simd(&src, &mut dst, 100, 100, 3, 3)?;
```

### Pattern 2: Chunked Processing

Process data in SIMD-friendly chunks:

```rust
const LANES: usize = 8; // AVX2 f32 width
let chunks = data.len() / LANES;

// SIMD processing
for i in 0..chunks {
    let start = i * LANES;
    let end = start + LANES;

    // Process chunk with SIMD
    for j in start..end {
        output[j] = data[j] * 2.0; // Auto-vectorized by LLVM
    }
}

// Handle remainder
let remainder_start = chunks * LANES;
for i in remainder_start..data.len() {
    output[i] = data[i] * 2.0;
}
```

### Pattern 3: Aligned Buffers

Use aligned buffers for maximum performance:

```rust
use oxigdal_core::simd_buffer::AlignedBuffer;

// Create 64-byte aligned buffer (AVX-512 optimal)
let mut buffer = AlignedBuffer::<f32>::new(10000, 64)?;

// Verify alignment
assert_eq!(buffer.alignment(), 64);
assert_eq!((buffer.as_ptr() as usize) % 64, 0);
```

### Pattern 4: Separable Filtering

For rectangular windows, use separable filtering:

```rust
use oxigdal_algorithms::simd::focal_simd;

// Separable filter is much faster than generic focal operation
// for large windows (e.g., 15x15)
let result = focal_simd::focal_mean_separable_simd(
    &src, &mut dst, width, height, 15, 15
)?;
```

## Best Practices

### 1. Memory Alignment

Align data to SIMD boundaries for best performance:

```rust
// Good: Aligned allocation
let buffer = AlignedBuffer::<f32>::new(1000, 64)?;

// Avoid: Unaligned slicing
let slice = &data[1..999]; // May be unaligned!
```

### 2. Data Layout

Use contiguous memory layouts (row-major or column-major):

```rust
// Good: Contiguous row-major
let raster = vec![0.0_f32; width * height];
let pixel = raster[y * width + x];

// Avoid: Nested vectors
let bad_raster = vec![vec![0.0_f32; width]; height]; // Cache unfriendly!
```

### 3. Batch Operations

Process multiple items in batches:

```rust
// Good: Process entire row/column
for y in 0..height {
    let row_start = y * width;
    let row_end = row_start + width;
    process_row_simd(&data[row_start..row_end]);
}

// Avoid: Pixel-by-pixel with function calls
for y in 0..height {
    for x in 0..width {
        process_pixel(x, y); // Function call overhead!
    }
}
```

### 4. Minimize Branching

Reduce conditional branches in hot loops:

```rust
// Good: Branchless selection
let val = a + ((b - a) & (mask as i32));

// Avoid: Branches in SIMD loop
if condition {
    result = a;
} else {
    result = b;
}
```

### 5. Use Appropriate Types

Choose types that map well to SIMD:

```rust
// Good: f32 (8 lanes with AVX2)
let data: Vec<f32> = ...;

// Good: u8 for masks (32 lanes with AVX2)
let mask: Vec<u8> = ...;

// Consider: f64 has half the lanes of f32
let data: Vec<f64> = ...; // 4 lanes vs 8 lanes with AVX2
```

## AVX-512 Support

### What is AVX-512?

AVX-512 doubles the vector width from AVX2's 256 bits to 512 bits, providing:
- 16×f32 per instruction (vs 8×f32 with AVX2)
- 8×f64 per instruction (vs 4×f64 with AVX2)
- 64×u8 per instruction (vs 32×u8 with AVX2)

### When to Use AVX-512

AVX-512 provides benefits for:
- ✅ Large data arrays (>10K elements)
- ✅ Simple arithmetic operations
- ✅ Reductions (sum, min, max)
- ✅ Memory bandwidth-bound operations

AVX-512 may not help for:
- ❌ Small datasets (<1K elements)
- ❌ CPU frequency throttling sensitive workloads
- ❌ Cache-bound operations
- ❌ Complex control flow

### CPU Compatibility

AVX-512 availability by CPU generation:
- **Intel**: Skylake-X/SP (2017+), Ice Lake (2019+), Tiger Lake (2020+)
- **AMD**: Zen 4 (2022+)
- **Not available**: Most consumer CPUs before 2020

### Enabling AVX-512

Compile with target feature:

```bash
# Enable AVX-512F (foundation)
RUSTFLAGS="-C target-feature=+avx512f" cargo build --release

# Enable full AVX-512 suite
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

### AVX-512 Code Example

```rust
// The existing SIMD code automatically uses wider vectors
// when compiled with AVX-512 support

const LANES: usize = if cfg!(target_feature = "avx512f") {
    16 // AVX-512
} else if cfg!(target_feature = "avx2") {
    8  // AVX2
} else {
    4  // SSE2/NEON
};

let chunks = data.len() / LANES;
for i in 0..chunks {
    let start = i * LANES;
    let end = start + LANES;

    // LLVM will use 512-bit vectors if AVX-512 is enabled
    for j in start..end {
        output[j] = data[j] * 2.0;
    }
}
```

### Performance Considerations

AVX-512 can cause **frequency throttling** on some CPUs:
- Heavy AVX-512 usage may reduce CPU clock speed
- Light AVX-512 usage typically doesn't throttle
- Monitor with `turbostat` or similar tools

Recommendation:
- Test on target hardware
- Compare AVX-512 vs AVX2 performance
- Consider workload characteristics

## Migration Guide

### From Scalar to SIMD

**Before** (scalar implementation):
```rust
for i in 0..data.len() {
    output[i] = data[i] * 2.0 + 1.0;
}
```

**After** (SIMD-friendly):
```rust
use oxigdal_algorithms::simd::raster;

raster::mul_f32(&data, &two, &mut temp)?;
raster::add_f32(&temp, &one, &mut output)?;
```

### From Generic Focal to Separable

**Before** (slower generic):
```rust
focal_mean(&src, &window, &boundary)?;
```

**After** (faster separable):
```rust
use oxigdal_algorithms::simd::focal_simd;

focal_simd::focal_mean_separable_simd(
    &src, &mut dst, width, height, 15, 15
)?;
```

### From Regular Buffers to Aligned

**Before**:
```rust
let data = vec![0.0_f32; 10000];
```

**After**:
```rust
use oxigdal_core::simd_buffer::AlignedBuffer;

let data = AlignedBuffer::<f32>::zeros(10000, 64)?;
```

## Troubleshooting

### Performance Not Improving

**Problem**: SIMD code not faster than scalar

**Solutions**:
1. Check if SIMD is actually being used:
   ```bash
   cargo rustc --release -- --emit=asm
   # Look for vmovaps, vaddps, etc. in assembly
   ```

2. Verify alignment:
   ```rust
   assert_eq!((ptr as usize) % 64, 0);
   ```

3. Profile with `perf`:
   ```bash
   perf stat -e instructions,cycles cargo bench
   ```

4. Check for small datasets (SIMD overhead matters <1K elements)

### Compilation Errors

**Problem**: Cannot compile with AVX-512

**Solution**: Ensure CPU and compiler support:
```bash
# Check CPU support
cat /proc/cpuinfo | grep avx512

# Use specific target
RUSTFLAGS="-C target-cpu=skylake-avx512" cargo build
```

### Runtime Errors

**Problem**: Illegal instruction error

**Solution**: Binary built for newer CPU than runtime:
- Build with appropriate target-cpu
- Or use runtime feature detection
- Avoid `-C target-cpu=native` for distribution

## Performance Measurement

### Benchmarking

Run SIMD benchmarks:
```bash
cargo bench --bench simd_algorithms
```

### Expected Results

Focal operations (100x100):
- Scalar baseline: ~1.5 ms
- AVX2: ~0.4 ms (3.75x speedup)
- AVX-512: ~0.25 ms (6x speedup)

Texture analysis (GLCM, 32 levels):
- Scalar baseline: ~2.0 ms
- AVX2: ~0.7 ms (2.9x speedup)
- AVX-512: ~0.5 ms (4x speedup)

## References

- [Intel Intrinsics Guide](https://software.intel.com/sites/landingpage/IntrinsicsGuide/)
- [ARM NEON Intrinsics](https://developer.arm.com/architectures/instruction-sets/intrinsics/)
- [LLVM Auto-Vectorization](https://llvm.org/docs/Vectorizers.html)
- [Rust std::simd Documentation](https://doc.rust-lang.org/std/simd/)

## Support

For issues or questions:
- GitHub Issues: https://github.com/cool-japan/oxigdal
- Documentation: https://docs.rs/oxigdal-algorithms

---

**Last Updated**: January 2026
**OxiGDAL Version**: 0.1.0
