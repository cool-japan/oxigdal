//! TIFF/BigTIFF file format parsing
//!
//! This module provides low-level TIFF parsing functionality:
//!
//! - [`TiffHeader`] - TIFF/BigTIFF header parsing
//! - [`Ifd`] - Image File Directory parsing
//! - [`IfdEntry`] - Individual tag entries
//! - [`TiffTag`] - Tag definitions
//! - [`Compression`] - Compression schemes

mod header;
mod ifd;
mod tags;

pub use header::{ByteOrderType, TIFF_MAGIC_BE, TIFF_MAGIC_LE, TiffHeader, TiffVariant};
pub use ifd::{FieldType, Ifd, IfdEntry};
pub use tags::{
    Compression, PhotometricInterpretation, PlanarConfiguration, Predictor, SampleFormat, TiffTag,
};

use oxigeo_core::error::{FormatError, OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_core::types::RasterDataType;

/// A parsed TIFF file
#[derive(Debug, Clone)]
pub struct TiffFile {
    /// File header
    pub header: TiffHeader,
    /// Image File Directories
    pub ifds: Vec<Ifd>,
}

impl TiffFile {
    /// Reads the leading header bytes, however few of them the source has.
    ///
    /// The parser wants [`TiffHeader::BIGTIFF_HEADER_SIZE`] bytes, because that
    /// is the largest header it may have to look at, but a *classic* TIFF header
    /// is only [`TiffHeader::MIN_HEADER_SIZE`] bytes and
    /// [`TiffHeader::parse`] handles every length from there up, with a specific
    /// error for each way it can fall short.
    ///
    /// Sources differ in what they do with a range that runs past the end:
    /// exact-read sources (`FileDataSource`, `MmapDataSource`) fail, clamping
    /// ones return what they have. Asking for 16 bytes unconditionally therefore
    /// made a file's verdict depend on which source it was opened through, and
    /// on the exact-read ones it reported every short file — 2 bytes or 15 —
    /// as "Failed to read 16 bytes at offset 0", an I/O error describing this
    /// read rather than the file. So: ask for 16, and if the source refuses
    /// *because it is smaller than that*, ask again for exactly what it has and
    /// let the header parser render the verdict. A genuine I/O failure is still
    /// returned untouched, and a truncated file still errors — with the header
    /// parser's message instead of this function's.
    fn read_header_bytes<S: DataSource>(source: &S) -> Result<Vec<u8>> {
        let want = TiffHeader::BIGTIFF_HEADER_SIZE as u64;
        match source.read_range(ByteRange::from_offset_length(0, want)) {
            Ok(bytes) => Ok(bytes),
            Err(err) => {
                let Ok(size) = source.size() else {
                    return Err(err);
                };
                if size >= want {
                    return Err(err);
                }
                source.read_range(ByteRange::from_offset_length(0, size))
            }
        }
    }

    /// Parses a TIFF file from a data source
    ///
    /// # Errors
    /// Returns an error if the header is shorter than a TIFF header, declares
    /// neither `II` nor `MM`, names an unsupported version, or if no IFD can be
    /// parsed from it. A file too short to hold a header is reported as such
    /// (see [`TiffHeader::parse`]), not as an unknown format.
    pub fn parse<S: DataSource>(source: &S) -> Result<Self> {
        // Read header
        let header_bytes = Self::read_header_bytes(source)?;
        let header = TiffHeader::parse(&header_bytes)?;

        // Parse all IFDs
        let mut ifds = Vec::new();
        let mut next_offset = header.first_ifd_offset;

        while next_offset != 0 {
            if ifds.len() > 1000 {
                return Err(OxiGeoError::io_error_builder("Too many IFDs in TIFF file")
                    .with_operation("parse_tiff")
                    .with_parameter("ifd_count", ifds.len().to_string())
                    .with_parameter("max_allowed", "1000")
                    .with_parameter("next_offset", next_offset.to_string())
                    .with_suggestion("File may be corrupted or contains malicious IFD chain. Verify file integrity")
                    .build());
            }

            let ifd = Ifd::parse(source, next_offset, header.byte_order, header.variant)?;
            next_offset = ifd.next_ifd_offset;
            ifds.push(ifd);
        }

        if ifds.is_empty() {
            return Err(OxiGeoError::io_error_builder("No IFDs found in TIFF file")
                .with_operation("parse_tiff")
                .with_parameter("first_ifd_offset", header.first_ifd_offset.to_string())
                .with_suggestion("File header indicates no image directories. File may be corrupted or incomplete")
                .build());
        }

        Ok(Self { header, ifds })
    }

    /// Returns the primary (first) IFD
    #[must_use]
    pub fn primary_ifd(&self) -> &Ifd {
        &self.ifds[0]
    }

    /// Returns the number of images (IFDs)
    #[must_use]
    pub fn image_count(&self) -> usize {
        self.ifds.len()
    }

    /// Returns the byte order
    #[must_use]
    pub const fn byte_order(&self) -> ByteOrderType {
        self.header.byte_order
    }

    /// Returns true if this is a BigTIFF
    #[must_use]
    pub const fn is_bigtiff(&self) -> bool {
        self.header.is_bigtiff()
    }
}

/// Image properties extracted from an IFD
#[derive(Debug, Clone)]
pub struct ImageInfo {
    /// Image width in pixels
    pub width: u64,
    /// Image height in pixels
    pub height: u64,
    /// Bits per sample
    pub bits_per_sample: Vec<u16>,
    /// Samples per pixel
    pub samples_per_pixel: u16,
    /// Sample format
    pub sample_format: SampleFormat,
    /// Compression scheme
    pub compression: Compression,
    /// Photometric interpretation
    pub photometric: PhotometricInterpretation,
    /// Planar configuration
    pub planar_config: PlanarConfiguration,
    /// Tile width (if tiled)
    pub tile_width: Option<u32>,
    /// Tile height (if tiled)
    pub tile_height: Option<u32>,
    /// Rows per strip (if striped)
    pub rows_per_strip: Option<u32>,
    /// Predictor for compression
    pub predictor: Predictor,
}

impl ImageInfo {
    /// Extracts image info from an IFD
    pub fn from_ifd<S: DataSource>(
        ifd: &Ifd,
        source: &S,
        byte_order: ByteOrderType,
        variant: TiffVariant,
    ) -> Result<Self> {
        // Required tags
        let width = ifd
            .get_entry(TiffTag::ImageWidth)
            .ok_or(OxiGeoError::Format(FormatError::MissingTag {
                tag: "ImageWidth",
            }))?
            .get_u64_from_source(source, byte_order, variant)?;

        let height = ifd
            .get_entry(TiffTag::ImageLength)
            .ok_or(OxiGeoError::Format(FormatError::MissingTag {
                tag: "ImageLength",
            }))?
            .get_u64_from_source(source, byte_order, variant)?;

        // BitsPerSample (default: 1)
        let bits_per_sample = if let Some(entry) = ifd.get_entry(TiffTag::BitsPerSample) {
            entry
                .get_u64_vec(source, byte_order, variant)
                .map(|v| v.into_iter().map(|x| x as u16).collect())
                .unwrap_or_else(|_| vec![1])
        } else {
            vec![1]
        };

        // SamplesPerPixel (default: 1)
        let samples_per_pixel = ifd
            .get_entry(TiffTag::SamplesPerPixel)
            .and_then(|e| {
                e.get_u64_from_source(source, byte_order, variant)
                    .map(|v| v as u16)
                    .ok()
            })
            .unwrap_or(1);

        // SampleFormat (default: unsigned integer)
        let sample_format_raw = ifd
            .get_entry(TiffTag::SampleFormat)
            .and_then(|e| {
                e.get_u64_from_source(source, byte_order, variant)
                    .map(|v| v as u16)
                    .ok()
            })
            .unwrap_or(1);
        let sample_format =
            SampleFormat::from_u16(sample_format_raw).unwrap_or(SampleFormat::UnsignedInteger);

        // Compression (default: none)
        let compression_raw = ifd
            .get_entry(TiffTag::Compression)
            .and_then(|e| {
                e.get_u64_from_source(source, byte_order, variant)
                    .map(|v| v as u16)
                    .ok()
            })
            .unwrap_or(1);
        let compression = Compression::from_u16(compression_raw).unwrap_or(Compression::None);

        // PhotometricInterpretation
        let photometric_raw = ifd
            .get_entry(TiffTag::PhotometricInterpretation)
            .and_then(|e| {
                e.get_u64_from_source(source, byte_order, variant)
                    .map(|v| v as u16)
                    .ok()
            })
            .unwrap_or(1);
        let photometric = PhotometricInterpretation::from_u16(photometric_raw)
            .unwrap_or(PhotometricInterpretation::BlackIsZero);

        // PlanarConfiguration (default: chunky)
        let planar_raw = ifd
            .get_entry(TiffTag::PlanarConfiguration)
            .and_then(|e| {
                e.get_u64_from_source(source, byte_order, variant)
                    .map(|v| v as u16)
                    .ok()
            })
            .unwrap_or(1);
        let planar_config =
            PlanarConfiguration::from_u16(planar_raw).unwrap_or(PlanarConfiguration::Chunky);

        // Tile dimensions (optional - skip gracefully if unreadable)
        let tile_width = ifd.get_entry(TiffTag::TileWidth).and_then(|e| {
            e.get_u64_from_source(source, byte_order, variant)
                .map(|v| v as u32)
                .ok()
        });
        let tile_height = ifd.get_entry(TiffTag::TileLength).and_then(|e| {
            e.get_u64_from_source(source, byte_order, variant)
                .map(|v| v as u32)
                .ok()
        });

        // Rows per strip (for striped images - skip gracefully if unreadable)
        let rows_per_strip = ifd.get_entry(TiffTag::RowsPerStrip).and_then(|e| {
            e.get_u64_from_source(source, byte_order, variant)
                .map(|v| v as u32)
                .ok()
        });

        // Predictor (default: none)
        let predictor_raw = ifd
            .get_entry(TiffTag::Predictor)
            .and_then(|e| {
                e.get_u64_from_source(source, byte_order, variant)
                    .map(|v| v as u16)
                    .ok()
            })
            .unwrap_or(1);
        let predictor = Predictor::from_u16(predictor_raw).unwrap_or(Predictor::None);

        Ok(Self {
            width,
            height,
            bits_per_sample,
            samples_per_pixel,
            sample_format,
            compression,
            photometric,
            planar_config,
            tile_width,
            tile_height,
            rows_per_strip,
            predictor,
        })
    }

    /// Returns true if the image is tiled
    #[must_use]
    pub const fn is_tiled(&self) -> bool {
        self.tile_width.is_some() && self.tile_height.is_some()
    }

    /// Returns the data type
    #[must_use]
    pub fn data_type(&self) -> Option<RasterDataType> {
        let bps = *self.bits_per_sample.first()?;

        RasterDataType::from_tiff_sample_format(self.sample_format as u16, bps)
    }

    /// Returns the number of tiles in X direction
    #[must_use]
    pub fn tiles_across(&self) -> u32 {
        if let Some(tw) = self.tile_width {
            (self.width as u32).div_ceil(tw)
        } else {
            1
        }
    }

    /// Returns the number of tiles in Y direction
    #[must_use]
    pub fn tiles_down(&self) -> u32 {
        if let Some(th) = self.tile_height {
            (self.height as u32).div_ceil(th)
        } else if let Some(rps) = self.rows_per_strip {
            (self.height as u32).div_ceil(rps)
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: in-memory data source for testing
    struct MemorySource(Vec<u8>);

    impl DataSource for MemorySource {
        fn size(&self) -> Result<u64> {
            Ok(self.0.len() as u64)
        }

        fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
            Ok(self.0[range.start as usize..range.end as usize].to_vec())
        }
    }

    #[test]
    fn test_image_info_tiles() {
        let info = ImageInfo {
            width: 1024,
            height: 1024,
            bits_per_sample: vec![8],
            samples_per_pixel: 1,
            sample_format: SampleFormat::UnsignedInteger,
            compression: Compression::None,
            photometric: PhotometricInterpretation::BlackIsZero,
            planar_config: PlanarConfiguration::Chunky,
            tile_width: Some(256),
            tile_height: Some(256),
            rows_per_strip: None,
            predictor: Predictor::None,
        };

        assert!(info.is_tiled());
        assert_eq!(info.tiles_across(), 4);
        assert_eq!(info.tiles_down(), 4);
        assert_eq!(info.data_type(), Some(RasterDataType::UInt8));
    }
}
