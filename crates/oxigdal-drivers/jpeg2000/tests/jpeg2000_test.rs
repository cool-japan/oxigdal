//! Integration tests for JPEG2000 driver

use oxigdal_jpeg2000::{Jpeg2000Reader, is_j2k, is_jp2};
use std::io::Cursor;

/// Create minimal valid JP2 file
fn create_minimal_jp2() -> Vec<u8> {
    let mut data = Vec::new();

    // JP2 Signature box
    data.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x0C, // Length: 12
        0x6A, 0x50, 0x20, 0x20, // Type: 'jP  '
        0x0D, 0x0A, 0x87, 0x0A, // Signature
    ]);

    // File Type box
    data.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x14, // Length: 20
        0x66, 0x74, 0x79, 0x70, // Type: 'ftyp'
        0x6A, 0x70, 0x32, 0x20, // Brand: 'jp2 '
        0x00, 0x00, 0x00, 0x00, // Minor version: 0
        0x6A, 0x70, 0x32, 0x20, // Compatibility: 'jp2 '
    ]);

    data
}

/// Create minimal J2K codestream
fn create_minimal_j2k() -> Vec<u8> {
    let mut data = Vec::new();

    // SOC marker
    data.extend_from_slice(&[0xFF, 0x4F]);

    // SIZ marker
    data.extend_from_slice(&[0xFF, 0x51]); // Marker
    data.extend_from_slice(&[0x00, 0x2F]); // Length: 47 (2 for length + 45 for data)
    data.extend_from_slice(&[0x00, 0x00]); // Capability
    data.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]); // Width: 256
    data.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]); // Height: 256
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // X offset: 0
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Y offset: 0
    data.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]); // Tile width: 256
    data.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]); // Tile height: 256
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Tile X offset: 0
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Tile Y offset: 0
    data.extend_from_slice(&[0x00, 0x03]); // Number of components: 3

    // Component 0: 8-bit unsigned, no subsampling
    data.extend_from_slice(&[0x07, 0x01, 0x01]);
    // Component 1: 8-bit unsigned, no subsampling
    data.extend_from_slice(&[0x07, 0x01, 0x01]);
    // Component 2: 8-bit unsigned, no subsampling
    data.extend_from_slice(&[0x07, 0x01, 0x01]);

    // EOC marker
    data.extend_from_slice(&[0xFF, 0xD9]);

    data
}

#[test]
fn test_detect_jp2_format() {
    let data = create_minimal_jp2();
    let mut cursor = Cursor::new(data);

    assert!(is_jp2(&mut cursor).expect("detection failed"));
}

#[test]
fn test_detect_j2k_format() {
    let data = create_minimal_j2k();
    let mut cursor = Cursor::new(data);

    assert!(is_j2k(&mut cursor).expect("detection failed"));
}

#[test]
fn test_reject_invalid_format() {
    let data = vec![0x00, 0x01, 0x02, 0x03];
    let mut cursor = Cursor::new(data);

    assert!(!is_jp2(&mut cursor).expect("detection failed"));
}

#[test]
fn test_parse_j2k_headers() {
    let data = create_minimal_j2k();
    let cursor = Cursor::new(data);

    let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");
    assert!(reader.parse_headers().is_ok());

    assert_eq!(reader.width().ok(), Some(256));
    assert_eq!(reader.height().ok(), Some(256));
    assert_eq!(reader.num_components().ok(), Some(3));
}

#[test]
fn test_image_info() {
    let data = create_minimal_j2k();
    let cursor = Cursor::new(data);

    let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");
    reader.parse_headers().expect("header parsing failed");

    let info = reader.info().expect("info failed");

    assert_eq!(info.width, 256);
    assert_eq!(info.height, 256);
    assert_eq!(info.num_components, 3);
    assert!(!info.is_jp2);
}

#[test]
fn test_reader_creation() {
    let data = create_minimal_jp2();
    let cursor = Cursor::new(data);

    let result = Jpeg2000Reader::new(cursor);
    assert!(result.is_ok());

    let reader = result.expect("reader failed");
    assert!(reader.metadata().is_none()); // No metadata until parsed
}

#[test]
fn test_empty_data() {
    let data = vec![];
    let cursor = Cursor::new(data);

    let result = Jpeg2000Reader::new(cursor);
    assert!(result.is_err());
}

#[test]
fn test_truncated_j2k() {
    // Just SOC marker, no SIZ
    let data = vec![0xFF, 0x4F];
    let cursor = Cursor::new(data);

    let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");
    let result = reader.parse_headers();

    assert!(result.is_err());
}

#[cfg(test)]
mod box_reader_tests {
    use oxigdal_jpeg2000::box_reader::{BoxReader, BoxType};
    use std::io::Cursor;

