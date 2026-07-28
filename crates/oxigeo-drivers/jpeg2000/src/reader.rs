//! High-level JP2/J2K reader
//!
//! This module provides a high-level interface for reading JPEG2000 files.

use crate::box_reader::{BoxReader, BoxType};
use crate::codestream::{
    CodestreamParser, CodingStyle, ImageSize, Marker, Quantization, WaveletTransform,
};
use crate::color::ColorConverter;
use crate::error::{Jpeg2000Error, ResilienceMode, Result};
use crate::metadata::{EnumeratedColorSpace, Jp2Metadata};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor as IoCursor, Read, Seek, SeekFrom};

/// JPEG2000 reader
pub struct Jpeg2000Reader<R> {
    /// Input reader
    reader: R,
    /// JP2 metadata
    metadata: Option<Jp2Metadata>,
    /// Codestream image size
    image_size: Option<ImageSize>,
    /// Coding style
    coding_style: Option<CodingStyle>,
    /// Quantization
    quantization: Option<Quantization>,
    /// Is JP2 format (vs raw codestream)
    is_jp2: bool,
    /// Error resilience mode
    resilience_mode: ResilienceMode,
    /// Progressive decoding state
    progressive_state: Option<ProgressiveDecodingState>,
    /// Raw codestream bytes (stored after parsing for decode use)
    raw_codestream: Option<Vec<u8>>,
}

/// Progressive decoding state
#[derive(Debug, Clone)]
struct ProgressiveDecodingState {
    /// Current quality layer being decoded
    current_layer: u16,
    /// Maximum quality layer available
    #[allow(dead_code)]
    max_layers: u16,
    /// Intermediate decoded data (partial quality)
    intermediate_data: Vec<u8>,
    /// Width of intermediate image
    #[allow(dead_code)]
    width: usize,
    /// Height of intermediate image
    #[allow(dead_code)]
    height: usize,
}

impl<R: Read + Seek> Jpeg2000Reader<R> {
    /// Create new JPEG2000 reader
    pub fn new(mut reader: R) -> Result<Self> {
        // Try to detect format - but handle truncated files gracefully
        let mut magic = [0u8; 12];
        let is_jp2 = match reader.read_exact(&mut magic) {
            Ok(()) => {
                reader.seek(SeekFrom::Start(0))?;
                magic[4..8] == *b"jP  "
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // File is too short to be JP2
                // But check if it's completely empty by trying to read at least 2 bytes
                reader.seek(SeekFrom::Start(0))?;
                let mut min_magic = [0u8; 2];
                match reader.read_exact(&mut min_magic) {
                    Ok(()) => {
                        // Has at least 2 bytes, could be J2K
                        reader.seek(SeekFrom::Start(0))?;
                        false
                    }
                    Err(_) => {
                        // Can't even read 2 bytes - file is too small
                        return Err(Jpeg2000Error::CodestreamError(
                            "File too small to be valid JPEG2000".to_string(),
                        ));
                    }
                }
            }
            Err(e) => return Err(e.into()),
        };

        Ok(Self {
            reader,
            metadata: None,
            image_size: None,
            coding_style: None,
            quantization: None,
            is_jp2,
            resilience_mode: ResilienceMode::default(),
            progressive_state: None,
            raw_codestream: None,
        })
    }

    /// Set error resilience mode
    pub fn set_resilience_mode(&mut self, mode: ResilienceMode) {
        self.resilience_mode = mode;
    }

    /// Get current error resilience mode
    pub fn resilience_mode(&self) -> ResilienceMode {
        self.resilience_mode
    }

    /// Enable basic error resilience
    pub fn enable_error_resilience(&mut self) {
        self.resilience_mode = ResilienceMode::Basic;
    }

    /// Enable full error resilience with aggressive recovery
    pub fn enable_full_error_resilience(&mut self) {
        self.resilience_mode = ResilienceMode::Full;
    }

    /// Disable error resilience
    pub fn disable_error_resilience(&mut self) {
        self.resilience_mode = ResilienceMode::None;
    }

    /// Parse file headers
    pub fn parse_headers(&mut self) -> Result<()> {
        if self.is_jp2 {
            self.parse_jp2_headers()?;
        } else {
            self.parse_j2k_headers()?;
        }

        Ok(())
    }

    /// Parse JP2 format headers
    fn parse_jp2_headers(&mut self) -> Result<()> {
        // Parse JP2 metadata (includes ftyp, jp2h with ihdr and colr)
        self.metadata = Some(Jp2Metadata::parse(&mut self.reader)?);

        // Parse additional optional boxes (resolution, XML, UUID)
        self.parse_optional_boxes()?;

        // Find codestream box
        let mut box_reader = BoxReader::new(&mut self.reader)?;

        if let Some(jp2c_header) = box_reader.find_box(BoxType::ContiguousCodestream)? {
            // Read codestream data
            let codestream_data = box_reader.read_box_data(&jp2c_header)?;

            // Store raw codestream for later decoding
            self.raw_codestream = Some(codestream_data.clone());

            // Parse codestream
            let mut parser = CodestreamParser::new(std::io::Cursor::new(&codestream_data));
            self.parse_codestream(&mut parser)?;
        } else {
            return Err(Jpeg2000Error::BoxParseError {
                box_type: "jp2c".to_string(),
                reason: "Codestream box not found".to_string(),
            });
        }

        Ok(())
    }

    /// Parse optional JP2 boxes (resolution, XML, UUID, etc.)
    fn parse_optional_boxes(&mut self) -> Result<()> {
        let mut box_reader = BoxReader::new(&mut self.reader)?;

        // Look for Resolution box inside jp2h
        box_reader.reset()?;
        if let Some(jp2h_header) = box_reader.find_box(BoxType::Jp2Header)? {
            let jp2h_data = box_reader.read_box_data(&jp2h_header)?;
            let mut jp2h_cursor = std::io::Cursor::new(&jp2h_data);
            let mut sub_reader = BoxReader::new(&mut jp2h_cursor)?;

            // Look for resolution superbox
            if let Some(res_header) = sub_reader.find_box(BoxType::Resolution)? {
                let res_data = sub_reader.read_box_data(&res_header)?;
                let mut res_cursor = std::io::Cursor::new(&res_data);
                let mut res_sub_reader = BoxReader::new(&mut res_cursor)?;

                // Parse capture resolution
                if let Some(resc_header) = res_sub_reader.find_box(BoxType::CaptureResolution)? {
                    let resc_data = res_sub_reader.read_box_data(&resc_header)?;
                    let mut resc_cursor = std::io::Cursor::new(&resc_data);
                    if let Some(ref mut metadata) = self.metadata {
                        metadata.capture_resolution =
                            Some(crate::metadata::Resolution::parse(&mut resc_cursor)?);
                    }
                }

                // Parse display resolution
                res_sub_reader.reset()?;
                if let Some(resd_header) = res_sub_reader.find_box(BoxType::DisplayResolution)? {
                    let resd_data = res_sub_reader.read_box_data(&resd_header)?;
                    let mut resd_cursor = std::io::Cursor::new(&resd_data);
                    if let Some(ref mut metadata) = self.metadata {
                        metadata.display_resolution =
                            Some(crate::metadata::Resolution::parse(&mut resd_cursor)?);
                    }
                }
            }
        }

        // Parse XML boxes (can be multiple)
        box_reader.reset()?;
        while let Some(xml_header) = box_reader.find_box(BoxType::Xml)? {
            let xml_data = box_reader.read_box_data(&xml_header)?;
            let mut xml_cursor = std::io::Cursor::new(&xml_data);
            if let Some(ref mut metadata) = self.metadata
                && let Ok(xml_box) =
                    crate::metadata::XmlMetadata::parse(&mut xml_cursor, xml_header.data_size())
            {
                metadata.xml_boxes.push(xml_box);
            }
        }

        // Parse UUID boxes (can be multiple)
        box_reader.reset()?;
        while let Some(uuid_header) = box_reader.find_box(BoxType::Uuid)? {
            let uuid_data = box_reader.read_box_data(&uuid_header)?;
            let mut uuid_cursor = std::io::Cursor::new(&uuid_data);
            if let Some(ref mut metadata) = self.metadata
                && let Ok(uuid_box) =
                    crate::metadata::UuidBox::parse(&mut uuid_cursor, uuid_header.data_size())
            {
                metadata.uuid_boxes.push(uuid_box);
            }
        }

        Ok(())
    }

    /// Parse raw J2K codestream headers
    fn parse_j2k_headers(&mut self) -> Result<()> {
        // Read entire codestream into buffer to avoid borrow checker issues
        let mut codestream_data = Vec::new();
        self.reader.read_to_end(&mut codestream_data)?;

        // Store raw codestream for later decoding
        self.raw_codestream = Some(codestream_data.clone());

        let mut parser = CodestreamParser::new(std::io::Cursor::new(&codestream_data));
        self.parse_codestream(&mut parser)?;
        Ok(())
    }

