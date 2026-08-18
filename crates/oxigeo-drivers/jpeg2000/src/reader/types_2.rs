//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::box_reader::{BoxReader, BoxType};
use crate::codestream::{
    CodestreamParser, CodingStyle, ImageSize, Marker, Quantization, WaveletTransform,
};
use crate::color::ColorConverter;
use crate::error::{Jpeg2000Error, ResilienceMode, Result};
use crate::metadata::Jp2Metadata;
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor as IoCursor, Read, Seek, SeekFrom};

use super::functions::place_tile_samples;
use super::types::ProgressiveDecoder;
use super::types_3::{ImageInfo, ProgressiveDecodingState};

/// JPEG2000 reader
pub struct Jpeg2000Reader<R> {
    /// Input reader
    pub(super) reader: R,
    /// JP2 metadata
    pub(super) metadata: Option<Jp2Metadata>,
    /// Codestream image size
    pub(super) image_size: Option<ImageSize>,
    /// Coding style
    pub(super) coding_style: Option<CodingStyle>,
    /// Quantization
    pub(super) quantization: Option<Quantization>,
    /// Is JP2 format (vs raw codestream)
    pub(super) is_jp2: bool,
    /// Error resilience mode
    pub(super) resilience_mode: ResilienceMode,
    /// Progressive decoding state
    pub(super) progressive_state: Option<ProgressiveDecodingState>,
    /// Raw codestream bytes (stored after parsing for decode use)
    pub(super) raw_codestream: Option<Vec<u8>>,
}
impl<R: Read + Seek> Jpeg2000Reader<R> {
    /// Create new JPEG2000 reader
    pub fn new(mut reader: R) -> Result<Self> {
        let mut magic = [0u8; 12];
        let is_jp2 = match reader.read_exact(&mut magic) {
            Ok(()) => {
                reader.seek(SeekFrom::Start(0))?;
                magic[4..8] == *b"jP  "
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                reader.seek(SeekFrom::Start(0))?;
                let mut min_magic = [0u8; 2];
                match reader.read_exact(&mut min_magic) {
                    Ok(()) => {
                        reader.seek(SeekFrom::Start(0))?;
                        false
                    }
                    Err(_) => {
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
    pub(super) fn parse_jp2_headers(&mut self) -> Result<()> {
        self.metadata = Some(Jp2Metadata::parse(&mut self.reader)?);
        self.parse_optional_boxes()?;
        let mut box_reader = BoxReader::new(&mut self.reader)?;
        if let Some(jp2c_header) = box_reader.find_box(BoxType::ContiguousCodestream)? {
            let codestream_data = box_reader.read_box_data(&jp2c_header)?;
            self.raw_codestream = Some(codestream_data.clone());
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
    pub(super) fn parse_optional_boxes(&mut self) -> Result<()> {
        let mut box_reader = BoxReader::new(&mut self.reader)?;
        box_reader.reset()?;
        if let Some(jp2h_header) = box_reader.find_box(BoxType::Jp2Header)? {
            let jp2h_data = box_reader.read_box_data(&jp2h_header)?;
            let mut jp2h_cursor = std::io::Cursor::new(&jp2h_data);
            let mut sub_reader = BoxReader::new(&mut jp2h_cursor)?;
            if let Some(res_header) = sub_reader.find_box(BoxType::Resolution)? {
                let res_data = sub_reader.read_box_data(&res_header)?;
                let mut res_cursor = std::io::Cursor::new(&res_data);
                let mut res_sub_reader = BoxReader::new(&mut res_cursor)?;
                if let Some(resc_header) = res_sub_reader.find_box(BoxType::CaptureResolution)? {
                    let resc_data = res_sub_reader.read_box_data(&resc_header)?;
                    let mut resc_cursor = std::io::Cursor::new(&resc_data);
                    if let Some(ref mut metadata) = self.metadata {
                        metadata.capture_resolution =
                            Some(crate::metadata::Resolution::parse(&mut resc_cursor)?);
                    }
                }
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
    pub(super) fn parse_j2k_headers(&mut self) -> Result<()> {
        let mut codestream_data = Vec::new();
        self.reader.read_to_end(&mut codestream_data)?;
        self.raw_codestream = Some(codestream_data.clone());
        let mut parser = CodestreamParser::new(std::io::Cursor::new(&codestream_data));
        self.parse_codestream(&mut parser)?;
        Ok(())
    }
    /// Parse codestream
    pub(super) fn parse_codestream<CS: Read>(
        &mut self,
        parser: &mut CodestreamParser<CS>,
    ) -> Result<()> {
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
                    break;
                }
                Ok(Some(Marker::Eoc)) => {
                    break;
                }
                Ok(Some(marker)) => {
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
        if self.image_size.is_none() {
            if self.resilience_mode.is_full() {
                tracing::warn!("SIZ marker not found, using error concealment with default size");
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
    pub(super) fn find_tile_bitstream(&self, tile_index: u32) -> Result<Vec<u8>> {
        let codestream = self.raw_codestream.as_ref().ok_or_else(|| {
            Jpeg2000Error::CodestreamError("No raw codestream stored".to_string())
        })?;
        let mut cursor = IoCursor::new(codestream.as_slice());
        let soc = cursor
            .read_u16::<BigEndian>()
            .map_err(|e| Jpeg2000Error::CodestreamError(format!("Read SOC: {}", e)))?;
        if soc != 0xFF4F {
            return Err(Jpeg2000Error::CodestreamError(format!(
                "Expected SOC 0xFF4F, got 0x{:04X}",
                soc
            )));
        }
        loop {
            let marker_val = cursor
                .read_u16::<BigEndian>()
                .map_err(|e| Jpeg2000Error::CodestreamError(format!("Read marker: {}", e)))?;
            match marker_val {
                0xFF90 => {
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
                    let cur_pos = cursor.position() as usize;
                    let sot_start = cur_pos.saturating_sub(usize::from(lsot)).saturating_sub(2);
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
                        return Err(Jpeg2000Error::CodestreamError(format!(
                            "Tile {} not found: encountered a tile-part with unknown \
                             length (Psot=0) for tile {} first",
                            tile_index, isot
                        )));
                    }
                    loop {
                        let inner = cursor.read_u16::<BigEndian>().map_err(|e| {
                            Jpeg2000Error::CodestreamError(format!(
                                "Read tile header marker: {}",
                                e
                            ))
                        })?;
                        match inner {
                            0xFF93 => {
                                let sod_pos = cursor.position() as usize;
                                let mut end = tile_part_end.min(codestream.len());
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
    pub(super) fn decode_tile_to_components(
        &self,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<Vec<Vec<i32>>> {
        let image_size = self
            .image_size
            .as_ref()
            .ok_or_else(|| Jpeg2000Error::InvalidImageHeader("No image size".to_string()))?;
        let coding_style = self
            .coding_style
            .as_ref()
            .ok_or_else(|| Jpeg2000Error::CodestreamError("No coding style".to_string()))?;
        let num_tiles_x = image_size.num_tiles_x();
        let num_tiles_y = image_size.num_tiles_y();
        if tile_x >= num_tiles_x || tile_y >= num_tiles_y {
            return Err(Jpeg2000Error::InvalidTile(format!(
                "Tile ({}, {}) out of bounds ({}x{} tiles)",
                tile_x, tile_y, num_tiles_x, num_tiles_y
            )));
        }
        let tile_index = tile_y * num_tiles_x + tile_x;
        if coding_style.wavelet == WaveletTransform::Irreversible97 {
            return Err(
                Jpeg2000Error::UnsupportedFeature(
                    "9/7 irreversible wavelet decode is not yet implemented; use reversible 5/3 encoding"
                        .to_string(),
                ),
            );
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
        let rgb = self.decode_rgb()?;
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
    pub(super) fn compute_intersecting_tiles(
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