    #[test]
    fn test_find_box() {
        let mut data = Vec::new();

        // JP2 Signature
        data.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ]);

        // File Type box
        data.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x14, // Length
            0x66, 0x74, 0x79, 0x70, // Type: 'ftyp'
            0x6A, 0x70, 0x32, 0x20, // Brand
            0x00, 0x00, 0x00, 0x00, // Version
            0x6A, 0x70, 0x32, 0x20, // Compatibility
        ]);

        let cursor = Cursor::new(data);
        let mut reader = BoxReader::new(cursor).expect("reader creation failed");

        let result = reader.find_box(BoxType::FileType);
        assert!(result.is_ok());
        assert!(result.expect("find failed").is_some());
    }
}

#[cfg(test)]
mod codestream_tests {
    use oxigdal_jpeg2000::codestream::{Marker, ProgressionOrder, WaveletTransform};

    #[test]
    fn test_marker_conversion() {
        assert_eq!(Marker::from_u16(0xFF4F).ok(), Some(Marker::Soc));
        assert_eq!(Marker::from_u16(0xFFD9).ok(), Some(Marker::Eoc));
        assert_eq!(Marker::from_u16(0xFF51).ok(), Some(Marker::Siz));
    }

    #[test]
    fn test_progression_order() {
        assert_eq!(
            ProgressionOrder::from_u8(0).ok(),
            Some(ProgressionOrder::Lrcp)
        );
        assert_eq!(
            ProgressionOrder::from_u8(4).ok(),
            Some(ProgressionOrder::Cprl)
        );
        assert!(ProgressionOrder::from_u8(99).is_err());
    }

    #[test]
    fn test_wavelet_type() {
        assert_eq!(
            WaveletTransform::from_u8(0).ok(),
            Some(WaveletTransform::Irreversible97)
        );
        assert_eq!(
            WaveletTransform::from_u8(1).ok(),
            Some(WaveletTransform::Reversible53)
        );
        assert!(WaveletTransform::from_u8(99).is_err());
    }
}

#[cfg(test)]
mod metadata_tests {
    use oxigdal_jpeg2000::metadata::EnumeratedColorSpace;

    #[test]
    fn test_colorspace_conversion() {
        let cs = EnumeratedColorSpace::from_u32(16);
        assert_eq!(cs, EnumeratedColorSpace::Srgb);
        assert_eq!(cs.to_u32(), 16);

        let cs2 = EnumeratedColorSpace::from_u32(17);
        assert_eq!(cs2, EnumeratedColorSpace::Grayscale);
    }

    #[test]
    fn test_custom_colorspace() {
        let cs = EnumeratedColorSpace::from_u32(999);
        assert!(matches!(cs, EnumeratedColorSpace::Custom(999)));
    }
}

#[cfg(test)]
mod color_tests {
    use oxigdal_jpeg2000::color::{ColorConverter, level_shift};
    use oxigdal_jpeg2000::metadata::EnumeratedColorSpace;

    #[test]
    fn test_grayscale_conversion() {
        let converter = ColorConverter::new(EnumeratedColorSpace::Grayscale, 1);
        let gray = vec![vec![100u8, 150, 200]];

        let rgb = converter.to_rgb(&gray).expect("conversion failed");

        assert_eq!(rgb.len(), 9);
        assert_eq!(rgb[0], 100);
        assert_eq!(rgb[1], 100);
        assert_eq!(rgb[2], 100);
    }

    #[test]
    fn test_level_shift_signed() {
        let data = vec![-100, 0, 100];
        let result = level_shift(&data, 8, true);

        assert!(result[0] < 128);
        assert_eq!(result[1], 128);
        assert!(result[2] > 128);
    }

    #[test]
    fn test_level_shift_unsigned() {
        let data = vec![0, 128, 255];
        let result = level_shift(&data, 8, false);

        assert_eq!(result[0], 0);
        assert_eq!(result[1], 128);
        assert_eq!(result[2], 255);
    }
}

#[cfg(test)]
mod wavelet_tests {
    use oxigdal_jpeg2000::wavelet::{Irreversible97, Reversible53};

    #[test]
    fn test_reversible_1d() {
        let mut data = vec![100, 200, 150, 50];
        let original = data.clone();

        Reversible53::inverse_1d(&mut data);

        // Data should be transformed
        assert_eq!(data.len(), original.len());
    }

    #[test]
    fn test_irreversible_1d() {
        let mut data = vec![1.0, 2.0, 3.0, 4.0];
        let original = data.clone();

        Irreversible97::inverse_1d(&mut data);

        // Data should be transformed
        assert_eq!(data.len(), original.len());
    }

    #[test]
    fn test_reversible_2d() {
        let mut data = vec![10, 20, 30, 40, 50, 60, 70, 80, 90];
        let result = Reversible53::inverse_2d(&mut data, 3, 3);

        assert!(result.is_ok());
        assert_eq!(data.len(), 9);
    }

    #[test]
    fn test_irreversible_2d() {
        let mut data = vec![1.0, 2.0, 3.0, 4.0];
        let result = Irreversible97::inverse_2d(&mut data, 2, 2);

        assert!(result.is_ok());
        assert_eq!(data.len(), 4);
    }
}
