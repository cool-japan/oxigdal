//! VRT XML format parser and writer

use crate::band::{PixelFunction, VrtBand};
use crate::dataset::{VrtDataset, VrtSubclass};
use crate::error::{Result, VrtError};
use crate::source::{PixelRect, SourceFilename, SourceWindow, VrtSource};
use oxigdal_core::types::{ColorInterpretation, GeoTransform, NoDataValue, RasterDataType};
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
        reader.config_mut().trim_text(true);

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

    fn parse_source<R: BufRead>(reader: &mut Reader<R>, start: &BytesStart) -> Result<VrtSource> {
        let mut filename = None;
        let mut source_band = 1usize;
        let mut src_rect = None;
        let mut dst_rect = None;
        let mut buf = Vec::new();
        let element_name = start.name().as_ref().to_vec();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => match e.name().as_ref() {
                    b"SourceFilename" => {
                        let text = Self::parse_text_element(reader, "SourceFilename")?;
                        filename = Some(SourceFilename::absolute(text));
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

        Ok(source)
    }

    /// Parses PixelRect from attributes only (for self-closing tags like `<SrcRect ... />`)
    fn parse_rect_from_empty(start: &BytesStart) -> Result<PixelRect> {
        let mut x_off = 0u64;
        let mut y_off = 0u64;
        let mut x_size = 0u64;
        let mut y_size = 0u64;

        for attr in start.attributes() {
            let attr = attr.map_err(|e| VrtError::xml_parse(format!("Attribute error: {}", e)))?;
            match attr.key.as_ref() {
                b"xOff" => x_off = Self::parse_u64(&attr.value)?,
                b"yOff" => y_off = Self::parse_u64(&attr.value)?,
                b"xSize" => x_size = Self::parse_u64(&attr.value)?,
                b"ySize" => y_size = Self::parse_u64(&attr.value)?,
                _ => {}
            }
        }

        Ok(PixelRect::new(x_off, y_off, x_size, y_size))
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

    fn parse_text_element<R: BufRead>(reader: &mut Reader<R>, name: &str) -> Result<String> {
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

        writer
            .write_event(Event::End(BytesEnd::new("VRTDataset")))
            .map_err(|e| VrtError::xml_parse(format!("Write error: {}", e)))?;

        Ok(())
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

        Self::write_text_element(
            writer,
            "SourceFilename",
            &source.filename.path.display().to_string(),
        )?;
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
