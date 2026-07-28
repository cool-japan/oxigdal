# oxigeo-jpeg2000

Pure Rust JPEG2000 (JP2/J2K) driver for OxiGeo.

## Overview

This crate provides a Pure Rust implementation of JPEG2000 image decoding, supporting both JP2 (JPEG2000 Part 1) and raw J2K codestream formats. It is designed as part of the OxiGeo ecosystem for geospatial data processing.

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
use oxigeo_jpeg2000::Jpeg2000Reader;
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

Decoding is real (full EBCOT tier-1 entropy decoding, wavelet inverse transform, and
multi-component transform all run end-to-end -- `decode_rgb`/`decode_rgba` return actual
decoded pixels, not a flat-gray placeholder), and ROI (region of interest) markers are
parsed and honored. What is still missing for full JPEG2000 compliance:

- **No encoding/write support** -- this crate is decode-only; there is no `Jpeg2000Writer`
- Complex quantization modes beyond the common cases
- GeoJP2 / GMLJP2 georeferencing metadata boxes (no `ModelTiepoint`/`GeoKeyDirectory` extraction)
- JPX (JPEG2000 Part 2) extensions
- SIMD-optimized wavelet transforms and parallel tile decoding (see Performance below)

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
| **v0.2.0/v0.2.1** (shipped) | Full tier-1 EBCOT decoder wired into `decode_rgb`/`decode_rgba`; multi-tile decode correctness (per-tile `Psot` bounds + correct pixel offset compositing); `jp2h` box recursion so `ihdr`/`colr` are read from spec-conformant `.jp2` files; progressive quality-layer decoding; ROI decoding |
| **v0.3.0** (planned) | Encoding / write support (`Jpeg2000Writer`, J2K + JP2 container), GeoJP2 metadata box, SIMD-optimized wavelet transforms, parallel tile decoding, memory-mapped large file support |

## License

Apache-2.0

## References

- ISO/IEC 15444-1:2019 - JPEG 2000 image coding system
- ITU-T T.800 - JPEG 2000 image coding system: Core coding system
