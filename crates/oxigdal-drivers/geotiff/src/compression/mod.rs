//! Compression and decompression for TIFF data
//!
//! This module provides implementations for various TIFF compression schemes.

use oxigdal_core::error::{CompressionError, OxiGdalError, Result};

use crate::tiff::{Compression, Predictor};

// Re-export JPEG types for public API
#[cfg(feature = "jpeg")]
pub use jpeg_encoder::ColorType;

/// Decompresses data using the specified compression scheme
pub fn decompress(data: &[u8], compression: Compression, expected_size: usize) -> Result<Vec<u8>> {
    match compression {
        Compression::None => Ok(data.to_vec()),

        #[cfg(feature = "deflate")]
        Compression::Deflate | Compression::AdobeDeflate => decompress_deflate(data, expected_size),

        #[cfg(feature = "lzw")]
        Compression::Lzw => decompress_lzw(data, expected_size),

        #[cfg(feature = "zstd")]
        Compression::Zstd => decompress_zstd(data, expected_size),

        Compression::Packbits => decompress_packbits(data, expected_size),

        #[cfg(feature = "jpeg")]
        Compression::Jpeg => decompress_jpeg(data),

        _ => Err(OxiGdalError::Compression(CompressionError::UnknownMethod {
            method: compression as u16,
        })),
    }
}

/// Compresses data using the specified compression scheme
pub fn compress(data: &[u8], compression: Compression) -> Result<Vec<u8>> {
    match compression {
        Compression::None => Ok(data.to_vec()),

        #[cfg(feature = "deflate")]
        Compression::Deflate | Compression::AdobeDeflate => compress_deflate(data),

        #[cfg(feature = "lzw")]
        Compression::Lzw => compress_lzw(data),

        #[cfg(feature = "zstd")]
        Compression::Zstd => compress_zstd(data),

        Compression::Packbits => compress_packbits(data),

        #[cfg(feature = "jpeg")]
        Compression::Jpeg => compress_jpeg(data, 85),

        _ => Err(OxiGdalError::Compression(CompressionError::UnknownMethod {
            method: compression as u16,
        })),
    }
}

/// Applies horizontal differencing predictor (reverse)
pub fn apply_predictor_reverse(
    data: &mut [u8],
    predictor: Predictor,
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    width: usize,
) {
    match predictor {
        Predictor::None => {}
        Predictor::HorizontalDifferencing => {
            let row_bytes = width * samples_per_pixel * bytes_per_sample;
            for row_start in (0..data.len()).step_by(row_bytes) {
                let row_end = (row_start + row_bytes).min(data.len());
                let row = &mut data[row_start..row_end];

                // Reconstruct original values
                let pixel_bytes = samples_per_pixel * bytes_per_sample;
                for i in pixel_bytes..row.len() {
                    row[i] = row[i].wrapping_add(row[i - pixel_bytes]);
                }
            }
        }
        Predictor::FloatingPoint => {
            // Floating-point predictor: more complex, involves byte reordering
            // This is a simplified placeholder
            tracing::warn!("Floating-point predictor not fully implemented");
        }
    }
}

/// Applies horizontal differencing predictor (forward, for compression)
pub fn apply_predictor_forward(
    data: &mut [u8],
    predictor: Predictor,
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    width: usize,
) {
    match predictor {
        Predictor::None => {}
        Predictor::HorizontalDifferencing => {
            let row_bytes = width * samples_per_pixel * bytes_per_sample;
            for row_start in (0..data.len()).step_by(row_bytes) {
                let row_end = (row_start + row_bytes).min(data.len());
                let row = &mut data[row_start..row_end];

                // Store differences (process backwards to avoid overwriting needed values)
                let pixel_bytes = samples_per_pixel * bytes_per_sample;
                for i in (pixel_bytes..row.len()).rev() {
                    row[i] = row[i].wrapping_sub(row[i - pixel_bytes]);
                }
            }
        }
        Predictor::FloatingPoint => {
            tracing::warn!("Floating-point predictor not fully implemented");
        }
    }
}

#[cfg(feature = "deflate")]
fn decompress_deflate(data: &[u8], _expected_size: usize) -> Result<Vec<u8>> {
    oxiarc_deflate::zlib_decompress(data).map_err(|e| {
        OxiGdalError::Compression(CompressionError::DecompressionFailed {
            message: format!("DEFLATE decompression failed: {}", e),
        })
    })
}

#[cfg(feature = "deflate")]
fn compress_deflate(data: &[u8]) -> Result<Vec<u8>> {
    // Default compression level 6
    oxiarc_deflate::zlib_compress(data, 6).map_err(|e| {
        OxiGdalError::Compression(CompressionError::CompressionFailed {
            message: format!("DEFLATE compression failed: {}", e),
        })
    })
}

