# oxigdal-jpeg2000

Pure Rust JPEG2000 (JP2/J2K) driver for OxiGDAL.

## Overview

This crate provides a Pure Rust implementation of JPEG2000 image decoding, supporting both JP2 (JPEG2000 Part 1) and raw J2K codestream formats. It is designed as part of the OxiGDAL ecosystem for geospatial data processing.

## Features

- **Pure Rust** - No C/C++ dependencies (OpenJPEG-free, Kakadu-free)
- **JP2 Format** - Full JP2 box structure parsing
- **J2K Codestream** - Raw codestream support
- **Wavelet Transforms** - Both 5/3 reversible (lossless) and 9/7 irreversible (lossy)
- **Multi-component** - RGB, RGBA, and grayscale images
- **Tiling** - Support for tiled images
- **Metadata** - Complete JP2 metadata extraction
- **Color Spaces** - sRGB, grayscale, sYCC conversions

## Architecture

The decoder is organized into several layers:

- **Box Reader** - JP2 box structure parsing
- **Codestream** - JPEG2000 marker and segment parsing
- **Tier-2** - Packet decoding and quality layers
- **Tier-1** - Code-block decoding (EBCOT)
- **Wavelet** - Inverse wavelet transforms
- **Color** - Color space conversions
- **Metadata** - JP2 metadata boxes
- **Reader** - High-level decoding interface

## Usage

```rust
use oxigdal_jpeg2000::Jpeg2000Reader;
use std::fs::File;
use std::io::BufReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("image.jp2")?;
    let reader = BufReader::new(file);

    let mut decoder = Jpeg2000Reader::new(reader)?;
    decoder.parse_headers()?;

    let info = decoder.info()?;
    println!("Image: {}x{}", info.width, info.height);
    println!("Components: {}", info.num_components);
    println!("Decomposition levels: {}", info.num_decomposition_levels);

    // Get metadata
    if let Some(metadata) = decoder.metadata() {
        if let Some(color_spec) = &metadata.color_spec {
            println!("Color space: {:?}", color_spec.enum_cs);
        }
    }

    Ok(())
}
```

## Limitations

This is a reference implementation with simplified decoding for common cases. Full JPEG2000 compliance requires extensive additional work, particularly for:

- Complete tier-1 EBCOT decoder
- All progression orders
- Region of interest (ROI)
- Complex quantization modes
- JPX (JPEG2000 Part 2) extensions

For production use with complex JPEG2000 files, consider this a starting point that may need enhancement.

## JPEG2000 Standard

JPEG2000 is defined in ISO/IEC 15444-1:2019. This implementation follows the standard for basic decoding functionality.

## Performance

The implementation prioritizes correctness and code clarity over performance:

- Wavelet transforms are not SIMD-optimized
- Memory usage is not optimized for very large images
- Parallel tile decoding is not implemented

For high-performance applications, additional optimization work is recommended.

## Roadmap

| Release | Feature |
|---------|---------|
| **v0.2.0** (Q2 2026) | Complete tier-1 EBCOT decoder, SIMD-optimized wavelet transforms, parallel tile decoding, memory-mapped large file support |
| **v0.3.0** (Q3 2026) | Encoding / write support, progressive quality decoding, region-of-interest decoding, JPX (Part 2) features |

## License

Apache-2.0

## References

- ISO/IEC 15444-1:2019 - JPEG 2000 image coding system
- ITU-T T.800 - JPEG 2000 image coding system: Core coding system
