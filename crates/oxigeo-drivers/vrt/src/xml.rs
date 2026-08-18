//! VRT XML format parser and writer

use crate::band::{PixelFunction, VrtBand};
use crate::dataset::{VrtDataset, VrtSubclass};
use crate::error::{Result, VrtError};
use crate::source::{PixelRect, SourceFilename, SourceWindow, VrtSource};
use crate::warp::{
    GenImgProjTransformer, ReprojectionTransformer, WarpBandMapping, WarpOptions, WarpResampleAlg,
    parse_working_data_type,
};
use oxigeo_core::types::{ColorInterpretation, GeoTransform, NoDataValue, RasterDataType};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use std::io::{BufRead, Write};
use std::path::Path;

/// VRT XML parser
pub struct VrtXmlParser;

impl VrtXmlParser {
    /// Parses VRT from XML string
    ///
    /// # Errors
    /// Returns an error if parsing fails
    pub fn parse(xml: &str) -> Result<VrtDataset> {
        let mut reader = Reader::from_str(xml);
        // Text is *not* trimmed per event: an entity reference splits an
        // element's character data into several `Text` events, and trimming
        // each one separately deletes the whitespace on either side of the
        // entity — turning a `<SourceFilename>` of `a &amp; b.tif` into
        // `a&b.tif` (cool-japan/oxigeo#15). `parse_text_element` trims the
        // assembled value instead, which is what "trim the element's text"
        // actually means.
        reader.config_mut().trim_text(false);

        let mut dataset = None;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) if e.name().as_ref() == b"VRTDataset" => {
                    dataset = Some(Self::parse_dataset(&mut reader, e)?);
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(VrtError::xml_parse(format!(
                        "XML parsing error at position {}: {}",
                        reader.buffer_position(),
                        e
                    )));
                }
                _ => {}
            }
            buf.clear();
        }

        dataset.ok_or_else(|| VrtError::xml_parse("No VRTDataset element found"))
    }

    /// Parses VRT from a file
    ///
    /// # Errors
    /// Returns an error if file reading or parsing fails
    pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<VrtDataset> {
        let xml = std::fs::read_to_string(&path)?;
        let mut dataset = Self::parse(&xml)?;
        dataset.vrt_path = Some(path.as_ref().to_path_buf());
        Ok(dataset)
    }

    fn parse_dataset<R: BufRead>(reader: &mut Reader<R>, start: &BytesStart) -> Result<VrtDataset> {
        let mut raster_x_size = 0u64;
        let mut raster_y_size = 0u64;
        let mut subclass = None;

        // Parse attributes
        for attr in start.attributes() {
            let attr = attr.map_err(|e| VrtError::xml_parse(format!("Attribute error: {}", e)))?;
            match attr.key.as_ref() {
                b"rasterXSize" => {
                    raster_x_size = Self::parse_u64(&attr.value)?;
                }
                b"rasterYSize" => {
                    raster_y_size = Self::parse_u64(&attr.value)?;
                }
                b"subClass" => {
                    let s = Self::parse_string(&attr.value)?;
                    subclass = Some(match s.as_str() {
                        "VRTWarpedDataset" => VrtSubclass::Warped,
                        "VRTPansharpenedDataset" => VrtSubclass::Pansharpened,
                        "VRTProcessedDataset" => VrtSubclass::Processed,
                        _ => VrtSubclass::Standard,
                    });
                }
                _ => {}
            }
        }

        let mut dataset = VrtDataset::new(raster_x_size, raster_y_size);
        if let Some(sc) = subclass {
            dataset = dataset.with_subclass(sc);
        }

        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => match e.name().as_ref() {
                    b"SRS" => {
                        dataset.srs = Some(Self::parse_text_element(reader, "SRS")?);
                    }
                    b"GeoTransform" => {
                        let text = Self::parse_text_element(reader, "GeoTransform")?;
                        dataset.geo_transform = Some(Self::parse_geotransform(&text)?);
                    }
                    b"VRTRasterBand" => {
                        let band = Self::parse_band(reader, e)?;
                        dataset.add_band(band);
                    }
                    b"BlockXSize" => {
                        let text = Self::parse_text_element(reader, "BlockXSize")?;
                        let x_size = text.parse::<u32>().map_err(|e| {
                            VrtError::xml_parse(format!("Invalid BlockXSize: {}", e))
                        })?;
                        let (_, y_size) = dataset.block_size.unwrap_or((0, 0));
                        dataset.block_size = Some((x_size, y_size));
                    }
                    b"BlockYSize" => {
                        let text = Self::parse_text_element(reader, "BlockYSize")?;
                        let y_size = text.parse::<u32>().map_err(|e| {
                            VrtError::xml_parse(format!("Invalid BlockYSize: {}", e))
                        })?;
                        let (x_size, _) = dataset.block_size.unwrap_or((0, 0));
                        dataset.block_size = Some((x_size, y_size));
                    }
                    b"GDALWarpOptions" => {
                        dataset.warp_options = Some(Self::parse_warp_options(reader)?);
                    }
                    _ => {
                        Self::skip_element(reader)?;
                    }
                },
                Ok(Event::End(ref e)) if e.name().as_ref() == b"VRTDataset" => break,
                Ok(Event::Eof) => {
                    return Err(VrtError::xml_parse("Unexpected EOF in VRTDataset"));
                }
                Err(e) => {
                    return Err(VrtError::xml_parse(format!("XML error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(dataset)
    }

    fn parse_band<R: BufRead>(reader: &mut Reader<R>, start: &BytesStart) -> Result<VrtBand> {
        let mut band_num = 0usize;
        let mut data_type = RasterDataType::UInt8;

        // Parse attributes
        for attr in start.attributes() {
            let attr = attr.map_err(|e| VrtError::xml_parse(format!("Attribute error: {}", e)))?;
            match attr.key.as_ref() {
                b"band" => {
                    band_num = Self::parse_usize(&attr.value)?;
                }
                b"dataType" => {
                    let s = Self::parse_string(&attr.value)?;
                    data_type = Self::parse_data_type(&s)?;
                }
                _ => {}
            }
        }

        let mut band = VrtBand::new(band_num, data_type);
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => match e.name().as_ref() {
                    b"NoDataValue" => {
                        let text = Self::parse_text_element(reader, "NoDataValue")?;
                        band.nodata = Self::parse_nodata(&text)?;
                    }
                    b"ColorInterp" => {
                        let text = Self::parse_text_element(reader, "ColorInterp")?;
                        band.color_interp = Self::parse_color_interp(&text);
                    }
                    b"SimpleSource" | b"ComplexSource" => {
                        let source = Self::parse_source(reader, e)?;
                        band.add_source(source);
                    }
                    b"Offset" => {
                        let text = Self::parse_text_element(reader, "Offset")?;
                        band.offset = text.parse::<f64>().ok();
                    }
                    b"Scale" => {
                        let text = Self::parse_text_element(reader, "Scale")?;
                        band.scale = text.parse::<f64>().ok();
                    }
                    b"PixelFunctionType" => {
                        let text = Self::parse_text_element(reader, "PixelFunctionType")?;
                        band.pixel_function = Some(Self::parse_pixel_function(&text));
                    }
                    _ => {
                        Self::skip_element(reader)?;
                    }
                },
                Ok(Event::End(ref e)) if e.name().as_ref() == b"VRTRasterBand" => break,
                Ok(Event::Eof) => {
                    return Err(VrtError::xml_parse("Unexpected EOF in VRTRasterBand"));
                }
                Err(e) => {
                    return Err(VrtError::xml_parse(format!("XML error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(band)
    }

    /// Parses a `<GDALWarpOptions>` block.
    ///
    /// The subtree is walked by element name rather than by position: GDAL
    /// wraps `<GenImgProjTransformer>` in `<ApproxTransformer>`/
    /// `<BaseTransformer>` only when an approximating spline was requested, and
    /// adds elements between releases, so anchoring on the leaf names parses
    /// every shape a real writer emits.
    fn parse_warp_options<R: BufRead>(reader: &mut Reader<R>) -> Result<WarpOptions> {
        let mut options = WarpOptions::default();
        Self::walk_warp_element(reader, b"GDALWarpOptions", &mut options, 0)?;
        Ok(options)
    }

    /// Recursion limit for the warp transformer chain. GDAL's deepest emitted
    /// nesting is 7 levels; the bound only exists so a hand-written or hostile
    /// VRT cannot drive the parser into a stack overflow.
    const MAX_WARP_DEPTH: usize = 32;

    fn walk_warp_element<R: BufRead>(
        reader: &mut Reader<R>,
        end_name: &[u8],
        options: &mut WarpOptions,
        depth: usize,
    ) -> Result<()> {
        if depth > Self::MAX_WARP_DEPTH {
            return Err(VrtError::xml_parse(
                "GDALWarpOptions nesting exceeds the supported depth",
            ));
        }

        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    Self::warp_start_element(reader, e, &name, options, depth)?;
                }
                Ok(Event::Empty(ref e)) => {
                    Self::warp_empty_element(e, options)?;
                }
                Ok(Event::End(ref e)) if e.name().as_ref() == end_name => break,
                Ok(Event::Eof) => {
                    return Err(VrtError::xml_parse("Unexpected EOF inside GDALWarpOptions"));
                }
                Err(e) => {
                    return Err(VrtError::xml_parse(format!("XML error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(())
    }

    fn warp_start_element<R: BufRead>(
        reader: &mut Reader<R>,
        start: &BytesStart,
        name: &[u8],
        options: &mut WarpOptions,
        depth: usize,
    ) -> Result<()> {
        match name {
            b"ResampleAlg" => {
                let text = Self::parse_text_element(reader, "ResampleAlg")?;
                options.resample_alg = WarpResampleAlg::parse(&text);
            }
            b"WorkingDataType" => {
                let text = Self::parse_text_element(reader, "WorkingDataType")?;
                options.working_data_type = parse_working_data_type(&text);
            }
            b"WarpMemoryLimit" => {
                let text = Self::parse_text_element(reader, "WarpMemoryLimit")?;
                options.warp_memory_limit = text.parse::<f64>().ok();
            }
            b"SourceDataset" => {
                let relative = Self::relative_to_vrt(start)?;
                let text = Self::parse_text_element(reader, "SourceDataset")?;
                options.source_dataset = Some(SourceFilename::new(text, relative));
            }
            b"Option" => {
                let (key, is_reprojection) = Self::warp_option_key(start)?;
                let value = Self::parse_text_element(reader, "Option")?;
                Self::push_warp_option(options, key, value, is_reprojection);
            }
            b"SrcGeoTransform" => {
                let text = Self::parse_text_element(reader, "SrcGeoTransform")?;
                Self::transformer_mut(options).src_geo_transform =
                    Some(Self::parse_geotransform(&text)?);
            }
            b"DstGeoTransform" => {
                let text = Self::parse_text_element(reader, "DstGeoTransform")?;
                Self::transformer_mut(options).dst_geo_transform =
                    Some(Self::parse_geotransform(&text)?);
            }
            // The inverse geotransforms GDAL writes are a rounded convenience
            // copy of the forward ones. They are deliberately ignored: the
            // warper inverts `Src`/`DstGeoTransform` itself at full precision,
            // and honouring the file's rounded values would inject sub-pixel
            // sampling error (cool-japan/oxigeo#15).
            b"SrcInvGeoTransform" | b"DstInvGeoTransform" => {
                Self::skip_element(reader)?;
            }
            b"MaxError" => {
                let text = Self::parse_text_element(reader, "MaxError")?;
                Self::transformer_mut(options).max_error = text.parse::<f64>().ok();
            }
            b"SourceSRS" => {
                let text = Self::parse_text_element(reader, "SourceSRS")?;
                Self::reprojection_mut(options).source_srs = Some(text);
            }
            b"TargetSRS" => {
                let text = Self::parse_text_element(reader, "TargetSRS")?;
                Self::reprojection_mut(options).target_srs = Some(text);
            }
            b"BandMapping" => {
                let mapping = Self::parse_band_mapping(reader, start)?;
                options.band_mappings.push(mapping);
            }
            // Container elements (Transformer, ApproxTransformer,
            // BaseTransformer, GenImgProjTransformer, ReprojectTransformer,
            // ReprojectionTransformer, Options, BandList, …) and any element a
            // future GDAL adds: descend and keep looking for leaves we know.
            other => {
                Self::walk_warp_element(reader, other, options, depth + 1)?;
            }
        }

        Ok(())
    }

    fn warp_empty_element(start: &BytesStart, options: &mut WarpOptions) -> Result<()> {
        match start.name().as_ref() {
            b"Option" => {
                let (key, is_reprojection) = Self::warp_option_key(start)?;
                Self::push_warp_option(options, key, String::new(), is_reprojection);
            }
            b"BandMapping" => {
                let (src, dst) = Self::band_mapping_attrs(start)?;
                options.band_mappings.push(WarpBandMapping {
                    src,
                    dst,
                    src_nodata_real: None,
                    dst_nodata_real: None,
                });
            }
            _ => {}
        }
        Ok(())
    }

    /// Returns the option key and whether it belongs to the reprojection
    /// transformer.
    ///
    /// GDAL spells warp-level options `<Option name="…">` and the
    /// reprojection transformer's `<Option key="…">`; the attribute is what
    /// tells the two apart, since both appear under `<GDALWarpOptions>`.
    fn warp_option_key(start: &BytesStart) -> Result<(String, bool)> {
        let mut key = String::new();
        let mut is_reprojection = false;

        for attr in start.attributes() {
            let attr = attr.map_err(|e| VrtError::xml_parse(format!("Attribute error: {}", e)))?;
            match attr.key.as_ref() {
                b"name" => key = Self::parse_string(&attr.value)?,
                b"key" => {
                    key = Self::parse_string(&attr.value)?;
                    is_reprojection = true;
                }
                _ => {}
            }
        }

        Ok((key, is_reprojection))
    }

    fn push_warp_option(
        options: &mut WarpOptions,
        key: String,
        value: String,
        is_reprojection: bool,
    ) {
        if key.is_empty() {
            return;
        }
        if is_reprojection {
            Self::reprojection_mut(options).options.push((key, value));
        } else {
            options.options.push((key, value));
        }
    }

    fn transformer_mut(options: &mut WarpOptions) -> &mut GenImgProjTransformer {
        options.transformer.get_or_insert_with(Default::default)
    }

    fn reprojection_mut(options: &mut WarpOptions) -> &mut ReprojectionTransformer {
        Self::transformer_mut(options)
            .reprojection
            .get_or_insert_with(Default::default)
    }

    fn band_mapping_attrs(start: &BytesStart) -> Result<(usize, usize)> {
        let mut src = 0usize;
        let mut dst = 0usize;

        for attr in start.attributes() {
            let attr = attr.map_err(|e| VrtError::xml_parse(format!("Attribute error: {}", e)))?;
            match attr.key.as_ref() {
                b"src" => src = Self::parse_usize(&attr.value)?,
                b"dst" => dst = Self::parse_usize(&attr.value)?,
                _ => {}
            }
        }

        Ok((src, dst))
    }

    fn parse_band_mapping<R: BufRead>(
        reader: &mut Reader<R>,
        start: &BytesStart,
    ) -> Result<WarpBandMapping> {
        let (src, dst) = Self::band_mapping_attrs(start)?;
        let mut src_nodata_real = None;
        let mut dst_nodata_real = None;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => match e.name().as_ref() {
                    b"SrcNoDataReal" => {
                        let text = Self::parse_text_element(reader, "SrcNoDataReal")?;
                        src_nodata_real = text.parse::<f64>().ok();
                    }
                    b"DstNoDataReal" => {
                        let text = Self::parse_text_element(reader, "DstNoDataReal")?;
                        dst_nodata_real = text.parse::<f64>().ok();
                    }
                    _ => {
                        Self::skip_element(reader)?;
                    }
                },
                Ok(Event::End(ref e)) if e.name().as_ref() == b"BandMapping" => break,
                Ok(Event::Eof) => {
                    return Err(VrtError::xml_parse("Unexpected EOF in BandMapping"));
                }
                Err(e) => {
                    return Err(VrtError::xml_parse(format!("XML error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(WarpBandMapping {
            src,
            dst,
            src_nodata_real,
            dst_nodata_real,
        })
    }

    /// Reads a `relativeToVRT` attribute, defaulting to absolute.
    fn relative_to_vrt(start: &BytesStart) -> Result<bool> {
        for attr in start.attributes() {
            let attr = attr.map_err(|e| VrtError::xml_parse(format!("Attribute error: {}", e)))?;
            if attr.key.as_ref() == b"relativeToVRT" {
                return Ok(Self::parse_string(&attr.value)?.trim() != "0");
            }
        }
        Ok(false)
    }

    fn parse_source<R: BufRead>(reader: &mut Reader<R>, start: &BytesStart) -> Result<VrtSource> {
        let mut filename = None;
        let mut source_band = 1usize;
        let mut src_rect = None;
        let mut dst_rect = None;
        let mut nodata = NoDataValue::None;
        let mut buf = Vec::new();
        let element_name = start.name().as_ref().to_vec();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => match e.name().as_ref() {
                    b"SourceFilename" => {
                        // `relativeToVRT` was discarded here, so every source of
                        // a VRT written with relative paths — the default for
                        // `gdalbuildvrt` run inside the data directory — was
                        // resolved against the process CWD instead of the VRT's
                        // own directory (cool-japan/oxigeo#15).
                        let relative = Self::relative_to_vrt(e)?;
                        let text = Self::parse_text_element(reader, "SourceFilename")?;
                        filename = Some(SourceFilename::new(text, relative));
                    }
                    b"SourceBand" => {
                        let text = Self::parse_text_element(reader, "SourceBand")?;
                        source_band = text.parse::<usize>().map_err(|e| {
                            VrtError::xml_parse(format!("Invalid SourceBand: {}", e))
                        })?;
                    }
                    b"SrcRect" => {
                        src_rect = Some(Self::parse_rect_from_start(reader, e)?);
                    }
                    b"DstRect" => {
                        dst_rect = Some(Self::parse_rect_from_start(reader, e)?);
                    }
                    // `<NODATA>` marks the source's own nodata value. GDAL
                    // skips source pixels equal to it when compositing, so an
                    // overlapping source can supply valid data where this one
                    // has none (cool-japan/oxigeo#19). Discarding it here made
                    // that impossible to honour downstream.
                    b"NODATA" => {
                        let text = Self::parse_text_element(reader, "NODATA")?;
                        nodata = Self::parse_nodata(&text)?;
                    }
                    _ => {
                        Self::skip_element(reader)?;
                    }
                },
                // Handle self-closing elements like <SrcRect ... />
                Ok(Event::Empty(ref e)) => match e.name().as_ref() {
                    b"SrcRect" => {
                        src_rect = Some(Self::parse_rect_from_empty(e)?);
                    }
                    b"DstRect" => {
                        dst_rect = Some(Self::parse_rect_from_empty(e)?);
                    }
                    _ => {}
                },
                Ok(Event::End(ref e)) if e.name().as_ref() == element_name => break,
                Ok(Event::Eof) => {
                    return Err(VrtError::xml_parse("Unexpected EOF in source element"));
                }
                Err(e) => {
                    return Err(VrtError::xml_parse(format!("XML error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        let filename = filename.ok_or_else(|| VrtError::xml_parse("Missing SourceFilename"))?;
        let mut source = VrtSource::new(filename, source_band);

        if let (Some(src), Some(dst)) = (src_rect, dst_rect) {
            source = source.with_window(SourceWindow::new(src, dst));
        }
        if !nodata.is_none() {
            source = source.with_nodata(nodata);
        }

        Ok(source)
    }

    /// Parses PixelRect from attributes only (for self-closing tags like `<SrcRect ... />`)
    ///
    /// GDAL keeps source and destination windows as doubles and only rounds
    /// them at rasterisation time, so `gdalbuildvrt` / `gdalwarp -of VRT`
    /// routinely emit sub-pixel values such as `xOff="9783.50000000003"` or
    /// `xSize="9889.75000000021"`. Parsing these with `str::parse::<u64>`
    /// rejected the whole file with `Invalid u64: invalid digit found in
    /// string`, which made every real-world GDAL mosaic unreadable
    /// (cool-japan/oxigeo#18).
    fn parse_rect_from_empty(start: &BytesStart) -> Result<PixelRect> {
        let mut x_off = 0.0f64;
        let mut y_off = 0.0f64;
        let mut x_size = 0.0f64;
        let mut y_size = 0.0f64;

        for attr in start.attributes() {
            let attr = attr.map_err(|e| VrtError::xml_parse(format!("Attribute error: {}", e)))?;
            match attr.key.as_ref() {
                b"xOff" => x_off = Self::parse_rect_f64(&attr.value, "xOff")?,
                b"yOff" => y_off = Self::parse_rect_f64(&attr.value, "yOff")?,
                b"xSize" => x_size = Self::parse_rect_f64(&attr.value, "xSize")?,
                b"ySize" => y_size = Self::parse_rect_f64(&attr.value, "ySize")?,
                _ => {}
            }
        }

        let (x_off, x_size) = Self::round_span(x_off, x_size, 'x')?;
        let (y_off, y_size) = Self::round_span(y_off, y_size, 'y')?;

        Ok(PixelRect::new(x_off, y_off, x_size, y_size))
    }

    /// Parses one `SrcRect`/`DstRect` attribute, accepting every numeric format
    /// GDAL may write (integer, decimal, or scientific notation).
    fn parse_rect_f64(bytes: &[u8], attr_name: &str) -> Result<f64> {
        let s = Self::parse_string(bytes)?;
        let value = s.trim().parse::<f64>().map_err(|e| {
            VrtError::xml_parse(format!("Invalid {attr_name} rect value {s:?}: {e}"))
        })?;
        if !value.is_finite() {
            return Err(VrtError::xml_parse(format!(
                "Invalid {attr_name} rect value {s:?}: must be finite"
            )));
        }
        Ok(value)
    }

    /// Rounds a sub-pixel `(offset, size)` span onto the whole-pixel grid.
    ///
    /// Rounding is **edge-consistent**: the near edge (`off`) and the far edge
    /// (`off + size`) are each rounded to nearest and the size is derived as
    /// their difference. Rounding `off` and `size` independently would let a
    /// tile whose far edge is exactly its neighbour's near edge round to two
    /// different integers and open a one-pixel seam between adjacent mosaic
    /// sources; deriving the size from the rounded edges keeps neighbours flush
    /// by construction.
    fn round_span(off: f64, size: f64, axis: char) -> Result<(u64, u64)> {
        if off < 0.0 {
            return Err(VrtError::xml_parse(format!(
                "Invalid {axis}Off rect value {off}: must not be negative"
            )));
        }
        if size < 0.0 {
            return Err(VrtError::xml_parse(format!(
                "Invalid {axis}Size rect value {size}: must not be negative"
            )));
        }

        let near = off.round();
        let far = (off + size).round();
        let rounded_off = near as u64;
        let mut rounded_size = (far - near).max(0.0) as u64;

        // A source covering a non-empty sub-pixel span must not collapse to an
        // empty rect: `PixelRect::intersect` drops zero-sized rects, so the
        // pixels this source carries would silently vanish from the mosaic.
        if rounded_size == 0 && size > 0.0 {
            rounded_size = 1;
        }

        Ok((rounded_off, rounded_size))
    }

    /// Parses PixelRect from a Start event (needs to consume End event)
    fn parse_rect_from_start<R: BufRead>(
        reader: &mut Reader<R>,
        start: &BytesStart,
    ) -> Result<PixelRect> {
        let rect = Self::parse_rect_from_empty(start)?;
        Self::skip_element(reader)?;
        Ok(rect)
    }

    fn parse_geotransform(text: &str) -> Result<GeoTransform> {
        let parts: Vec<&str> = text.split(',').map(|s| s.trim()).collect();
        if parts.len() != 6 {
            return Err(VrtError::xml_parse("GeoTransform must have 6 values"));
        }

        let values: Result<Vec<f64>> = parts
            .iter()
            .map(|s| {
                s.parse::<f64>()
                    .map_err(|e| VrtError::xml_parse(format!("Invalid GeoTransform value: {}", e)))
            })
            .collect();

        let v = values?;
        Ok(GeoTransform {
            origin_x: v[0],
            pixel_width: v[1],
            row_rotation: v[2],
            origin_y: v[3],
            col_rotation: v[4],
            pixel_height: v[5],
        })
    }

    fn parse_data_type(s: &str) -> Result<RasterDataType> {
        match s {
            "Byte" => Ok(RasterDataType::UInt8),
            "UInt16" => Ok(RasterDataType::UInt16),
            "Int16" => Ok(RasterDataType::Int16),
            "UInt32" => Ok(RasterDataType::UInt32),
            "Int32" => Ok(RasterDataType::Int32),
            "Float32" => Ok(RasterDataType::Float32),
            "Float64" => Ok(RasterDataType::Float64),
            _ => Err(VrtError::xml_parse(format!("Unknown data type: {}", s))),
        }
    }

    fn parse_nodata(s: &str) -> Result<NoDataValue> {
        if let Ok(val) = s.parse::<f64>() {
            Ok(NoDataValue::Float(val))
        } else {
            Ok(NoDataValue::None)
        }
    }

    fn parse_color_interp(s: &str) -> ColorInterpretation {
        match s {
            "Red" => ColorInterpretation::Red,
            "Green" => ColorInterpretation::Green,
            "Blue" => ColorInterpretation::Blue,
            "Alpha" => ColorInterpretation::Alpha,
            "Gray" => ColorInterpretation::Gray,
            "Palette" => ColorInterpretation::PaletteIndex,
            _ => ColorInterpretation::Undefined,
        }
    }

    fn parse_pixel_function(s: &str) -> PixelFunction {
        match s {
            "average" | "Average" => PixelFunction::Average,
            "min" | "Min" => PixelFunction::Min,
            "max" | "Max" => PixelFunction::Max,
            "sum" | "Sum" => PixelFunction::Sum,
            _ => PixelFunction::Custom {
                name: s.to_string(),
            },
        }
    }

    /// Reads the character content of an element, up to its closing tag.
    ///
    /// quick-xml reports an entity reference (`&quot;`, `&amp;`, `&#34;`) as its
    /// own [`Event::GeneralRef`] rather than as part of the surrounding
    /// [`Event::Text`]. Those events used to fall through the catch-all arm and
    /// be dropped, so every escaped character silently **vanished** from the
    /// parsed value: a `<SRS>` written by this crate's own [`VrtXmlWriter`]
    /// (which escapes the quotes of a WKT tree) read back as
    /// `GEOGCS[WGS 84,...]` with every `"` missing, and any `<SourceFilename>`
    /// containing `&` lost it (cool-japan/oxigeo#15).
    pub(crate) fn parse_text_element<R: BufRead>(
        reader: &mut Reader<R>,
        name: &str,
    ) -> Result<String> {
        let mut text = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Text(e)) => {
                    text.push_str(
                        &e.decode().map_err(|e| {
                            VrtError::xml_parse(format!("Text decode error: {}", e))
                        })?,
                    );
                }
                Ok(Event::CData(e)) => {
                    text.push_str(&String::from_utf8_lossy(&e.into_inner()));
                }
                Ok(Event::GeneralRef(e)) => {
                    if let Ok(Some(ch)) = e.resolve_char_ref() {
                        text.push(ch);
                        buf.clear();
                        continue;
                    }
                    let name = e
                        .decode()
                        .map_err(|e| VrtError::xml_parse(format!("Entity decode error: {}", e)))?;
                    match name.as_ref() {
                        "quot" => text.push('"'),
                        "apos" => text.push('\''),
                        "lt" => text.push('<'),
                        "gt" => text.push('>'),
                        "amp" => text.push('&'),
                        // A DTD-defined entity, which a VRT never uses: keep it
                        // verbatim rather than silently dropping it.
                        other => {
                            text.push('&');
                            text.push_str(other);
                            text.push(';');
                        }
                    }
                }
                Ok(Event::End(_)) => break,
                Ok(Event::Eof) => {
                    return Err(VrtError::xml_parse(format!("Unexpected EOF in {}", name)));
                }
                Err(e) => {
                    return Err(VrtError::xml_parse(format!("XML error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(text.trim().to_string())
    }

    fn skip_element<R: BufRead>(reader: &mut Reader<R>) -> Result<()> {
        let mut depth = 1;
        let mut buf = Vec::new();

        while depth > 0 {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(_)) => depth += 1,
                Ok(Event::End(_)) => depth -= 1,
                Ok(Event::Eof) => {
                    return Err(VrtError::xml_parse("Unexpected EOF while skipping element"));
                }
                Err(e) => {
                    return Err(VrtError::xml_parse(format!("XML error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(())
    }

    fn parse_string(bytes: &[u8]) -> Result<String> {
        String::from_utf8(bytes.to_vec())
            .map_err(|e| VrtError::xml_parse(format!("UTF-8 error: {}", e)))
    }

    fn parse_u64(bytes: &[u8]) -> Result<u64> {
        let s = Self::parse_string(bytes)?;
        s.parse::<u64>()
            .map_err(|e| VrtError::xml_parse(format!("Invalid u64: {}", e)))
    }

    fn parse_usize(bytes: &[u8]) -> Result<usize> {
        let s = Self::parse_string(bytes)?;
        s.parse::<usize>()
            .map_err(|e| VrtError::xml_parse(format!("Invalid usize: {}", e)))
    }
}

/// VRT XML writer
pub struct VrtXmlWriter;

impl VrtXmlWriter {
    /// Writes VRT dataset to XML string
    ///
    /// # Errors
    /// Returns an error if writing fails
    pub fn write(dataset: &VrtDataset) -> Result<String> {
        let mut buffer = Vec::new();
        let mut writer = Writer::new_with_indent(&mut buffer, b' ', 2);

        Self::write_dataset(&mut writer, dataset)?;

        String::from_utf8(buffer).map_err(|e| VrtError::xml_parse(format!("UTF-8 error: {}", e)))
    }

    /// Writes VRT dataset to a file
    ///
    /// # Errors
    /// Returns an error if file writing fails
    pub fn write_file<P: AsRef<Path>>(dataset: &VrtDataset, path: P) -> Result<()> {
        let xml = Self::write(dataset)?;
        std::fs::write(path, xml)?;
        Ok(())
    }

    fn write_dataset<W: Write>(writer: &mut Writer<W>, dataset: &VrtDataset) -> Result<()> {
        let mut elem = BytesStart::new("VRTDataset");
        elem.push_attribute(("rasterXSize", dataset.raster_x_size.to_string().as_str()));
        elem.push_attribute(("rasterYSize", dataset.raster_y_size.to_string().as_str()));

        if let Some(ref subclass) = dataset.subclass {
            let subclass_str = match subclass {
                VrtSubclass::Warped => "VRTWarpedDataset",
                VrtSubclass::Pansharpened => "VRTPansharpenedDataset",
                VrtSubclass::Processed => "VRTProcessedDataset",
                VrtSubclass::Standard => "VRTDataset",
            };
            elem.push_attribute(("subClass", subclass_str));
        }

        writer
            .write_event(Event::Start(elem))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        if let Some(ref srs) = dataset.srs {
            Self::write_text_element(writer, "SRS", srs)?;
        }

        if let Some(ref gt) = dataset.geo_transform {
            let text = format!(
                "{}, {}, {}, {}, {}, {}",
                gt.origin_x,
                gt.pixel_width,
                gt.row_rotation,
                gt.origin_y,
                gt.col_rotation,
                gt.pixel_height
            );
            Self::write_text_element(writer, "GeoTransform", &text)?;
        }

        for band in &dataset.bands {
            Self::write_band(writer, band)?;
        }

        if let Some((x_size, y_size)) = dataset.block_size {
            Self::write_text_element(writer, "BlockXSize", &x_size.to_string())?;
            Self::write_text_element(writer, "BlockYSize", &y_size.to_string())?;
        }

        // Without this a warped VRT read in and written back out lost the only
        // element describing its pixels, leaving a `VRTWarpedDataset` that
        // names no source at all (cool-japan/oxigeo#15).
        if let Some(ref warp) = dataset.warp_options {
            Self::write_warp_options(writer, warp)?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("VRTDataset")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        Ok(())
    }

    fn write_warp_options<W: Write>(writer: &mut Writer<W>, warp: &WarpOptions) -> Result<()> {
        writer
            .write_event(Event::Start(BytesStart::new("GDALWarpOptions")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        if let Some(limit) = warp.warp_memory_limit {
            Self::write_text_element(writer, "WarpMemoryLimit", &limit.to_string())?;
        }
        Self::write_text_element(writer, "ResampleAlg", warp.resample_alg.as_str())?;
        if let Some(dt) = warp.working_data_type {
            Self::write_text_element(writer, "WorkingDataType", Self::data_type_name(dt))?;
        }

        for (key, value) in &warp.options {
            Self::write_named_option(writer, "name", key, value)?;
        }

        if let Some(ref source) = warp.source_dataset {
            let mut elem = BytesStart::new("SourceDataset");
            elem.push_attribute((
                "relativeToVRT",
                if source.relative_to_vrt { "1" } else { "0" },
            ));
            writer
                .write_event(Event::Start(elem))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
            writer
                .write_event(Event::Text(BytesText::new(
                    &source.path.display().to_string(),
                )))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
            writer
                .write_event(Event::End(BytesEnd::new("SourceDataset")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        }

        if let Some(ref transformer) = warp.transformer {
            Self::write_transformer(writer, transformer)?;
        }

        if !warp.band_mappings.is_empty() {
            writer
                .write_event(Event::Start(BytesStart::new("BandList")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
            for mapping in &warp.band_mappings {
                let mut elem = BytesStart::new("BandMapping");
                elem.push_attribute(("src", mapping.src.to_string().as_str()));
                elem.push_attribute(("dst", mapping.dst.to_string().as_str()));
                writer
                    .write_event(Event::Start(elem))
                    .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
                if let Some(v) = mapping.src_nodata_real {
                    Self::write_text_element(writer, "SrcNoDataReal", &v.to_string())?;
                }
                if let Some(v) = mapping.dst_nodata_real {
                    Self::write_text_element(writer, "DstNoDataReal", &v.to_string())?;
                }
                writer
                    .write_event(Event::End(BytesEnd::new("BandMapping")))
                    .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
            }
            writer
                .write_event(Event::End(BytesEnd::new("BandList")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("GDALWarpOptions")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        Ok(())
    }

    /// Writes the transformer chain.
    ///
    /// The `<ApproxTransformer>`/`<BaseTransformer>` wrappers are emitted only
    /// when a `<MaxError>` was recorded, matching what GDAL does — writing them
    /// unconditionally would claim an approximation that was never requested.
    fn write_transformer<W: Write>(
        writer: &mut Writer<W>,
        transformer: &GenImgProjTransformer,
    ) -> Result<()> {
        writer
            .write_event(Event::Start(BytesStart::new("Transformer")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        let approx = transformer.max_error.is_some();
        if let Some(max_error) = transformer.max_error {
            writer
                .write_event(Event::Start(BytesStart::new("ApproxTransformer")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
            Self::write_text_element(writer, "MaxError", &max_error.to_string())?;
            writer
                .write_event(Event::Start(BytesStart::new("BaseTransformer")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        }

        writer
            .write_event(Event::Start(BytesStart::new("GenImgProjTransformer")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        if let Some(ref gt) = transformer.src_geo_transform {
            Self::write_text_element(writer, "SrcGeoTransform", &Self::geotransform_text(gt))?;
        }
        if let Some(ref gt) = transformer.dst_geo_transform {
            Self::write_text_element(writer, "DstGeoTransform", &Self::geotransform_text(gt))?;
        }

        if let Some(ref reprojection) = transformer.reprojection {
            writer
                .write_event(Event::Start(BytesStart::new("ReprojectTransformer")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
            writer
                .write_event(Event::Start(BytesStart::new("ReprojectionTransformer")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

            if let Some(ref srs) = reprojection.source_srs {
                Self::write_text_element(writer, "SourceSRS", srs)?;
            }
            if let Some(ref srs) = reprojection.target_srs {
                Self::write_text_element(writer, "TargetSRS", srs)?;
            }
            if !reprojection.options.is_empty() {
                writer
                    .write_event(Event::Start(BytesStart::new("Options")))
                    .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
                for (key, value) in &reprojection.options {
                    Self::write_named_option(writer, "key", key, value)?;
                }
                writer
                    .write_event(Event::End(BytesEnd::new("Options")))
                    .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
            }

            writer
                .write_event(Event::End(BytesEnd::new("ReprojectionTransformer")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
            writer
                .write_event(Event::End(BytesEnd::new("ReprojectTransformer")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("GenImgProjTransformer")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        if approx {
            writer
                .write_event(Event::End(BytesEnd::new("BaseTransformer")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
            writer
                .write_event(Event::End(BytesEnd::new("ApproxTransformer")))
                .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("Transformer")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        Ok(())
    }

    fn write_named_option<W: Write>(
        writer: &mut Writer<W>,
        attr: &str,
        key: &str,
        value: &str,
    ) -> Result<()> {
        let mut elem = BytesStart::new("Option");
        elem.push_attribute((attr, key));
        writer
            .write_event(Event::Start(elem))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        writer
            .write_event(Event::Text(BytesText::new(value)))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        writer
            .write_event(Event::End(BytesEnd::new("Option")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        Ok(())
    }

    /// Formats a geotransform with enough precision to survive a round-trip.
    ///
    /// A warped VRT's geotransform is a sub-metre grid definition over
    /// continent-scale coordinates; `{}` drops digits that shift the whole
    /// raster.
    fn geotransform_text(gt: &GeoTransform) -> String {
        format!(
            "{:.17},{:.17},{:.17},{:.17},{:.17},{:.17}",
            gt.origin_x,
            gt.pixel_width,
            gt.row_rotation,
            gt.origin_y,
            gt.col_rotation,
            gt.pixel_height
        )
    }

    fn write_band<W: Write>(writer: &mut Writer<W>, band: &VrtBand) -> Result<()> {
        let mut elem = BytesStart::new("VRTRasterBand");
        elem.push_attribute(("band", band.band.to_string().as_str()));
        elem.push_attribute(("dataType", Self::data_type_name(band.data_type)));

        writer
            .write_event(Event::Start(elem))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        if let Some(nodata) = Self::nodata_value(band.nodata) {
            Self::write_text_element(writer, "NoDataValue", &nodata)?;
        }

        if band.color_interp != ColorInterpretation::Undefined {
            Self::write_text_element(
                writer,
                "ColorInterp",
                Self::color_interp_name(band.color_interp),
            )?;
        }

        for source in &band.sources {
            Self::write_source(writer, source)?;
        }

        if let Some(offset) = band.offset {
            Self::write_text_element(writer, "Offset", &offset.to_string())?;
        }

        if let Some(scale) = band.scale {
            Self::write_text_element(writer, "Scale", &scale.to_string())?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("VRTRasterBand")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        Ok(())
    }

    fn write_source<W: Write>(writer: &mut Writer<W>, source: &VrtSource) -> Result<()> {
        let elem = BytesStart::new("SimpleSource");
        writer
            .write_event(Event::Start(elem))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        // The flag must be written back: dropping it turns a portable
        // VRT-relative path into one resolved against the process CWD, which is
        // exactly the read-side bug this release fixes (cool-japan/oxigeo#15).
        let mut filename_elem = BytesStart::new("SourceFilename");
        filename_elem.push_attribute((
            "relativeToVRT",
            if source.filename.relative_to_vrt {
                "1"
            } else {
                "0"
            },
        ));
        writer
            .write_event(Event::Start(filename_elem))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        writer
            .write_event(Event::Text(BytesText::new(
                &source.filename.path.display().to_string(),
            )))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        writer
            .write_event(Event::End(BytesEnd::new("SourceFilename")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        Self::write_text_element(writer, "SourceBand", &source.source_band.to_string())?;

        if let Some(ref window) = source.window {
            Self::write_rect(writer, "SrcRect", &window.src_rect)?;
            Self::write_rect(writer, "DstRect", &window.dst_rect)?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("SimpleSource")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        Ok(())
    }

    fn write_rect<W: Write>(writer: &mut Writer<W>, name: &str, rect: &PixelRect) -> Result<()> {
        let mut elem = BytesStart::new(name);
        elem.push_attribute(("xOff", rect.x_off.to_string().as_str()));
        elem.push_attribute(("yOff", rect.y_off.to_string().as_str()));
        elem.push_attribute(("xSize", rect.x_size.to_string().as_str()));
        elem.push_attribute(("ySize", rect.y_size.to_string().as_str()));

        writer
            .write_event(Event::Empty(elem))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        Ok(())
    }

    fn write_text_element<W: Write>(writer: &mut Writer<W>, name: &str, text: &str) -> Result<()> {
        writer
            .write_event(Event::Start(BytesStart::new(name)))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        writer
            .write_event(Event::Text(BytesText::new(text)))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        writer
            .write_event(Event::End(BytesEnd::new(name)))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;
        Ok(())
    }

    fn data_type_name(dt: RasterDataType) -> &'static str {
        match dt {
            RasterDataType::UInt8 => "Byte",
            RasterDataType::UInt16 => "UInt16",
            RasterDataType::Int16 => "Int16",
            RasterDataType::UInt32 => "UInt32",
            RasterDataType::Int32 => "Int32",
            RasterDataType::Float32 => "Float32",
            RasterDataType::Float64 => "Float64",
            _ => "Byte",
        }
    }

    fn nodata_value(nd: NoDataValue) -> Option<String> {
        match nd {
            NoDataValue::None => None,
            NoDataValue::Integer(v) => Some(v.to_string()),
            NoDataValue::Float(v) => Some(v.to_string()),
        }
    }

    fn color_interp_name(ci: ColorInterpretation) -> &'static str {
        match ci {
            ColorInterpretation::Red => "Red",
            ColorInterpretation::Green => "Green",
            ColorInterpretation::Blue => "Blue",
            ColorInterpretation::Alpha => "Alpha",
            ColorInterpretation::Gray => "Gray",
            ColorInterpretation::PaletteIndex => "Palette",
            ColorInterpretation::Undefined => "Undefined",
            _ => "Undefined",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_vrt() {
        let xml = r#"
<VRTDataset rasterXSize="512" rasterYSize="512">
  <SRS>EPSG:4326</SRS>
  <GeoTransform>0.0, 1.0, 0.0, 0.0, 0.0, -1.0</GeoTransform>
  <VRTRasterBand band="1" dataType="Byte">
    <NoDataValue>0</NoDataValue>
    <SimpleSource>
      <SourceFilename>/path/to/file.tif</SourceFilename>
      <SourceBand>1</SourceBand>
    </SimpleSource>
  </VRTRasterBand>
</VRTDataset>
"#;

        let dataset = VrtXmlParser::parse(xml);
        assert!(dataset.is_ok());
        let ds = dataset.expect("Should parse");
        assert_eq!(ds.raster_x_size, 512);
        assert_eq!(ds.raster_y_size, 512);
        assert_eq!(ds.band_count(), 1);
    }

    #[test]
    fn test_write_simple_vrt() {
        let mut dataset = VrtDataset::new(512, 512);
        let source = VrtSource::simple("/test.tif", 1);
        let band = VrtBand::simple(1, RasterDataType::UInt8, source);
        dataset.add_band(band);

        let xml = VrtXmlWriter::write(&dataset);
        assert!(xml.is_ok());
        let xml_str = xml.expect("Should write");
        assert!(xml_str.contains("VRTDataset"));
        assert!(xml_str.contains("rasterXSize=\"512\""));
        assert!(xml_str.contains("VRTRasterBand"));
    }

    #[test]
    fn test_roundtrip() {
        let mut dataset = VrtDataset::new(1024, 768);
        dataset = dataset.with_srs("EPSG:4326");
        let source = VrtSource::simple("/test.tif", 1);
        let band = VrtBand::simple(1, RasterDataType::UInt8, source);
        dataset.add_band(band);

        let xml = VrtXmlWriter::write(&dataset).expect("Should write");
        let parsed = VrtXmlParser::parse(&xml).expect("Should parse");

        assert_eq!(parsed.raster_x_size, 1024);
        assert_eq!(parsed.raster_y_size, 768);
        assert_eq!(parsed.srs, Some("EPSG:4326".to_string()));
    }
}