#[cfg(feature = "lzw")]
fn decompress_lzw(data: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    // Use oxiarc-lzw for TIFF LZW decompression
    // This fixes the truncation bug found in weezl
    oxiarc_lzw::decompress_tiff(data, expected_size).map_err(|e| {
        OxiGdalError::Compression(CompressionError::DecompressionFailed {
            message: format!("LZW decompression failed: {}", e),
        })
    })
}

#[cfg(feature = "lzw")]
fn compress_lzw(data: &[u8]) -> Result<Vec<u8>> {
    // Use oxiarc-lzw for TIFF LZW compression
    oxiarc_lzw::compress_tiff(data).map_err(|e| {
        OxiGdalError::Compression(CompressionError::CompressionFailed {
            message: format!("LZW compression failed: {}", e),
        })
    })
}

#[cfg(feature = "zstd")]
fn decompress_zstd(data: &[u8], _expected_size: usize) -> Result<Vec<u8>> {
    oxiarc_zstd::decode_all(data).map_err(|e| {
        OxiGdalError::Compression(CompressionError::DecompressionFailed {
            message: format!("ZSTD decompression failed: {}", e),
        })
    })
}

#[cfg(feature = "zstd")]
fn compress_zstd(data: &[u8]) -> Result<Vec<u8>> {
    oxiarc_zstd::encode_all(data, 3).map_err(|e| {
        OxiGdalError::Compression(CompressionError::CompressionFailed {
            message: format!("ZSTD compression failed: {}", e),
        })
    })
}

#[cfg(feature = "jpeg")]
fn decompress_jpeg(data: &[u8]) -> Result<Vec<u8>> {
    use jpeg_decoder::Decoder;

    let mut decoder = Decoder::new(data);
    let pixels = decoder.decode().map_err(|e| {
        OxiGdalError::Compression(CompressionError::DecompressionFailed {
            message: format!("JPEG decompression failed: {}", e),
        })
    })?;

    // Get image metadata
    let info = decoder.info().ok_or_else(|| {
        OxiGdalError::Compression(CompressionError::DecompressionFailed {
            message: "JPEG decoder missing image info".to_string(),
        })
    })?;

    // Handle YCbCr to RGB conversion if needed
    // jpeg-decoder already handles this conversion internally
    // The output is in RGB format for color images

    // For grayscale or RGB, pixels are already in the correct format
    // TIFF expects RGB for color images
    match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => {
            // Grayscale, no conversion needed
            Ok(pixels)
        }
        jpeg_decoder::PixelFormat::RGB24 => {
            // RGB, no conversion needed
            Ok(pixels)
        }
        jpeg_decoder::PixelFormat::CMYK32 => {
            // CMYK needs conversion to RGB
            cmyk_to_rgb(&pixels)
        }
        _ => Err(OxiGdalError::Compression(
            CompressionError::DecompressionFailed {
                message: format!("Unsupported JPEG pixel format: {:?}", info.pixel_format),
            },
        )),
    }
}

#[cfg(feature = "jpeg")]
fn compress_jpeg(_data: &[u8], _quality: u8) -> Result<Vec<u8>> {
    // Determine image properties from data
    // For now, assume RGB 8-bit data
    // In a real implementation, this would need additional parameters
    // or context to determine the correct dimensions and color type

    // Note: This is a simplified version. In practice, we'd need width, height,
    // and color type information passed separately or derived from context.

    // For demonstration, we'll create a simple encoder
    // Real usage would require image dimensions to be passed as parameters

    // Since we don't have dimensions here, we'll need to refactor this
    // to accept width, height, and color_type as parameters

    // For now, return an error indicating this needs more information
    Err(OxiGdalError::Compression(CompressionError::CompressionFailed {
        message: "JPEG compression requires image dimensions and color type information. Use compress_jpeg_with_params instead.".to_string(),
    }))
}

/// JPEG compression with explicit parameters
#[cfg(feature = "jpeg")]
pub fn compress_jpeg_with_params(
    data: &[u8],
    width: u16,
    height: u16,
    color_type: jpeg_encoder::ColorType,
    quality: u8,
) -> Result<Vec<u8>> {
    use jpeg_encoder::Encoder;

    let mut output = Vec::new();
    let encoder = Encoder::new(&mut output, quality);

    encoder
        .encode(data, width, height, color_type)
        .map_err(|e| {
            OxiGdalError::Compression(CompressionError::CompressionFailed {
                message: format!("JPEG compression failed: {}", e),
            })
        })?;

    Ok(output)
}