    /// Parse codestream
    fn parse_codestream<CS: Read>(&mut self, parser: &mut CodestreamParser<CS>) -> Result<()> {
        // Verify SOC marker
        match parser.read_marker() {
            Ok(Some(Marker::Soc)) => {}
            Ok(Some(m)) => {
                if self.resilience_mode.is_enabled() {
                    tracing::warn!(
                        "Expected SOC marker, got {:?}, continuing with resilience mode",
                        m
                    );
                } else {
                    return Err(Jpeg2000Error::CodestreamError(format!(
                        "Expected SOC marker, got {:?}",
                        m
                    )));
                }
            }
            Ok(None) => {
                if self.resilience_mode.is_enabled() {
                    tracing::warn!(
                        "Unexpected end of stream at SOC, continuing with resilience mode"
                    );
                } else {
                    return Err(Jpeg2000Error::CodestreamError(
                        "Unexpected end of stream".to_string(),
                    ));
                }
            }
            Err(e) => {
                if self.resilience_mode.is_enabled() {
                    tracing::warn!(
                        "Error reading SOC marker: {}, continuing with resilience mode",
                        e
                    );
                } else {
                    return Err(e);
                }
            }
        }

        // Parse main header markers
        loop {
            let marker_result = parser.read_marker();

            match marker_result {
                Ok(Some(Marker::Siz)) => match parser.parse_siz() {
                    Ok(siz) => self.image_size = Some(siz),
                    Err(e) => {
                        if self.resilience_mode.is_enabled() {
                            tracing::warn!(
                                "Error parsing SIZ marker: {}, using error concealment",
                                e
                            );
                        } else {
                            return Err(e);
                        }
                    }
                },
                Ok(Some(Marker::Cod)) => match parser.parse_cod() {
                    Ok(cod) => self.coding_style = Some(cod),
                    Err(e) => {
                        if self.resilience_mode.is_enabled() {
                            tracing::warn!("Error parsing COD marker: {}, using defaults", e);
                        } else {
                            return Err(e);
                        }
                    }
                },
                Ok(Some(Marker::Qcd)) => match parser.parse_qcd() {
                    Ok(qcd) => self.quantization = Some(qcd),
                    Err(e) => {
                        if self.resilience_mode.is_enabled() {
                            tracing::warn!("Error parsing QCD marker: {}, using defaults", e);
                        } else {
                            return Err(e);
                        }
                    }
                },
                Ok(Some(Marker::Sot)) => {
                    // Start of tile - main header complete
                    break;
                }
                Ok(Some(Marker::Eoc)) => {
                    // End of codestream
                    break;
                }
                Ok(Some(marker)) => {
                    // Skip other markers
                    if marker.has_segment() {
                        match parser.read_segment_length() {
                            Ok(length) => {
                                if let Err(e) = parser.skip_segment(length) {
                                    if self.resilience_mode.is_enabled() {
                                        tracing::warn!(
                                            "Error skipping marker segment: {}, continuing",
                                            e
                                        );
                                    } else {
                                        return Err(e);
                                    }
                                }
                            }
                            Err(e) => {
                                if self.resilience_mode.is_enabled() {
                                    tracing::warn!(
                                        "Error reading segment length: {}, continuing",
                                        e
                                    );
                                } else {
                                    return Err(e);
                                }
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    if self.resilience_mode.is_enabled() {
                        tracing::warn!("Error reading marker: {}, attempting to continue", e);
                        break;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        // Verify required markers were found
        if self.image_size.is_none() {
            if self.resilience_mode.is_full() {
                tracing::warn!("SIZ marker not found, using error concealment with default size");
                // In full resilience mode, continue without SIZ (will fail later but gracefully)
            } else {
                return Err(Jpeg2000Error::CodestreamError(
                    "SIZ marker not found".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Get image width
    pub fn width(&self) -> Result<u32> {
        if let Some(ref size) = self.image_size {
            Ok(size.width)
        } else if let Some(ref metadata) = self.metadata {
            metadata
                .image_header
                .as_ref()
                .map(|h| h.width)
                .ok_or_else(|| Jpeg2000Error::InvalidImageHeader("No image header".to_string()))
        } else {
            Err(Jpeg2000Error::InvalidImageHeader(
                "Image size not available".to_string(),
            ))
        }
    }

    /// Get image height
    pub fn height(&self) -> Result<u32> {
        if let Some(ref size) = self.image_size {
            Ok(size.height)
        } else if let Some(ref metadata) = self.metadata {
            metadata
                .image_header
                .as_ref()
                .map(|h| h.height)
                .ok_or_else(|| Jpeg2000Error::InvalidImageHeader("No image header".to_string()))
        } else {
            Err(Jpeg2000Error::InvalidImageHeader(
                "Image size not available".to_string(),
            ))
        }
    }

    /// Get number of components
    pub fn num_components(&self) -> Result<u16> {
        if let Some(ref size) = self.image_size {
            Ok(size.num_components)
        } else if let Some(ref metadata) = self.metadata {
            metadata
                .image_header
                .as_ref()
                .map(|h| h.num_components)
                .ok_or_else(|| Jpeg2000Error::InvalidImageHeader("No image header".to_string()))
        } else {
            Err(Jpeg2000Error::InvalidImageHeader(
                "Image size not available".to_string(),
            ))
        }
    }

    /// Get metadata
    pub fn metadata(&self) -> Option<&Jp2Metadata> {
        self.metadata.as_ref()
    }

    /// Find the raw bitstream bytes for a given tile index in the stored codestream.
    ///
    /// Returns the bytes starting immediately after the SOD marker of the requested tile.
    fn find_tile_bitstream(&self, tile_index: u32) -> Result<Vec<u8>> {
        let codestream = self.raw_codestream.as_ref().ok_or_else(|| {
            Jpeg2000Error::CodestreamError("No raw codestream stored".to_string())
        })?;

        let mut cursor = IoCursor::new(codestream.as_slice());

        // Check SOC marker (0xFF4F)
        let soc = cursor
            .read_u16::<BigEndian>()
            .map_err(|e| Jpeg2000Error::CodestreamError(format!("Read SOC: {}", e)))?;
        if soc != 0xFF4F {
            return Err(Jpeg2000Error::CodestreamError(format!(
                "Expected SOC 0xFF4F, got 0x{:04X}",
                soc
            )));
        }

        // Skip main header markers until SOT
        loop {
            let marker_val = cursor
                .read_u16::<BigEndian>()
                .map_err(|e| Jpeg2000Error::CodestreamError(format!("Read marker: {}", e)))?;

            match marker_val {
                0xFF90 => {
                    // SOT — start of tile
                    let lsot = cursor
                        .read_u16::<BigEndian>()
                        .map_err(|e| Jpeg2000Error::CodestreamError(format!("Read Lsot: {}", e)))?;
                    let isot = cursor
                        .read_u16::<BigEndian>()
                        .map_err(|e| Jpeg2000Error::CodestreamError(format!("Read Isot: {}", e)))?;
                    let psot = cursor
                        .read_u32::<BigEndian>()
                        .map_err(|e| Jpeg2000Error::CodestreamError(format!("Read Psot: {}", e)))?;
                    let _tpsot = cursor.read_u8().map_err(|e| {
                        Jpeg2000Error::CodestreamError(format!("Read TPsot: {}", e))
                    })?;
                    let _tnsot = cursor.read_u8().map_err(|e| {
                        Jpeg2000Error::CodestreamError(format!("Read TNsot: {}", e))
                    })?;

                    // psot counts from the SOT marker (0xFF90); we've consumed
                    // 2 (marker) + lsot bytes, so recover the SOT start offset.
                    let cur_pos = cursor.position() as usize;
                    let sot_start = cur_pos.saturating_sub(usize::from(lsot)).saturating_sub(2);
                    // Exclusive end of *this* tile-part's data. `Psot == 0` means
                    // "extends to the end of the codestream" (only legal for the
                    // last tile-part), so bound by the whole codestream length.
                    let tile_part_end = if psot > 0 {
                        (sot_start + psot as usize).min(codestream.len())
                    } else {
                        codestream.len()
                    };

                    if u32::from(isot) != tile_index {
                        if psot > 0 {
                            cursor.set_position(tile_part_end as u64);
                            continue;
                        }
                        // A zero-length tile-part before the requested tile leaves
                        // no way to locate a following SOT; fail loud rather than
                        // return another tile's bytes.
                        return Err(Jpeg2000Error::CodestreamError(format!(
                            "Tile {} not found: encountered a tile-part with unknown \
                             length (Psot=0) for tile {} first",
                            tile_index, isot
                        )));
                    }

                    // Tile found — now parse tile-part header markers until SOD
                    loop {
                        let inner = cursor.read_u16::<BigEndian>().map_err(|e| {
                            Jpeg2000Error::CodestreamError(format!(
                                "Read tile header marker: {}",
                                e
                            ))
                        })?;
                        match inner {
                            0xFF93 => {
                                // SOD — packet data starts here. Bound the returned
                                // slice by this tile-part's own end (Psot-derived),
                                // never the end of the whole codestream, so multi-tile
                                // streams don't bleed one tile's bytes into the next.
                                let sod_pos = cursor.position() as usize;
                                let mut end = tile_part_end.min(codestream.len());
                                // Strip a trailing EOC (0xFFD9) if the slice runs to
                                // the very end of the codestream (Psot == 0 case).
                                if end >= sod_pos + 2
                                    && codestream[end - 2] == 0xFF
                                    && codestream[end - 1] == 0xD9
                                {
                                    end -= 2;
                                }
                                let start = sod_pos.min(end);
                                return Ok(codestream[start..end].to_vec());
                            }
                            0xFFD9 => {
                                return Ok(Vec::new());
                            }
                            _ => {
                                let seg_len = cursor.read_u16::<BigEndian>().map_err(|e| {
                                    Jpeg2000Error::CodestreamError(format!("Read seg len: {}", e))
                                })?;
                                if seg_len >= 2 {
                                    cursor
                                        .seek(SeekFrom::Current(i64::from(seg_len) - 2))
                                        .map_err(|e| {
                                            Jpeg2000Error::CodestreamError(format!("Seek: {}", e))
                                        })?;
                                }
                            }
                        }
                    }
                }
                0xFFD9 => {
                    return Err(Jpeg2000Error::CodestreamError(format!(
                        "Tile {} not found before EOC",
                        tile_index
                    )));
                }
                _ => {
                    if marker_val & 0xFF00 == 0xFF00 && marker_val != 0xFF4F {
                        let len = cursor.read_u16::<BigEndian>().map_err(|e| {
                            Jpeg2000Error::CodestreamError(format!("Read main hdr len: {}", e))
                        })?;
                        if len >= 2 {
                            cursor
                                .seek(SeekFrom::Current(i64::from(len) - 2))
                                .map_err(|e| {
                                    Jpeg2000Error::CodestreamError(format!("Seek main hdr: {}", e))
                                })?;
                        }
                    }
                }
            }
        }
    }

    /// Decode all components for a given tile, returning per-component sample
    /// arrays (post inverse-DWT, pre colour transform).
    ///
    /// This drives the real Tier-2 packet demultiplexer
    /// ([`crate::tier2::tile::decode_tile_components`]): packet headers are
    /// parsed per the COD progression order and the exact code-block
    /// contribution bytes are sliced and fed to Tier-1 EBCOT, rather than
    /// splitting the SOD byte range by naive even division.  RCT is applied
    /// afterwards when MCT is enabled.
    fn decode_tile_to_components(&self, tile_x: u32, tile_y: u32) -> Result<Vec<Vec<i32>>> {
        let image_size = self
            .image_size
            .as_ref()
            .ok_or_else(|| Jpeg2000Error::InvalidImageHeader("No image size".to_string()))?;
        let coding_style = self
            .coding_style
            .as_ref()
            .ok_or_else(|| Jpeg2000Error::CodestreamError("No coding style".to_string()))?;

        // Raster tile index (row-major): tile-parts are addressed by this Isot.
        let num_tiles_x = image_size.num_tiles_x();
        let num_tiles_y = image_size.num_tiles_y();
        if tile_x >= num_tiles_x || tile_y >= num_tiles_y {
            return Err(Jpeg2000Error::InvalidTile(format!(
                "Tile ({}, {}) out of bounds ({}x{} tiles)",
                tile_x, tile_y, num_tiles_x, num_tiles_y
            )));
        }
        let tile_index = tile_y * num_tiles_x + tile_x;

        // Only the reversible 5/3 reconstruction path is implemented; fail loud
        // (rather than mis-reconstruct) for the 9/7 irreversible transform.
        if coding_style.wavelet == WaveletTransform::Irreversible97 {
            return Err(Jpeg2000Error::UnsupportedFeature(
                "9/7 irreversible wavelet decode is not yet implemented; use reversible 5/3 encoding".to_string()
            ));
        }

        let num_components = image_size.num_components as usize;
        let tile_w = image_size.tile_width as usize;
        let tile_h = image_size.tile_height as usize;
        let num_levels = u32::from(coding_style.num_levels);
        let cbw = coding_style.code_block_width_px();
        let cbh = coding_style.code_block_height_px();

        let tile_data = self.find_tile_bitstream(tile_index)?;

        let mut comp_inputs = Vec::with_capacity(num_components);
        for comp in 0..num_components {
            let dx = usize::from(image_size.components.get(comp).map(|c| c.dx).unwrap_or(1)).max(1);
            let dy = usize::from(image_size.components.get(comp).map(|c| c.dy).unwrap_or(1)).max(1);
            let comp_w = tile_w.div_ceil(dx).max(1);
            let comp_h = tile_h.div_ceil(dy).max(1);
            let precision = image_size
                .components
                .get(comp)
                .map(|c| c.precision)
                .unwrap_or(8);
            comp_inputs.push(crate::tier2::tile::TileComponentInput {
                comp_w,
                comp_h,
                precision,
            });
        }

        let guard_bits = self
            .quantization
            .as_ref()
            .map(|q| q.guard_bits)
            .unwrap_or(2);

        let params = crate::tier2::tile::TileDecodeParams {
            components: &comp_inputs,
            num_levels,
            cbw,
            cbh,
            progression: coding_style.progression_order,
            num_layers: coding_style.num_layers,
            guard_bits,
            quantization: self.quantization.as_ref(),
            has_sop: coding_style.has_sop,
            has_eph: coding_style.has_eph,
        };

        let mut component_coeffs = crate::tier2::tile::decode_tile_components(&tile_data, &params)?;

        if coding_style.use_mct && num_components >= 3 {
            ColorConverter::apply_rct(&mut component_coeffs)
                .map_err(|e| Jpeg2000Error::CodestreamError(format!("RCT failed: {:?}", e)))?;
        }

        Ok(component_coeffs)
    }

    /// Decode image to RGB
    ///
    /// Decodes **every** tile of the image and composites each tile's samples at
    /// its correct pixel offset in the output raster (multi-tile geospatial
    /// rasters are the common case), rather than assuming a single tile spans
    /// the whole image.
    pub fn decode_rgb(&mut self) -> Result<Vec<u8>> {
        let width = self.width()? as usize;
        let height = self.height()? as usize;
        let num_components = self.num_components()? as usize;

        // Never fabricate pixel data: if the codestream was never stored (e.g.
        // headers were only partially parsed) this is a programming/parse error,
        // not a gray image.
        if self.raw_codestream.is_none() {
            return Err(Jpeg2000Error::CodestreamError(
                "decode_rgb called before the codestream was parsed \
                 (call parse_headers first)"
                    .to_string(),
            ));
        }

        let (num_tiles_x, num_tiles_y, tile_width, tile_height, tile_x_offset, tile_y_offset) = {
            let s = self
                .image_size
                .as_ref()
                .ok_or_else(|| Jpeg2000Error::InvalidImageHeader("No image size".to_string()))?;
            (
                s.num_tiles_x(),
                s.num_tiles_y(),
                s.tile_width as usize,
                s.tile_height as usize,
                s.tile_x_offset as usize,
                s.tile_y_offset as usize,
            )
        };

        let precision = self
            .image_size
            .as_ref()
            .and_then(|s| s.components.first())
            .map(|c| c.precision)
            .unwrap_or(8);
        let is_signed = self
            .image_size
            .as_ref()
            .and_then(|s| s.components.first())
            .map(|c| c.is_signed)
            .unwrap_or(false);

        let mut rgb = vec![128u8; width * height * 3];

        for ty in 0..num_tiles_y {
            for tx in 0..num_tiles_x {
                let component_samples = self.decode_tile_to_components(tx, ty)?;
                let shifted: Vec<Vec<u8>> = component_samples
                    .iter()
                    .map(|comp| crate::color::level_shift(comp, precision, is_signed))
                    .collect();

                let ox = (tile_x_offset + tx as usize * tile_width).min(width);
                let oy = (tile_y_offset + ty as usize * tile_height).min(height);
                let tx1 = (tile_x_offset + (tx as usize + 1) * tile_width).min(width);
                let ty1 = (tile_y_offset + (ty as usize + 1) * tile_height).min(height);

                place_tile_samples(
                    &mut rgb,
                    width,
                    &shifted,
                    num_components,
                    tile_width,
                    ox,
                    oy,
                    tx1,
                    ty1,
                );
            }
        }

        Ok(rgb)
    }

    /// Decode image to RGBA
    pub fn decode_rgba(&mut self) -> Result<Vec<u8>> {
        let rgb = self.decode_rgb()?;
        let num_pixels = rgb.len() / 3;

        let mut rgba = Vec::with_capacity(num_pixels * 4);

        for i in 0..num_pixels {
            rgba.push(rgb[i * 3]);
            rgba.push(rgb[i * 3 + 1]);
            rgba.push(rgb[i * 3 + 2]);
            rgba.push(255);
        }

        Ok(rgba)
    }

    /// Decode specific tile
    pub fn decode_tile(&mut self, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        {
            let image_size = self.image_size.as_ref().ok_or_else(|| {
                Jpeg2000Error::InvalidImageHeader("Image size not available".to_string())
            })?;

            if tile_x >= image_size.num_tiles_x() || tile_y >= image_size.num_tiles_y() {
                return Err(Jpeg2000Error::InvalidTile(format!(
                    "Tile ({}, {}) out of bounds",
                    tile_x, tile_y
                )));
            }
        }

        if self.raw_codestream.is_none() {
            return Err(Jpeg2000Error::CodestreamError(
                "decode_tile called before the codestream was parsed \
                 (call parse_headers first)"
                    .to_string(),
            ));
        }

        let component_samples = self.decode_tile_to_components(tile_x, tile_y)?;

        let precision = self
            .image_size
            .as_ref()
            .and_then(|s| s.components.first())
            .map(|c| c.precision)
            .unwrap_or(8);
        let is_signed = self
            .image_size
            .as_ref()
            .and_then(|s| s.components.first())
            .map(|c| c.is_signed)
            .unwrap_or(false);

        let tile_w = self
            .image_size
            .as_ref()
            .map(|s| s.tile_width as usize)
            .unwrap_or(0);
        let tile_h = self
            .image_size
            .as_ref()
            .map(|s| s.tile_height as usize)
            .unwrap_or(0);
        let num_components = self.num_components()? as usize;

        let shifted: Vec<Vec<u8>> = component_samples
            .iter()
            .map(|comp| crate::color::level_shift(comp, precision, is_signed))
            .collect();

        let num_pixels = tile_w * tile_h;
        let mut rgb = vec![128u8; num_pixels * 3];

        if num_components >= 3 && shifted.len() >= 3 {
            for i in 0..num_pixels {
                rgb[i * 3] = shifted[0].get(i).copied().unwrap_or(128);
                rgb[i * 3 + 1] = shifted[1].get(i).copied().unwrap_or(128);
                rgb[i * 3 + 2] = shifted[2].get(i).copied().unwrap_or(128);
            }
        } else if !shifted.is_empty() {
            for i in 0..num_pixels {
                let gray = shifted[0].get(i).copied().unwrap_or(128);
                rgb[i * 3] = gray;
                rgb[i * 3 + 1] = gray;
                rgb[i * 3 + 2] = gray;
            }
        }

        Ok(rgb)
    }

    /// Get information about the image
    pub fn info(&self) -> Result<ImageInfo> {
        let width = self.width()?;
        let height = self.height()?;
        let num_components = self.num_components()?;

        let num_tiles = if let Some(ref size) = self.image_size {
            size.num_tiles()
        } else {
            1
        };

        let color_space = self
            .metadata
            .as_ref()
            .and_then(|m| m.color_spec.as_ref())
            .and_then(|c| c.enum_cs);

        let num_levels = self
            .coding_style
            .as_ref()
            .map(|cs| cs.num_levels)
            .unwrap_or(0);

        Ok(ImageInfo {
            width,
            height,
            num_components,
            num_tiles,
            color_space,
            num_decomposition_levels: num_levels,
            is_jp2: self.is_jp2,
        })
    }

    /// Get file type information (JP2 format only)
    pub fn file_type(&self) -> Option<&crate::metadata::FileType> {
        self.metadata.as_ref()?.file_type.as_ref()
    }

    /// Get image header information
    pub fn image_header(&self) -> Option<&crate::metadata::ImageHeader> {
        self.metadata.as_ref()?.image_header.as_ref()
    }

    /// Get color specification
    pub fn color_specification(&self) -> Option<&crate::metadata::ColorSpecification> {
        self.metadata.as_ref()?.color_spec.as_ref()
    }

    /// Get capture resolution (if present)
    pub fn capture_resolution(&self) -> Option<&crate::metadata::Resolution> {
        self.metadata.as_ref()?.capture_resolution.as_ref()
    }

    /// Get display resolution (if present)
    pub fn display_resolution(&self) -> Option<&crate::metadata::Resolution> {
        self.metadata.as_ref()?.display_resolution.as_ref()
    }

    /// Get capture resolution in DPI (if present)
    pub fn capture_resolution_dpi(&self) -> Option<(f64, f64)> {
        self.capture_resolution().map(|r| r.to_dpi())
    }

    /// Get display resolution in DPI (if present)
    pub fn display_resolution_dpi(&self) -> Option<(f64, f64)> {
        self.display_resolution().map(|r| r.to_dpi())
    }

    /// Get XML metadata boxes
    pub fn xml_metadata(&self) -> Vec<&crate::metadata::XmlMetadata> {
        self.metadata
            .as_ref()
            .map(|m| m.xml_boxes.iter().collect())
            .unwrap_or_default()
    }

    /// Get UUID boxes
    pub fn uuid_boxes(&self) -> Vec<&crate::metadata::UuidBox> {
        self.metadata
            .as_ref()
            .map(|m| m.uuid_boxes.iter().collect())
            .unwrap_or_default()
    }

    /// Get coding style information
    pub fn coding_style(&self) -> Option<&CodingStyle> {
        self.coding_style.as_ref()
    }

    /// Get quantization information
    pub fn quantization(&self) -> Option<&Quantization> {
        self.quantization.as_ref()
    }

    /// Get image size information from codestream
    pub fn image_size_info(&self) -> Option<&ImageSize> {
        self.image_size.as_ref()
    }

    /// Check if image uses multiple component transform (MCT)
    pub fn uses_mct(&self) -> bool {
        self.coding_style
            .as_ref()
            .map(|cs| cs.use_mct)
            .unwrap_or(false)
    }

    /// Get number of quality layers
    pub fn num_quality_layers(&self) -> u16 {
        self.coding_style
            .as_ref()
            .map(|cs| cs.num_layers)
            .unwrap_or(1)
    }

    /// Get number of decomposition levels
    pub fn num_decomposition_levels(&self) -> u8 {
        self.coding_style
            .as_ref()
            .map(|cs| cs.num_levels)
            .unwrap_or(0)
    }

    /// Decode image and record the requested quality layer.
    ///
    /// # Current behaviour (important)
    ///
    /// This method performs a **full-quality decode of the whole codestream on
    /// every call** and returns real pixel data. It does *not* yet provide a
    /// speed or bandwidth benefit for lower `max_layer` values: true
    /// layer-limited decoding (including only the packets for layers
    /// `0..=max_layer`) depends on the multi-layer Tier-2 packet path, which is
    /// not yet wired into the main decode pipeline (single-layer streams are the
    /// only ones the Tier-2 demultiplexer currently accepts). `max_layer` is
    /// validated against the available layer count and recorded as the
    /// caller-requested layer in the progressive state, but the returned image
    /// is always the full-quality decode.
    ///
    /// The output is therefore always *correct* (never a lower-fidelity
    /// approximation); only the performance contract of partial decoding is
    /// pending. Callers relying on a fast low-quality preview should treat this
    /// as a full decode for now.
    ///
    /// # Arguments
    ///
    /// * `max_layer` - Requested maximum quality layer (0-based index); must be
    ///   less than [`Self::num_quality_layers`].
    ///
    /// # Returns
    ///
    /// Full-quality RGB image data.
    pub fn decode_quality_layers(&mut self, max_layer: u16) -> Result<Vec<u8>> {
        let width = self.width()? as usize;
        let height = self.height()? as usize;
        let num_layers = self.num_quality_layers();

        if max_layer >= num_layers {
            return Err(Jpeg2000Error::Tier2Error(format!(
                "Requested layer {} exceeds available layers {}",
                max_layer, num_layers
            )));
        }

        tracing::info!(
            "Decoding quality layers 0-{} of {} (progressive)",
            max_layer,
            num_layers
        );

        // Perform a real decode and return actual pixel data rather than the former
        // flat-gray placeholder.
        //
        // NOTE: true layer-limited tier-2 decoding (including only packets for
        // layers 0..=max_layer) is not yet wired here because the tier-2 packet
        // path is not integrated into the main decode pipeline (see the tile
        // demultiplexing note in decode_tile_to_components). Until that lands we
        // decode the full codestream and return real imagery, favouring pixel
        // correctness over false progressiveness. `max_layer` still governs which
        // layers a caller *requested* (recorded in the progressive state) and is
        // validated above, but every call currently yields the full-quality image.
        let rgb = self.decode_rgb()?;

        // Record/refresh the progressive decoding state with the real image data.
        self.progressive_state = Some(ProgressiveDecodingState {
            current_layer: max_layer,
            max_layers: num_layers,
            intermediate_data: rgb.clone(),
            width,
            height,
        });

        tracing::info!(
            "Progressive decode returned real image data for requested layer {} \
             (full-quality decode; layer-limited decode is a pending enhancement)",
            max_layer
        );

        Ok(rgb)
    }

    /// Decode image progressively with automatic layer progression.
    ///
    /// Returns an iterator that yields one image per quality layer. Note that,
    /// as documented on [`Self::decode_quality_layers`], each yielded image is
    /// currently a full-quality decode (layer-limited Tier-2 decoding is a
    /// pending enhancement), so the iterator yields `num_layers` identical
    /// full-quality frames rather than progressively refined ones.
    pub fn decode_progressive(&mut self) -> Result<ProgressiveDecoder<'_, R>> {
        let num_layers = self.num_quality_layers();

        Ok(ProgressiveDecoder {
            reader: self,
            current_layer: 0,
            max_layers: num_layers,
        })
    }

    /// Get current progressive decoding state
    pub fn progressive_layer(&self) -> Option<u16> {
        self.progressive_state.as_ref().map(|s| s.current_layer)
    }

    /// Get the most recently decoded progressive image buffer, if any.
    ///
    /// Returns the RGB pixel data produced by the last call to
    /// [`Self::decode_quality_layers`] (or the [`ProgressiveDecoder`] iterator),
    /// or `None` if no progressive decode has run yet.
    pub fn progressive_data(&self) -> Option<&[u8]> {
        self.progressive_state
            .as_ref()
            .map(|s| s.intermediate_data.as_slice())
    }

    /// Reset progressive decoding state
    pub fn reset_progressive_state(&mut self) {
        self.progressive_state = None;
    }

    /// Check if progressive decoding is in progress
    pub fn is_progressive_active(&self) -> bool {
        self.progressive_state.is_some()
    }

    /// Decode a specific region of interest (ROI) from the image
    ///
    /// This method decodes only the specified rectangular region, which can be
    /// more efficient than decoding the entire image when only a portion is needed.
    ///
    /// # Arguments
    ///
    /// * `x` - Left coordinate of the region (pixels)
    /// * `y` - Top coordinate of the region (pixels)
    /// * `width` - Width of the region (pixels)
    /// * `height` - Height of the region (pixels)
    ///
    /// # Returns
    ///
    /// RGB image data for the specified region
    pub fn decode_region(&mut self, x: u32, y: u32, width: u32, height: u32) -> Result<Vec<u8>> {
        let image_width = self.width()?;
        let image_height = self.height()?;

        // Validate region bounds
        if x + width > image_width {
            return Err(Jpeg2000Error::InvalidDimension(format!(
                "Region x+width ({}) exceeds image width ({})",
                x + width,
                image_width
            )));
        }

        if y + height > image_height {
            return Err(Jpeg2000Error::InvalidDimension(format!(
                "Region y+height ({}) exceeds image height ({})",
                y + height,
                image_height
            )));
        }

        tracing::info!(
            "Decoding region: {}x{} at ({}, {}) from {}x{} image",
            width,
            height,
            x,
            y,
            image_width,
            image_height
        );

        // Determine which tiles intersect with the requested region (for diagnostics)
        let tiles = self.compute_intersecting_tiles(x, y, width, height)?;
        tracing::debug!("Region intersects with {} tiles", tiles.len());

        let full_rgb = self.decode_rgb()?;
        let full_width = image_width as usize;
        let full_height = image_height as usize;

        let x_usize = x as usize;
        let y_usize = y as usize;
        let width_usize = (width as usize).min(full_width.saturating_sub(x_usize));
        let height_usize = (height as usize).min(full_height.saturating_sub(y_usize));

        if width_usize == 0 || height_usize == 0 {
            return Ok(Vec::new());
        }

        let mut region = vec![0u8; width_usize * height_usize * 3];
        for row in 0..height_usize {
            let src_row = y_usize + row;
            if src_row >= full_height {
                break;
            }
            let src_start = (src_row * full_width + x_usize) * 3;
            let dst_start = row * width_usize * 3;
            let copy_len = width_usize * 3;
            let src_end = (src_start + copy_len).min(full_rgb.len());
            if src_start < full_rgb.len() {
                let actual_copy = src_end - src_start;
                region[dst_start..dst_start + actual_copy]
                    .copy_from_slice(&full_rgb[src_start..src_end]);
            }
        }

        Ok(region)
    }

    /// Decode a region at a specific resolution level
    ///
    /// JPEG2000 supports multi-resolution decoding through wavelet decomposition levels.
    /// Resolution level 0 is the full resolution, level 1 is half resolution, etc.
    ///
    /// # Arguments
    ///
    /// * `x` - Left coordinate at target resolution (pixels)
    /// * `y` - Top coordinate at target resolution (pixels)
    /// * `width` - Width at target resolution (pixels)
    /// * `height` - Height at target resolution (pixels)
    /// * `resolution_level` - Resolution level (0 = full, 1 = half, 2 = quarter, etc.)
    ///
    /// # Returns
    ///
    /// RGB image data for the specified region at the specified resolution
    pub fn decode_region_at_resolution(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        resolution_level: u8,
    ) -> Result<Vec<u8>> {
        let max_levels = self.num_decomposition_levels();

        if resolution_level > max_levels {
            return Err(Jpeg2000Error::InvalidDimension(format!(
                "Resolution level {} exceeds maximum decomposition levels {}",
                resolution_level, max_levels
            )));
        }

        let scale_factor = 1u32 << resolution_level;
        let full_res_x = x * scale_factor;
        let full_res_y = y * scale_factor;
        let full_res_width = width * scale_factor;
        let full_res_height = height * scale_factor;

        let image_width = self.width()?;
        let image_height = self.height()?;

        if full_res_x + full_res_width > image_width || full_res_y + full_res_height > image_height
        {
            return Err(Jpeg2000Error::InvalidDimension(format!(
                "Scaled region ({}x{} at {},{}) exceeds image bounds ({}x{})",
                full_res_width, full_res_height, full_res_x, full_res_y, image_width, image_height
            )));
        }

        tracing::info!(
            "Decoding region {}x{} at ({},{}) with resolution level {} (scale 1/{})",
            width,
            height,
            x,
            y,
            resolution_level,
            scale_factor
        );

        self.decode_region(x, y, width, height)
    }

    /// Compute which tiles intersect with a given region
    fn compute_intersecting_tiles(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<Vec<(u32, u32)>> {
        let image_size = self.image_size.as_ref().ok_or_else(|| {
            Jpeg2000Error::InvalidImageHeader("Image size not available".to_string())
        })?;

        let tile_width = image_size.tile_width;
        let tile_height = image_size.tile_height;
        let tile_x_offset = image_size.tile_x_offset;
        let tile_y_offset = image_size.tile_y_offset;

        // Calculate tile range
        let start_tile_x = if x >= tile_x_offset {
            (x - tile_x_offset) / tile_width
        } else {
            0
        };

        let start_tile_y = if y >= tile_y_offset {
            (y - tile_y_offset) / tile_height
        } else {
            0
        };

        let end_tile_x = if x + width >= tile_x_offset {
            ((x + width - 1 - tile_x_offset) / tile_width).min(image_size.num_tiles_x() - 1)
        } else {
            0
        };

        let end_tile_y = if y + height >= tile_y_offset {
            ((y + height - 1 - tile_y_offset) / tile_height).min(image_size.num_tiles_y() - 1)
        } else {
            0
        };

        let mut tiles = Vec::new();
        for ty in start_tile_y..=end_tile_y {
            for tx in start_tile_x..=end_tile_x {
                tiles.push((tx, ty));
            }
        }

        Ok(tiles)
    }

    /// Decode region using tile indices
    ///
    /// This is a lower-level method that decodes **only** the explicitly listed
    /// tiles (a real partial-decode benefit) and composites them into the output
    /// raster, then crops out the requested region. Pixels of the region that
    /// fall outside the supplied tiles are left as neutral gray (128).
    pub fn decode_region_from_tiles(
        &mut self,
        tiles: &[(u32, u32)],
        region_x: u32,
        region_y: u32,
        region_width: u32,
        region_height: u32,
    ) -> Result<Vec<u8>> {
        if tiles.is_empty() {
            return Err(Jpeg2000Error::InvalidTile(
                "decode_region_from_tiles called with an empty tile list".to_string(),
            ));
        }

        let image_width = self.width()?;
        let image_height = self.height()?;
        if region_x + region_width > image_width || region_y + region_height > image_height {
            return Err(Jpeg2000Error::InvalidDimension(format!(
                "Region {}x{} at ({},{}) exceeds image bounds {}x{}",
                region_width, region_height, region_x, region_y, image_width, image_height
            )));
        }

        if self.raw_codestream.is_none() {
            return Err(Jpeg2000Error::CodestreamError(
                "decode_region_from_tiles called before the codestream was parsed".to_string(),
            ));
        }

        tracing::info!(
            "Decoding {} explicit tile(s) for region {}x{} at ({},{})",
            tiles.len(),
            region_width,
            region_height,
            region_x,
            region_y
        );

        let (num_tiles_x, num_tiles_y, tile_width, tile_height, tile_x_offset, tile_y_offset) = {
            let s = self
                .image_size
                .as_ref()
                .ok_or_else(|| Jpeg2000Error::InvalidImageHeader("No image size".to_string()))?;
            (
                s.num_tiles_x(),
                s.num_tiles_y(),
                s.tile_width as usize,
                s.tile_height as usize,
                s.tile_x_offset as usize,
                s.tile_y_offset as usize,
            )
        };

        let precision = self
            .image_size
            .as_ref()
            .and_then(|s| s.components.first())
            .map(|c| c.precision)
            .unwrap_or(8);
        let is_signed = self
            .image_size
            .as_ref()
            .and_then(|s| s.components.first())
            .map(|c| c.is_signed)
            .unwrap_or(false);
        let num_components = self.num_components()? as usize;

        let full_width = image_width as usize;
        let full_height = image_height as usize;
        let mut canvas = vec![128u8; full_width * full_height * 3];

        for &(tx, ty) in tiles {
            if tx >= num_tiles_x || ty >= num_tiles_y {
                return Err(Jpeg2000Error::InvalidTile(format!(
                    "Tile ({}, {}) out of bounds ({}x{} tiles)",
                    tx, ty, num_tiles_x, num_tiles_y
                )));
            }
            let component_samples = self.decode_tile_to_components(tx, ty)?;
            let shifted: Vec<Vec<u8>> = component_samples
                .iter()
                .map(|comp| crate::color::level_shift(comp, precision, is_signed))
                .collect();

            let ox = (tile_x_offset + tx as usize * tile_width).min(full_width);
            let oy = (tile_y_offset + ty as usize * tile_height).min(full_height);
            let tx1 = (tile_x_offset + (tx as usize + 1) * tile_width).min(full_width);
            let ty1 = (tile_y_offset + (ty as usize + 1) * tile_height).min(full_height);

            place_tile_samples(
                &mut canvas,
                full_width,
                &shifted,
                num_components,
                tile_width,
                ox,
                oy,
                tx1,
                ty1,
            );
        }

        // Crop the requested region out of the composited canvas.
        let x_usize = region_x as usize;
        let y_usize = region_y as usize;
        let width_usize = (region_width as usize).min(full_width.saturating_sub(x_usize));
        let height_usize = (region_height as usize).min(full_height.saturating_sub(y_usize));
        if width_usize == 0 || height_usize == 0 {
            return Ok(Vec::new());
        }

        let mut region = vec![0u8; width_usize * height_usize * 3];
        for row in 0..height_usize {
            let src_row = y_usize + row;
            let src_start = (src_row * full_width + x_usize) * 3;
            let dst_start = row * width_usize * 3;
            let copy_len = width_usize * 3;
            let src_end = (src_start + copy_len).min(canvas.len());
            if src_start < canvas.len() {
                let actual_copy = src_end - src_start;
                region[dst_start..dst_start + actual_copy]
                    .copy_from_slice(&canvas[src_start..src_end]);
            }
        }

        Ok(region)
    }
}

/// Composite one decoded tile's level-shifted component samples into the RGB
/// output raster.
///
/// `canvas` is the full-image RGB buffer (`out_w` pixels wide). The tile
/// occupies output pixels `[ox, tx1) × [oy, ty1)`; within the tile's own sample
/// buffers (`shifted`, one per component, each `tile_width`-strided) the local
/// coordinate is `(px - ox, py - oy)`.
#[allow(clippy::too_many_arguments)]
fn place_tile_samples(
    canvas: &mut [u8],
    out_w: usize,
    shifted: &[Vec<u8>],
    num_components: usize,
    tile_width: usize,
    ox: usize,
    oy: usize,
    tx1: usize,
    ty1: usize,
) {
    for py in oy..ty1 {
        for px in ox..tx1 {
            let local_idx = (py - oy) * tile_width + (px - ox);
            let dst = (py * out_w + px) * 3;
            if dst + 2 >= canvas.len() {
                continue;
            }
            if num_components >= 3 && shifted.len() >= 3 {
                canvas[dst] = shifted[0].get(local_idx).copied().unwrap_or(128);
                canvas[dst + 1] = shifted[1].get(local_idx).copied().unwrap_or(128);
                canvas[dst + 2] = shifted[2].get(local_idx).copied().unwrap_or(128);
            } else if !shifted.is_empty() {
                let gray = shifted[0].get(local_idx).copied().unwrap_or(128);
                canvas[dst] = gray;
                canvas[dst + 1] = gray;
                canvas[dst + 2] = gray;
            }
        }
    }
}

/// Progressive decoder iterator
///
/// Yields increasingly refined image data as quality layers are decoded.
pub struct ProgressiveDecoder<'a, R> {
    reader: &'a mut Jpeg2000Reader<R>,
    current_layer: u16,
    max_layers: u16,
}

impl<'a, R: Read + Seek> ProgressiveDecoder<'a, R> {
    /// Get next quality layer
    pub fn next_layer(&mut self) -> Result<Option<Vec<u8>>> {
        if self.current_layer >= self.max_layers {
            return Ok(None);
        }

        let data = self.reader.decode_quality_layers(self.current_layer)?;
        self.current_layer += 1;

        Ok(Some(data))
    }

    /// Get current layer index
    pub fn current_layer(&self) -> u16 {
        self.current_layer
    }

    /// Get total number of layers
    pub fn total_layers(&self) -> u16 {
        self.max_layers
    }

    /// Get progress as percentage (0.0 - 1.0)
    pub fn progress(&self) -> f64 {
        if self.max_layers == 0 {
            1.0
        } else {
            f64::from(self.current_layer) / f64::from(self.max_layers)
        }
    }

    /// Check if decoding is complete
    pub fn is_complete(&self) -> bool {
        self.current_layer >= self.max_layers
    }

    /// Skip to specific layer
    pub fn skip_to_layer(&mut self, layer: u16) -> Result<Vec<u8>> {
        if layer >= self.max_layers {
            return Err(Jpeg2000Error::Tier2Error(format!(
                "Layer {} exceeds maximum {}",
                layer, self.max_layers
            )));
        }

        self.current_layer = layer;
        self.reader.decode_quality_layers(layer)
    }
}

/// Image information
#[derive(Debug, Clone)]
pub struct ImageInfo {
    /// Image width
    pub width: u32,
    /// Image height
    pub height: u32,
    /// Number of components
    pub num_components: u16,
    /// Number of tiles
    pub num_tiles: u32,
    /// Color space
    pub color_space: Option<EnumeratedColorSpace>,
    /// Number of wavelet decomposition levels
    pub num_decomposition_levels: u8,
    /// Is JP2 format (vs raw codestream)
    pub is_jp2: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a minimal valid J2K codestream: SOC + SIZ + COD + QCD + SOT + SOD + EOC
    /// 4×4 grayscale, 1 decomposition level, 1 code-block, 0 packet data.
    fn build_minimal_j2k_4x4_grayscale() -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();

        // SOC: 0xFF4F
        out.extend_from_slice(&[0xFF, 0x4F]);

        // SIZ marker: 0xFF51
        // Lsiz = 2 + 2 + 4 + 4 + 4 + 4 + 4 + 4 + 4 + 4 + 2 + (1×3) = 41 bytes
        // But the length field includes itself (2 bytes), so Lsiz = 41
        // Fields: Rsiz(2) + Xsiz(4) + Ysiz(4) + XOsiz(4) + YOsiz(4)
        //       + XTsiz(4) + YTsiz(4) + XTOsiz(4) + YTOsiz(4) + Csiz(2)
        //       + Ssiz(1) + XRsiz(1) + YRsiz(1)  → 2+8×4+2+3 = 39 data bytes
        //       Lsiz = 39 + 2 = 41
        out.extend_from_slice(&[0xFF, 0x51]);
        out.extend_from_slice(&[0x00, 0x29]); // Lsiz = 41
        out.extend_from_slice(&[0x00, 0x00]); // Rsiz
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // Xsiz = 4
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // Ysiz = 4
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // XOsiz = 0
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // YOsiz = 0
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // XTsiz = 4
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // YTsiz = 4
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // XTOsiz = 0
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // YTOsiz = 0
        out.extend_from_slice(&[0x00, 0x01]); // Csiz = 1
        out.push(0x07); // Ssiz: signed=0, precision-1=7 → 8-bit unsigned
        out.push(0x01); // XRsiz = 1
        out.push(0x01); // YRsiz = 1

        // COD marker: 0xFF52
        // Lcod = 2 + 1+1+2+1+1+1+1+1+1 = 12 → Lcod = 12
        out.extend_from_slice(&[0xFF, 0x52]);
        out.extend_from_slice(&[0x00, 0x0C]); // Lcod = 12
        out.push(0x00); // Scod: no precincts, no SOT markers, no EPH
        out.push(0x00); // progression order = LRCP
        out.extend_from_slice(&[0x00, 0x01]); // num_layers = 1
        out.push(0x00); // mct = 0
        out.push(0x00); // num_levels = 0 (no decomposition)
        out.push(0x02); // xcb = 2 → code-block width = 1<<(2+2) = 16, but tile is 4x4
        out.push(0x02); // ycb = 2 → code-block height = 16, clamped to 4
        out.push(0x00); // code-block style
        out.push(0x01); // wavelet = 1 → 5/3 reversible

        // QCD marker: 0xFF5C — reversible quantization, no quantization (style=0)
        // Lqcd = 2 + 1 + num_steps
        // With style=0 (no quantization), 1 step size byte needed for 1 subband
        out.extend_from_slice(&[0xFF, 0x5C]);
        out.extend_from_slice(&[0x00, 0x04]); // Lqcd = 4
        out.push(0x00); // Sqcd = 0 (no quantization)
        out.push(0x00); // step size for LL subband

        // SOT marker: 0xFF90
        // Lsot = 10 (fixed), Isot = 0, Psot = 0 (unknown), TPsot = 0, TNsot = 1
        out.extend_from_slice(&[0xFF, 0x90]);
        out.extend_from_slice(&[0x00, 0x0A]); // Lsot = 10
        out.extend_from_slice(&[0x00, 0x00]); // Isot = 0
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Psot = 0 (unknown)
        out.push(0x00); // TPsot = 0
        out.push(0x01); // TNsot = 1

        // SOD: 0xFF93 — no packet data follows
        out.extend_from_slice(&[0xFF, 0x93]);

        // EOC: 0xFFD9
        out.extend_from_slice(&[0xFF, 0xD9]);

        out
    }

    #[test]
    fn test_decode_rgb_minimal_j2k_empty_sod() {
        let data = build_minimal_j2k_4x4_grayscale();
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // parse_headers will parse the J2K markers
        reader.parse_headers().expect("parse_headers failed");

        // Width/height should be 4×4 from SIZ
        assert_eq!(reader.width().expect("width"), 4);
        assert_eq!(reader.height().expect("height"), 4);
        assert_eq!(reader.num_components().expect("num_components"), 1);

        // raw_codestream should now be stored
        assert!(reader.raw_codestream.is_some());

        // decode_rgb: with zero coefficients and no wavelet levels,
        // level_shift(0, 8, false) = 0, so all output pixels should be 0
        // (unsigned, shift=0 → 0 clamped to [0, 255] → 0)
        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(rgb.len(), 4 * 4 * 3);
        // All pixels are gray-equivalent (all channel equal)
        for i in 0..(4 * 4) {
            assert_eq!(rgb[i * 3], rgb[i * 3 + 1], "R != G at pixel {}", i);
            assert_eq!(rgb[i * 3 + 1], rgb[i * 3 + 2], "G != B at pixel {}", i);
        }
    }

    #[test]
    fn test_decode_quality_layers_returns_real_pixels_not_flat_gray() {
        // Regression: decode_quality_layers() used to ignore the codestream and
        // fill the buffer with a flat gray value = 128 * (max_layer+1)/num_layers.
        // It must now return real decoded pixel data.
        let data = build_minimal_j2k_4x4_grayscale();
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        let layered = reader
            .decode_quality_layers(0)
            .expect("decode_quality_layers failed");
        assert_eq!(layered.len(), 4 * 4 * 3);

        // The old stub would have produced all-128 pixels; the real decode of an
        // all-zero-coefficient stream yields all-zero pixels.
        assert!(
            layered.iter().all(|&p| p == 0),
            "decode_quality_layers must return real decoded pixels, not the flat-gray stub"
        );

        // Progressive output must agree with the non-progressive decode path.
        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(layered, rgb, "progressive and full decode must agree");

        // Progressive state must be recorded for the requested layer, and the
        // stored buffer must match what was returned.
        assert_eq!(reader.progressive_layer(), Some(0));
        assert_eq!(reader.progressive_data(), Some(layered.as_slice()));
    }

    #[test]
    fn test_decode_quality_layers_rejects_out_of_range_layer() {
        let data = build_minimal_j2k_4x4_grayscale();
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        // Only one quality layer exists; requesting layer 1 must error, not decode.
        assert!(reader.decode_quality_layers(1).is_err());
    }

    #[test]
    fn test_decode_rgb_with_nonempty_packet_data_is_resilient() {
        // Regression for the naive even-division demux + error-resilient tier-1
        // fallback: a tile that actually carries packet bytes must decode without
        // panicking and yield a correctly sized buffer.
        let mut data = build_minimal_j2k_4x4_grayscale();
        // Insert non-empty "packet" bytes just before the trailing EOC (0xFF 0xD9).
        let eoc_pos = data.len() - 2;
        let packet_bytes = [0x80u8, 0x40, 0x55, 0x00, 0x12, 0x34];
        data.splice(eoc_pos..eoc_pos, packet_bytes.iter().copied());

        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(rgb.len(), 4 * 4 * 3);
    }

    /// Build a two-tile (2×1) raw J2K codestream. Image is 8×4 with 4×4 tiles,
    /// so tile 0 covers x∈[0,4) and tile 1 covers x∈[4,8). Each tile-part carries
    /// a distinctive, correctly `Psot`-bounded SOD payload so bleed across tile
    /// boundaries is detectable.
    fn build_two_tile_j2k(tile0_data: &[u8], tile1_data: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&[0xFF, 0x4F]); // SOC

        // SIZ (Lsiz = 41), 8×4 image, 4×4 tiles, 1 component.
        out.extend_from_slice(&[0xFF, 0x51]);
        out.extend_from_slice(&[0x00, 0x29]);
        out.extend_from_slice(&[0x00, 0x00]); // Rsiz
        out.extend_from_slice(&8u32.to_be_bytes()); // Xsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // Ysiz
        out.extend_from_slice(&0u32.to_be_bytes()); // XOsiz
        out.extend_from_slice(&0u32.to_be_bytes()); // YOsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // XTsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // YTsiz
        out.extend_from_slice(&0u32.to_be_bytes()); // XTOsiz
        out.extend_from_slice(&0u32.to_be_bytes()); // YTOsiz
        out.extend_from_slice(&1u16.to_be_bytes()); // Csiz
        out.push(0x07); // Ssiz: 8-bit unsigned
        out.push(0x01); // XRsiz
        out.push(0x01); // YRsiz

        // COD (Lcod = 12), LRCP, 1 layer, no levels, 5/3.
        out.extend_from_slice(&[0xFF, 0x52]);
        out.extend_from_slice(&[0x00, 0x0C]);
        out.push(0x00);
        out.push(0x00);
        out.extend_from_slice(&1u16.to_be_bytes());
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.push(0x01);

        // QCD (Lqcd = 4), no quantization.
        out.extend_from_slice(&[0xFF, 0x5C]);
        out.extend_from_slice(&[0x00, 0x04]);
        out.push(0x00);
        out.push(0x00);

        // Tile-part 0: SOT(12) + SOD(2) + data. Psot = 14 + data.len().
        out.extend_from_slice(&[0xFF, 0x90]);
        out.extend_from_slice(&10u16.to_be_bytes()); // Lsot
        out.extend_from_slice(&0u16.to_be_bytes()); // Isot = 0
        out.extend_from_slice(&((14 + tile0_data.len()) as u32).to_be_bytes()); // Psot
        out.push(0x00); // TPsot
        out.push(0x01); // TNsot
        out.extend_from_slice(&[0xFF, 0x93]); // SOD
        out.extend_from_slice(tile0_data);

        // Tile-part 1.
        out.extend_from_slice(&[0xFF, 0x90]);
        out.extend_from_slice(&10u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes()); // Isot = 1
        out.extend_from_slice(&((14 + tile1_data.len()) as u32).to_be_bytes()); // Psot
        out.push(0x00);
        out.push(0x01);
        out.extend_from_slice(&[0xFF, 0x93]); // SOD
        out.extend_from_slice(tile1_data);

        out.extend_from_slice(&[0xFF, 0xD9]); // EOC
        out
    }

    #[test]
    fn test_find_tile_bitstream_bounds_per_tile() {
        // Regression: each tile's returned bitstream must be bounded by its own
        // Psot, never bleed to the end of the whole codestream.
        let tile0 = [0xAAu8, 0xBB, 0xCC];
        let tile1 = [0x11u8, 0x22];
        let data = build_two_tile_j2k(&tile0, &tile1);
        let mut reader = Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        assert_eq!(reader.image_size.as_ref().map(|s| s.num_tiles()), Some(2));

        let b0 = reader.find_tile_bitstream(0).expect("tile 0 bitstream");
        let b1 = reader.find_tile_bitstream(1).expect("tile 1 bitstream");
        assert_eq!(b0, tile0, "tile 0 must not include tile 1's bytes");
        assert_eq!(b1, tile1, "tile 1 must return exactly its own bytes");
    }

    #[test]
    fn test_decode_rgb_two_tiles_full_size() {
        // Multi-tile decode must compose the full 8×4 raster (not a single tile).
        let data = build_two_tile_j2k(&[], &[]);
        let mut reader = Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");
        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(rgb.len(), 8 * 4 * 3);
    }

    #[test]
    fn test_decode_rgb_without_codestream_errors_not_gray() {
        // Regression: decode must never fabricate a flat-gray Ok when the
        // codestream was never parsed.
        let mut reader = Jpeg2000Reader::new(Cursor::new(vec![
            0xFF, 0x4F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]))
        .expect("reader creation failed");
        reader.image_size = Some(ImageSize {
            width: 8,
            height: 8,
            x_offset: 0,
            y_offset: 0,
            tile_width: 8,
            tile_height: 8,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 1,
            components: vec![],
        });
        assert!(matches!(
            reader.decode_rgb(),
            Err(Jpeg2000Error::CodestreamError(_))
        ));
    }

    /// Find the byte offset of a two-byte marker in a codestream.
    fn find_marker(data: &[u8], marker: u16) -> Option<usize> {
        let hi = (marker >> 8) as u8;
        let lo = marker as u8;
        data.windows(2).position(|w| w[0] == hi && w[1] == lo)
    }

    #[test]
    fn test_decode_tile_real_packet_matches_tier1() {
        // The core Tier-2 wiring assertion: a hand-built codestream carrying one
        // real packet (single included code block) must have its body bytes
        // sliced exactly by the packet parser and fed to Tier-1 — the decoded
        // coefficients must match a *direct* Tier-1 decode of those same bytes.
        use crate::tier1::{CodeBlockDecoder, SubbandType};
        use crate::tier2::layout::code_block_bitplanes;

        let body: [u8; 5] = [0x95, 0x40, 0x22, 0x0C, 0x71];
        // Packet header for a single included code block (1x1 grid):
        //   present=1, inclusion=1, zbp-terminator=1, num_passes(=1)=0,
        //   Lblock comma=0, length(=5)=0b101  =>  0b1110_0101 = 0xE5.
        let mut packet = vec![0xE5u8];
        packet.extend_from_slice(&body);

        let mut data = build_minimal_j2k_4x4_grayscale();
        let eoc = data.len() - 2; // splice packet data in just before the EOC
        data.splice(eoc..eoc, packet.iter().copied());

        let mut reader = Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        let comps = reader
            .decode_tile_to_components(0, 0)
            .expect("tile decode failed");
        assert_eq!(comps.len(), 1);

        // Parameters the reader derives from this stream: guard=0, exponent=0,
        // precision=8, zbp=0 => 9 bit-planes; 4x4 LL code block.
        let num_bitplanes = code_block_bitplanes(0, 0, 8, 0);
        let reference = CodeBlockDecoder::with_subband(4, 4, num_bitplanes, SubbandType::Ll)
            .decode(&body)
            .expect("reference tier-1 decode failed");

        assert_eq!(
            comps[0], reference,
            "Tier-2 must slice exactly the packet body bytes and feed them to Tier-1"
        );
        // The routed bytes must actually influence the output (not a no-op).
        assert_ne!(
            reference,
            vec![0i32; 16],
            "chosen body should decode to some non-zero coefficients"
        );

        // The full RGB path must also succeed and be correctly sized.
        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(rgb.len(), 4 * 4 * 3);
    }

    #[test]
    fn test_decode_multi_layer_codestream_rejected() {
        // Multi-layer streams must fail loud (typed error), never mis-slice.
        let mut data = build_minimal_j2k_4x4_grayscale();
        let cod = find_marker(&data, 0xFF52).expect("COD marker present");
        // num_layers is 2 bytes after COD marker + Lcod + Scod + progression.
        data[cod + 6] = 0x00;
        data[cod + 7] = 0x02;

        let mut reader = Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");
        assert_eq!(reader.num_quality_layers(), 2);

        let result = reader.decode_rgb();
        assert!(
            matches!(result, Err(Jpeg2000Error::UnsupportedFeature(_))),
            "multi-layer decode must return UnsupportedFeature, got {:?}",
            result.map(|v| v.len())
        );
    }

    #[test]
    fn test_decode_all_progression_orders_single_layer() {
        // Every progression order must decode for a single-layer stream (they
        // share the ProgressionIterator); none should spuriously reject.
        for order in 0u8..=4 {
            let mut data = build_minimal_j2k_4x4_grayscale();
            let cod = find_marker(&data, 0xFF52).expect("COD marker present");
            // Progression order is 1 byte after COD marker + Lcod + Scod.
            data[cod + 5] = order;

            let mut reader =
                Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
            reader.parse_headers().expect("parse_headers failed");

            let rgb = reader
                .decode_rgb()
                .unwrap_or_else(|e| panic!("progression {order} must decode, got {e:?}"));
            assert_eq!(rgb.len(), 4 * 4 * 3);
        }
    }

    #[test]
    fn test_reader_creation() {
        // Create minimal JP2 signature
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, // Box length
            0x6A, 0x50, 0x20, 0x20, // 'jP  '
            0x0D, 0x0A, 0x87, 0x0A, // Signature
        ];

        let cursor = Cursor::new(data);
        let result = Jpeg2000Reader::new(cursor);
        assert!(result.is_ok());

        let reader = result.expect("reader failed");
        assert!(reader.is_jp2);
    }

    #[test]
    fn test_j2k_detection() {
        // Create minimal J2K codestream (SOC marker + padding to 12 bytes for detection)
        let data = vec![
            0xFF, 0x4F, // SOC marker
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // padding
        ];

        let cursor = Cursor::new(data);
        let result = Jpeg2000Reader::new(cursor);
        assert!(result.is_ok());

        let reader = result.expect("reader failed");
        assert!(!reader.is_jp2);
    }

    #[test]
    fn test_resilience_mode_default() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        assert_eq!(reader.resilience_mode(), ResilienceMode::None);
        assert!(!reader.resilience_mode().is_enabled());
    }

    #[test]
    fn test_resilience_mode_configuration() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Test basic resilience
        reader.enable_error_resilience();
        assert_eq!(reader.resilience_mode(), ResilienceMode::Basic);
        assert!(reader.resilience_mode().is_enabled());

        // Test full resilience
        reader.enable_full_error_resilience();
        assert_eq!(reader.resilience_mode(), ResilienceMode::Full);
        assert!(reader.resilience_mode().is_full());

        // Test disable
        reader.disable_error_resilience();
        assert_eq!(reader.resilience_mode(), ResilienceMode::None);
    }

    #[test]
    fn test_progressive_state_initialization() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        assert!(!reader.is_progressive_active());
        assert!(reader.progressive_layer().is_none());
    }

    #[test]
    fn test_progressive_state_reset() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Initialize state by setting it manually
        reader.progressive_state = Some(ProgressiveDecodingState {
            current_layer: 2,
            max_layers: 5,
            intermediate_data: vec![],
            width: 256,
            height: 256,
        });

        assert!(reader.is_progressive_active());
        assert_eq!(reader.progressive_layer(), Some(2));

        // Reset state
        reader.reset_progressive_state();
        assert!(!reader.is_progressive_active());
        assert!(reader.progressive_layer().is_none());
    }

    #[test]
    fn test_region_bounds_validation() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Set up minimal image size
        reader.image_size = Some(ImageSize {
            width: 256,
            height: 256,
            x_offset: 0,
            y_offset: 0,
            tile_width: 256,
            tile_height: 256,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 3,
            components: vec![],
        });

        // An in-bounds region no longer fabricates placeholder pixels: without a
        // parsed codestream, decoding must return a typed error, never fake data.
        let result = reader.decode_region(0, 0, 128, 128);
        assert!(matches!(result, Err(Jpeg2000Error::CodestreamError(_))));

        // Region exceeding width should fail at bounds validation
        let result = reader.decode_region(200, 0, 100, 128);
        assert!(result.is_err());

        // Region exceeding height should fail at bounds validation
        let result = reader.decode_region(0, 200, 128, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_intersecting_tiles() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Set up image with multiple tiles
        reader.image_size = Some(ImageSize {
            width: 512,
            height: 512,
            x_offset: 0,
            y_offset: 0,
            tile_width: 128,
            tile_height: 128,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 3,
            components: vec![],
        });

        // Region in first tile only
        let tiles = reader.compute_intersecting_tiles(0, 0, 64, 64);
        assert!(tiles.is_ok());
        let tiles = tiles.expect("tiles");
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0], (0, 0));

        // Region spanning multiple tiles
        let tiles = reader.compute_intersecting_tiles(64, 64, 128, 128);
        assert!(tiles.is_ok());
        let tiles = tiles.expect("tiles");
        assert!(!tiles.is_empty());

        // Region covering entire image
        let tiles = reader.compute_intersecting_tiles(0, 0, 512, 512);
        assert!(tiles.is_ok());
        let tiles = tiles.expect("tiles");
        assert_eq!(tiles.len(), 16); // 4x4 tiles
    }

    #[test]
    fn test_resolution_level_scaling() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Set up image
        reader.image_size = Some(ImageSize {
            width: 256,
            height: 256,
            x_offset: 0,
            y_offset: 0,
            tile_width: 256,
            tile_height: 256,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 3,
            components: vec![],
        });

        // Set coding style with decomposition levels
        reader.coding_style = Some(CodingStyle {
            progression_order: crate::codestream::ProgressionOrder::Lrcp,
            num_layers: 5,
            use_mct: true,
            num_levels: 3,
            code_block_width: 64,
            code_block_height: 64,
            code_block_style: 0,
            wavelet: crate::codestream::WaveletTransform::Reversible53,
            has_sop: false,
            has_eph: false,
        });

        // Full/half resolution decodes without a parsed codestream must error
        // (no fabricated placeholder), while the scale-factor bounds math still
        // runs first.
        assert!(
            reader
                .decode_region_at_resolution(0, 0, 128, 128, 0)
                .is_err()
        );
        assert!(reader.decode_region_at_resolution(0, 0, 64, 64, 1).is_err());

        // Invalid resolution level should fail at validation
        let result = reader.decode_region_at_resolution(0, 0, 64, 64, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_metadata_accessors() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Initially, all metadata should be None
        assert!(reader.file_type().is_none());
        assert!(reader.image_header().is_none());
        assert!(reader.color_specification().is_none());
        assert!(reader.capture_resolution().is_none());
        assert!(reader.display_resolution().is_none());
        assert!(reader.xml_metadata().is_empty());
        assert!(reader.uuid_boxes().is_empty());
    }

    #[test]
    fn test_quality_layer_accessors() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Default should be 1 layer
        assert_eq!(reader.num_quality_layers(), 1);

        // Set coding style with multiple layers
        reader.coding_style = Some(CodingStyle {
            progression_order: crate::codestream::ProgressionOrder::Lrcp,
            num_layers: 10,
            use_mct: false,
            num_levels: 5,
            code_block_width: 64,
            code_block_height: 64,
            code_block_style: 0,
            wavelet: crate::codestream::WaveletTransform::Reversible53,
            has_sop: false,
            has_eph: false,
        });

        assert_eq!(reader.num_quality_layers(), 10);
        assert_eq!(reader.num_decomposition_levels(), 5);
        assert!(!reader.uses_mct());
    }

    #[test]
    fn test_progressive_decoder_iterator() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Set up minimal configuration
        reader.image_size = Some(ImageSize {
            width: 64,
            height: 64,
            x_offset: 0,
            y_offset: 0,
            tile_width: 64,
            tile_height: 64,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 3,
            components: vec![],
        });

        reader.coding_style = Some(CodingStyle {
            progression_order: crate::codestream::ProgressionOrder::Lrcp,
            num_layers: 3,
            use_mct: false,
            num_levels: 2,
            code_block_width: 32,
            code_block_height: 32,
            code_block_style: 0,
            wavelet: crate::codestream::WaveletTransform::Reversible53,
            has_sop: false,
            has_eph: false,
        });

        let decoder = reader.decode_progressive().expect("decoder");

        assert_eq!(decoder.total_layers(), 3);
        assert_eq!(decoder.current_layer(), 0);
        assert!(!decoder.is_complete());
        assert_eq!(decoder.progress(), 0.0);
    }
}
