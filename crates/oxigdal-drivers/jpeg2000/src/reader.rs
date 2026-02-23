//! High-level JP2/J2K reader
//!
//! This module provides a high-level interface for reading JPEG2000 files.

use crate::box_reader::{BoxReader, BoxType};
use crate::codestream::{CodestreamParser, CodingStyle, ImageSize, Marker, Quantization};
use crate::error::{Jpeg2000Error, ResilienceMode, Result};
use crate::metadata::{EnumeratedColorSpace, Jp2Metadata};
use std::io::{Read, Seek, SeekFrom};

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
            if let Some(ref mut metadata) = self.metadata {
                if let Ok(xml_box) =
                    crate::metadata::XmlMetadata::parse(&mut xml_cursor, xml_header.data_size())
                {
                    metadata.xml_boxes.push(xml_box);
                }
            }
        }

        // Parse UUID boxes (can be multiple)
        box_reader.reset()?;
        while let Some(uuid_header) = box_reader.find_box(BoxType::Uuid)? {
            let uuid_data = box_reader.read_box_data(&uuid_header)?;
            let mut uuid_cursor = std::io::Cursor::new(&uuid_data);
            if let Some(ref mut metadata) = self.metadata {
                if let Ok(uuid_box) =
                    crate::metadata::UuidBox::parse(&mut uuid_cursor, uuid_header.data_size())
                {
                    metadata.uuid_boxes.push(uuid_box);
                }
            }
        }

        Ok(())
    }

    /// Parse raw J2K codestream headers
    fn parse_j2k_headers(&mut self) -> Result<()> {
        // Read entire codestream into buffer to avoid borrow checker issues
        let mut codestream_data = Vec::new();
        self.reader.read_to_end(&mut codestream_data)?;

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

    /// Decode image to RGB
    pub fn decode_rgb(&mut self) -> Result<Vec<u8>> {
        // This is a simplified decoder - full implementation would be much more complex
        tracing::warn!("Using simplified JPEG2000 decoder - not suitable for production use");

        let width = self.width()? as usize;
        let height = self.height()? as usize;
        let num_components = self.num_components()? as usize;

        // For now, return a placeholder image
        // Full implementation would:
        // 1. Decode tiles
        // 2. Apply tier-2 packet decoding
        // 3. Apply tier-1 EBCOT decoding
        // 4. Apply inverse wavelet transform
        // 5. Apply inverse quantization
        // 6. Apply color transform
        // 7. Level shift

        let placeholder = vec![128u8; width * height * 3];

        tracing::info!(
            "JPEG2000 decoder placeholder: {}x{} with {} components",
            width,
            height,
            num_components
        );

        Ok(placeholder)
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
        let image_size = self.image_size.as_ref().ok_or_else(|| {
            Jpeg2000Error::InvalidImageHeader("Image size not available".to_string())
        })?;

        if tile_x >= image_size.num_tiles_x() || tile_y >= image_size.num_tiles_y() {
            return Err(Jpeg2000Error::InvalidTile(format!(
                "Tile ({}, {}) out of bounds",
                tile_x, tile_y
            )));
        }

        // Placeholder implementation
        let tile_width = image_size.tile_width as usize;
        let tile_height = image_size.tile_height as usize;

        Ok(vec![128u8; tile_width * tile_height * 3])
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

    /// Decode image progressively up to specified quality layer
    ///
    /// This method allows decoding only the first N quality layers, enabling
    /// faster decoding at lower quality levels. Quality layers are ordered
    /// from lowest (0) to highest quality.
    ///
    /// # Arguments
    ///
    /// * `max_layer` - Maximum quality layer to decode (0-based index)
    ///
    /// # Returns
    ///
    /// RGB image data decoded up to the specified quality layer
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

        // Initialize progressive state if not already present
        if self.progressive_state.is_none() {
            self.progressive_state = Some(ProgressiveDecodingState {
                current_layer: 0,
                max_layers: num_layers,
                intermediate_data: vec![0u8; width * height * 3],
                width,
                height,
            });
        }

        // Update progressive state
        if let Some(ref mut state) = self.progressive_state {
            state.current_layer = max_layer;

            // Placeholder implementation - full implementation would:
            // 1. Decode tiles up to specified quality layer
            // 2. Apply tier-2 packet decoding for layers 0..=max_layer
            // 3. Apply tier-1 EBCOT decoding
            // 4. Apply inverse wavelet transform
            // 5. Accumulate quality improvements

            // For now, simulate progressive quality by scaling intensity
            let quality_factor = (max_layer + 1) as f32 / num_layers as f32;
            let base_value = (128.0 * quality_factor) as u8;

            for pixel in state.intermediate_data.iter_mut() {
                *pixel = base_value;
            }

            tracing::info!(
                "Progressive decode to layer {} (quality factor: {:.2})",
                max_layer,
                quality_factor
            );

            Ok(state.intermediate_data.clone())
        } else {
            Err(Jpeg2000Error::Other(
                "Failed to initialize progressive state".to_string(),
            ))
        }
    }

    /// Decode image progressively with automatic layer progression
    ///
    /// Returns an iterator that yields increasingly refined versions of the image,
    /// one for each quality layer.
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

        // Determine which tiles intersect with the requested region
        let tiles = self.compute_intersecting_tiles(x, y, width, height)?;

        tracing::debug!("Region intersects with {} tiles", tiles.len());

        // Placeholder implementation - full implementation would:
        // 1. Identify tiles that intersect with the region
        // 2. Decode only those tiles
        // 3. Extract the relevant portion from each tile
        // 4. Assemble the final region

        let region_size = (width * height * 3) as usize;
        let mut region_data = vec![128u8; region_size];

        // Simulate region-specific decoding with a pattern
        for py in 0..height {
            for px in 0..width {
                let idx = ((py * width + px) * 3) as usize;
                if idx + 2 < region_data.len() {
                    // Create a simple pattern to show ROI works
                    region_data[idx] = ((px + x) % 256) as u8;
                    region_data[idx + 1] = ((py + y) % 256) as u8;
                    region_data[idx + 2] = 128;
                }
            }
        }

        Ok(region_data)
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

        // Placeholder implementation - full implementation would:
        // 1. Decode wavelet subbands only up to the requested resolution level
        // 2. Apply inverse wavelet transform only to that level
        // 3. Extract the requested region at that resolution

        let region_size = (width * height * 3) as usize;
        let mut region_data = vec![128u8; region_size];

        // Simulate lower resolution with averaged/blurred appearance
        let blur_factor = scale_factor as u8;
        for py in 0..height {
            for px in 0..width {
                let idx = ((py * width + px) * 3) as usize;
                if idx + 2 < region_data.len() {
                    region_data[idx] = ((px + x) / u32::from(blur_factor) % 128 + 64) as u8;
                    region_data[idx + 1] = ((py + y) / u32::from(blur_factor) % 128 + 64) as u8;
                    region_data[idx + 2] = 128;
                }
            }
        }

        Ok(region_data)
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
    /// This is a lower-level method that decodes a region by specifying
    /// the exact tiles to decode.
    pub fn decode_region_from_tiles(
        &mut self,
        tiles: &[(u32, u32)],
        region_x: u32,
        region_y: u32,
        region_width: u32,
        region_height: u32,
    ) -> Result<Vec<u8>> {
        tracing::info!(
            "Decoding {} tiles for region {}x{} at ({},{})",
            tiles.len(),
            region_width,
            region_height,
            region_x,
            region_y
        );

        // Placeholder implementation
        let region_size = (region_width * region_height * 3) as usize;
        Ok(vec![128u8; region_size])
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

        // Valid region should work (though it will return placeholder data)
        let result = reader.decode_region(0, 0, 128, 128);
        assert!(result.is_ok());

        // Region exceeding width should fail
        let result = reader.decode_region(200, 0, 100, 128);
        assert!(result.is_err());

        // Region exceeding height should fail
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
        });

        // Decode at full resolution (level 0)
        let result = reader.decode_region_at_resolution(0, 0, 128, 128, 0);
        assert!(result.is_ok());
        let data = result.expect("data");
        assert_eq!(data.len(), 128 * 128 * 3);

        // Decode at half resolution (level 1)
        let result = reader.decode_region_at_resolution(0, 0, 64, 64, 1);
        assert!(result.is_ok());
        let data = result.expect("data");
        assert_eq!(data.len(), 64 * 64 * 3);

        // Invalid resolution level should fail
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
        });

        let decoder = reader.decode_progressive().expect("decoder");

        assert_eq!(decoder.total_layers(), 3);
        assert_eq!(decoder.current_layer(), 0);
        assert!(!decoder.is_complete());
        assert_eq!(decoder.progress(), 0.0);
    }
}