/// Converts CMYK to RGB
#[cfg(feature = "jpeg")]
fn cmyk_to_rgb(cmyk_data: &[u8]) -> Result<Vec<u8>> {
    if cmyk_data.len() % 4 != 0 {
        return Err(OxiGdalError::Compression(CompressionError::InvalidData {
            message: "CMYK data length must be multiple of 4".to_string(),
        }));
    }

    let pixel_count = cmyk_data.len() / 4;
    let mut rgb_data = Vec::with_capacity(pixel_count * 3);

    for i in 0..pixel_count {
        let c = cmyk_data[i * 4] as f32 / 255.0;
        let m = cmyk_data[i * 4 + 1] as f32 / 255.0;
        let y = cmyk_data[i * 4 + 2] as f32 / 255.0;
        let k = cmyk_data[i * 4 + 3] as f32 / 255.0;

        // CMYK to RGB conversion
        let r = ((1.0 - c) * (1.0 - k) * 255.0) as u8;
        let g = ((1.0 - m) * (1.0 - k) * 255.0) as u8;
        let b = ((1.0 - y) * (1.0 - k) * 255.0) as u8;

        rgb_data.push(r);
        rgb_data.push(g);
        rgb_data.push(b);
    }

    Ok(rgb_data)
}

/// PackBits decompression (simple RLE)
fn decompress_packbits(data: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(expected_size);
    let mut i = 0;

    while i < data.len() && output.len() < expected_size {
        let n = data[i] as i8;
        i += 1;

        if n >= 0 {
            // Literal run: copy next n+1 bytes
            let count = (n as usize) + 1;
            if i + count > data.len() {
                return Err(OxiGdalError::Compression(CompressionError::InvalidData {
                    message: "PackBits: unexpected end of data".to_string(),
                }));
            }
            output.extend_from_slice(&data[i..i + count]);
            i += count;
        } else if n > -128 {
            // Repeat run: repeat next byte -n+1 times
            if i >= data.len() {
                return Err(OxiGdalError::Compression(CompressionError::InvalidData {
                    message: "PackBits: unexpected end of data".to_string(),
                }));
            }
            let count = ((-n) as usize) + 1;
            let byte = data[i];
            i += 1;
            output.extend(std::iter::repeat_n(byte, count));
        }
        // n == -128: no-op
    }

    Ok(output)
}

/// PackBits compression (simple RLE)
fn compress_packbits(data: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut i = 0;

    while i < data.len() {
        // Look for runs
        let mut run_length = 1;
        while i + run_length < data.len() && run_length < 128 && data[i + run_length] == data[i] {
            run_length += 1;
        }

        if run_length > 1 {
            // Write repeat run
            // For PackBits: repeat run of N bytes is encoded as (1 - N)
            // run_length ranges from 2 to 128, so (1 - run_length) ranges from -1 to -127
            output.push(((1_i16 - run_length as i16) as i8) as u8);
            output.push(data[i]);
            i += run_length;
        } else {
            // Look for literal run
            let literal_start = i;
            let mut literal_end = i + 1;

            while literal_end < data.len() && literal_end - literal_start < 128 {
                // Check if a run starts here
                if literal_end + 1 < data.len() && data[literal_end] == data[literal_end + 1] {
                    break;
                }
                literal_end += 1;
            }

            let count = literal_end - literal_start;
            output.push((count - 1) as u8);
            output.extend_from_slice(&data[literal_start..literal_end]);
            i = literal_end;
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn test_packbits_roundtrip() {
        let original = b"AAAAAABBBBCCCCCCCCCCDDDEEEEEEEEEE";
        let compressed = compress_packbits(original).expect("compression should work");
        let decompressed =
            decompress_packbits(&compressed, original.len()).expect("decompression should work");
        assert_eq!(&decompressed, original);
    }

    #[test]
    fn test_predictor() {
        let original = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let mut data = original.clone();

        // Apply forward then reverse should give original
        apply_predictor_forward(&mut data, Predictor::HorizontalDifferencing, 1, 1, 8);
        apply_predictor_reverse(&mut data, Predictor::HorizontalDifferencing, 1, 1, 8);

        assert_eq!(data, original);
    }

    #[cfg(feature = "deflate")]
    #[test]
    fn test_deflate_roundtrip() {
        let original = b"Hello, World! This is a test of DEFLATE compression.";
        let compressed = compress_deflate(original).expect("compression should work");
        let decompressed =
            decompress_deflate(&compressed, original.len()).expect("decompression should work");
        assert_eq!(&decompressed, original);
    }

    #[cfg(feature = "zstd")]
    #[test]
    fn test_zstd_roundtrip() {
        let original = b"Hello, World! This is a test of ZSTD compression.";
        let compressed = compress_zstd(original).expect("compression should work");
        let decompressed =
            decompress_zstd(&compressed, original.len()).expect("decompression should work");
        assert_eq!(&decompressed, original);
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_jpeg_grayscale_roundtrip() {
        use jpeg_encoder::ColorType;

        // Create a simple 8x8 grayscale test image
        let width = 8;
        let height = 8;
        let mut original = Vec::new();
        for y in 0..height {
            for x in 0..width {
                // Create a gradient pattern
                original.push(((x + y * width) * 4) as u8);
            }
        }

        // Compress
        let compressed =
            compress_jpeg_with_params(&original, width as u16, height as u16, ColorType::Luma, 85)
                .expect("compression should work");

        // Decompress
        let decompressed = decompress_jpeg(&compressed).expect("decompression should work");

        // JPEG is lossy, so we check that dimensions match and values are close
        assert_eq!(decompressed.len(), original.len());

        // Check that most pixels are within a reasonable threshold (JPEG is lossy)
        let mut close_count = 0;
        for (i, (&orig, &decomp)) in original.iter().zip(decompressed.iter()).enumerate() {
            let diff = (orig as i16 - decomp as i16).abs();
            if diff <= 20 {
                // Allow up to 20 levels difference for JPEG artifacts
                close_count += 1;
            } else {
                tracing::debug!("Pixel {} differs by {}: {} vs {}", i, diff, orig, decomp);
            }
        }

        // At least 90% of pixels should be close
        let close_ratio = close_count as f64 / original.len() as f64;
        assert!(
            close_ratio >= 0.9,
            "Only {:.1}% of pixels are close (expected >= 90%)",
            close_ratio * 100.0
        );
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_jpeg_rgb_roundtrip() {
        use jpeg_encoder::ColorType;

        // Create a simple 8x8 RGB test image
        let width = 8;
        let height = 8;
        let mut original = Vec::new();
        for y in 0..height {
            for x in 0..width {
                // Create a colorful gradient pattern
                original.push((x * 32) as u8); // R
                original.push((y * 32) as u8); // G
                original.push(((x + y) * 16) as u8); // B
            }
        }

        // Compress
        let compressed =
            compress_jpeg_with_params(&original, width as u16, height as u16, ColorType::Rgb, 85)
                .expect("compression should work");

        // Decompress
        let decompressed = decompress_jpeg(&compressed).expect("decompression should work");

        // Check dimensions
        assert_eq!(decompressed.len(), original.len());

        // Check that most pixels are within a reasonable threshold
        let mut close_count = 0;
        for (i, (&orig, &decomp)) in original.iter().zip(decompressed.iter()).enumerate() {
            let diff = (orig as i16 - decomp as i16).abs();
            if diff <= 25 {
                // Allow up to 25 levels difference for JPEG artifacts
                close_count += 1;
            } else {
                tracing::debug!(
                    "Pixel component {} differs by {}: {} vs {}",
                    i,
                    diff,
                    orig,
                    decomp
                );
            }
        }

        // At least 85% of pixel components should be close
        let close_ratio = close_count as f64 / original.len() as f64;
        assert!(
            close_ratio >= 0.85,
            "Only {:.1}% of pixel components are close (expected >= 85%)",
            close_ratio * 100.0
        );
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_jpeg_quality_settings() {
        use jpeg_encoder::ColorType;

        // Create a simple 16x16 grayscale test image
        let width = 16;
        let height = 16;
        let mut original = Vec::new();
        for y in 0..height {
            for x in 0..width {
                original.push(((x + y * width) * 2) as u8);
            }
        }

        // Test different quality levels
        let quality_low =
            compress_jpeg_with_params(&original, width as u16, height as u16, ColorType::Luma, 50)
                .expect("low quality compression should work");

        let quality_high =
            compress_jpeg_with_params(&original, width as u16, height as u16, ColorType::Luma, 95)
                .expect("high quality compression should work");

        // Higher quality should produce larger files
        assert!(
            quality_high.len() >= quality_low.len(),
            "High quality ({} bytes) should be >= low quality ({} bytes)",
            quality_high.len(),
            quality_low.len()
        );

        // Both should decompress successfully
        let _decompressed_low = decompress_jpeg(&quality_low).expect("should decompress");
        let _decompressed_high = decompress_jpeg(&quality_high).expect("should decompress");
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_cmyk_to_rgb_conversion() {
        // Test CMYK to RGB conversion
        // Pure black: C=0, M=0, Y=0, K=100
        let cmyk = vec![0, 0, 0, 255];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![0, 0, 0]);

        // Pure white: C=0, M=0, Y=0, K=0
        let cmyk = vec![0, 0, 0, 0];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![255, 255, 255]);

        // Pure cyan: C=100, M=0, Y=0, K=0
        let cmyk = vec![255, 0, 0, 0];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![0, 255, 255]);

        // Pure magenta: C=0, M=100, Y=0, K=0
        let cmyk = vec![0, 255, 0, 0];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![255, 0, 255]);

        // Pure yellow: C=0, M=0, Y=100, K=0
        let cmyk = vec![0, 0, 255, 0];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![255, 255, 0]);
    }
}
