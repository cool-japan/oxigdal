//! Integration tests for VRT driver

#![allow(clippy::expect_used)]

use oxigdal_core::types::{GeoTransform, RasterDataType};
use oxigdal_vrt::{
    MosaicBuilder, PixelRect, SourceFilename, SourceWindow, VrtBand, VrtBuilder, VrtDataset,
    VrtSource, VrtXmlParser, VrtXmlWriter,
};

#[test]
fn test_vrt_builder_simple() {
    let result = VrtBuilder::with_size(512, 512).add_source("/test.tif", 1, 1);

    assert!(result.is_ok());
    let builder = result.expect("Should create builder");
    let dataset = builder.build();
    assert!(dataset.is_ok());

    let ds = dataset.expect("Should build dataset");
    assert_eq!(ds.raster_x_size, 512);
    assert_eq!(ds.raster_y_size, 512);
    assert_eq!(ds.band_count(), 1);
}

#[test]
fn test_vrt_builder_multi_tile() {
    let result = VrtBuilder::new()
        .add_tile("/tile1.tif", 0, 0, 256, 256)
        .and_then(|b| b.add_tile("/tile2.tif", 256, 0, 256, 256))
        .and_then(|b| b.add_tile("/tile3.tif", 0, 256, 256, 256))
        .and_then(|b| b.add_tile("/tile4.tif", 256, 256, 256, 256))
        .and_then(|b| b.set_dimensions(512, 512).build());

    assert!(result.is_ok());
    let ds = result.expect("Should build");
    assert_eq!(ds.raster_x_size, 512);
    assert_eq!(ds.raster_y_size, 512);
}

#[test]
fn test_mosaic_builder() {
    let result = MosaicBuilder::new(256, 256)
        .add_tile("/tile1.tif")
        .and_then(|b| b.next_column().add_tile("/tile2.tif"))
        .and_then(|b| b.next_row().add_tile("/tile3.tif"))
        .and_then(|b| b.next_column().add_tile("/tile4.tif"))
        .and_then(|b| b.build());

    assert!(result.is_ok());
    let ds = result.expect("Should build");
    assert_eq!(ds.raster_x_size, 512);
    assert_eq!(ds.raster_y_size, 512);
}

#[test]
fn test_vrt_xml_roundtrip() {
    // Create a dataset
    let mut dataset = VrtDataset::new(1024, 768);
    dataset = dataset.with_srs("EPSG:4326");

    let geo_transform = GeoTransform {
        origin_x: -180.0,
        pixel_width: 0.1,
        row_rotation: 0.0,
        origin_y: 90.0,
        col_rotation: 0.0,
        pixel_height: -0.1,
    };
    dataset = dataset.with_geo_transform(geo_transform);

    let source = VrtSource::simple("/test.tif", 1);
    let band = VrtBand::simple(1, RasterDataType::UInt8, source);
    dataset.add_band(band);

    // Write to XML
    let xml = VrtXmlWriter::write(&dataset);
    assert!(xml.is_ok());
    let xml_str = xml.expect("Should write");

    // Parse back
    let parsed = VrtXmlParser::parse(&xml_str);
    assert!(parsed.is_ok());
    let parsed_ds = parsed.expect("Should parse");

    // Verify
    assert_eq!(parsed_ds.raster_x_size, 1024);
    assert_eq!(parsed_ds.raster_y_size, 768);
    assert_eq!(parsed_ds.band_count(), 1);
    assert_eq!(parsed_ds.srs, Some("EPSG:4326".to_string()));
    assert!(parsed_ds.geo_transform.is_some());
}

#[test]
fn test_vrt_xml_parsing() {
    let xml = r#"<?xml version="1.0"?>
<VRTDataset rasterXSize="512" rasterYSize="512">
  <SRS>EPSG:4326</SRS>
  <GeoTransform>0.0, 1.0, 0.0, 0.0, 0.0, -1.0</GeoTransform>
  <VRTRasterBand band="1" dataType="Byte">
    <NoDataValue>0</NoDataValue>
    <ColorInterp>Gray</ColorInterp>
    <SimpleSource>
      <SourceFilename>/path/to/source.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="512" ySize="512" />
      <DstRect xOff="0" yOff="0" xSize="512" ySize="512" />
    </SimpleSource>
  </VRTRasterBand>
</VRTDataset>"#;

    let result = VrtXmlParser::parse(xml);
    assert!(result.is_ok());

    let ds = result.expect("Should parse");
    assert_eq!(ds.raster_x_size, 512);
    assert_eq!(ds.raster_y_size, 512);
    assert_eq!(ds.band_count(), 1);
    assert_eq!(ds.srs, Some("EPSG:4326".to_string()));

    let band = ds.get_band(0).expect("Should have band");
    assert_eq!(band.band, 1);
    assert_eq!(band.data_type, RasterDataType::UInt8);
    assert_eq!(band.sources.len(), 1);

    let source = &band.sources[0];
    assert_eq!(source.source_band, 1);
    assert!(source.window.is_some());
}

#[test]
fn test_multi_band_vrt() {
    let mut dataset = VrtDataset::new(1024, 1024);

    // Band 1: Red
    let red_source = VrtSource::new(SourceFilename::absolute("/data/red.tif"), 1);
    let red_band = VrtBand::simple(1, RasterDataType::UInt8, red_source);
    dataset.add_band(red_band);

    // Band 2: Green
    let green_source = VrtSource::new(SourceFilename::absolute("/data/green.tif"), 1);
    let green_band = VrtBand::simple(2, RasterDataType::UInt8, green_source);
    dataset.add_band(green_band);

    // Band 3: Blue
    let blue_source = VrtSource::new(SourceFilename::absolute("/data/blue.tif"), 1);
    let blue_band = VrtBand::simple(3, RasterDataType::UInt8, blue_source);
    dataset.add_band(blue_band);

    assert_eq!(dataset.band_count(), 3);
    assert!(dataset.validate().is_ok());
    assert!(dataset.has_uniform_data_type());

    // Test XML roundtrip
    let xml = VrtXmlWriter::write(&dataset).expect("Should write");
    let parsed = VrtXmlParser::parse(&xml).expect("Should parse");
    assert_eq!(parsed.band_count(), 3);
}

#[test]
fn test_vrt_with_windows() {
    let mut dataset = VrtDataset::new(1024, 1024);

    let src_rect = PixelRect::new(0, 0, 512, 512);
    let dst_rect = PixelRect::new(100, 100, 512, 512);
    let window = SourceWindow::new(src_rect, dst_rect);

    let source = VrtSource::simple("/test.tif", 1).with_window(window);
    let band = VrtBand::simple(1, RasterDataType::UInt8, source);
    dataset.add_band(band);

    assert!(dataset.validate().is_ok());

    // Test XML roundtrip
    let xml = VrtXmlWriter::write(&dataset).expect("Should write");
    let parsed = VrtXmlParser::parse(&xml).expect("Should parse");

    let parsed_band = parsed.get_band(0).expect("Should have band");
    let parsed_source = &parsed_band.sources[0];
    assert!(parsed_source.window.is_some());

    if let Some(ref win) = parsed_source.window {
        assert_eq!(win.dst_rect.x_off, 100);
        assert_eq!(win.dst_rect.y_off, 100);
    }
}

#[test]
fn test_vrt_tile_grid() {
    let paths = vec![
        "/tiles/tile_0_0.tif",
        "/tiles/tile_1_0.tif",
        "/tiles/tile_0_1.tif",
        "/tiles/tile_1_1.tif",
    ];

    let result = VrtBuilder::new()
        .add_tile_grid(&paths, 256, 256, 2)
        .and_then(|b| b.set_dimensions(512, 512).build());

    assert!(result.is_ok());
    let ds = result.expect("Should build");
    assert_eq!(ds.raster_x_size, 512);
    assert_eq!(ds.raster_y_size, 512);

    let band = ds.get_band(0).expect("Should have band");
    assert_eq!(band.sources.len(), 4);

    // Verify tile positions
    for (idx, source) in band.sources.iter().enumerate() {
        let col = idx % 2;
        let row = idx / 2;
        let expected_x = col as u64 * 256;
        let expected_y = row as u64 * 256;

        if let Some(ref window) = source.window {
            assert_eq!(window.dst_rect.x_off, expected_x);
            assert_eq!(window.dst_rect.y_off, expected_y);
        }
    }
}

#[test]
fn test_pixel_rect_operations() {
    let rect1 = PixelRect::new(0, 0, 100, 100);
    let rect2 = PixelRect::new(50, 50, 100, 100);

    assert!(rect1.is_valid());
    assert!(rect2.is_valid());

    assert!(rect1.contains(50, 50));
    assert!(!rect1.contains(150, 150));

    assert!(rect1.intersects(&rect2));

    let intersection = rect1.intersect(&rect2).expect("Should intersect");
    assert_eq!(intersection.x_off, 50);
    assert_eq!(intersection.y_off, 50);
    assert_eq!(intersection.x_size, 50);
    assert_eq!(intersection.y_size, 50);

    let rect3 = PixelRect::new(200, 200, 100, 100);
    assert!(!rect1.intersects(&rect3));
    assert!(rect1.intersect(&rect3).is_none());
}

#[test]
fn test_source_validation() {
    let source = VrtSource::simple("/test.tif", 1);
    assert!(source.validate().is_ok());

    let invalid_source = VrtSource::simple("/test.tif", 0);
    assert!(invalid_source.validate().is_err());
}

#[test]
fn test_band_validation() {
    let source = VrtSource::simple("/test.tif", 1);
    let band = VrtBand::simple(1, RasterDataType::UInt8, source);
    assert!(band.validate().is_ok());

    let invalid_band = VrtBand::new(0, RasterDataType::UInt8);
    assert!(invalid_band.validate().is_err());

    let no_source_band = VrtBand::new(1, RasterDataType::UInt8);
    assert!(no_source_band.validate().is_err());
}

#[test]
fn test_dataset_validation() {
    let mut dataset = VrtDataset::new(512, 512);
    let source = VrtSource::simple("/test.tif", 1);
    let band = VrtBand::simple(1, RasterDataType::UInt8, source);
    dataset.add_band(band);
    assert!(dataset.validate().is_ok());

    let empty_dataset = VrtDataset::new(512, 512);
    assert!(empty_dataset.validate().is_err());

    let zero_size = VrtDataset::new(0, 0);
    assert!(zero_size.validate().is_err());
}

#[test]
fn test_geo_transform_roundtrip() {
    let gt = GeoTransform {
        origin_x: -123.456,
        pixel_width: 0.001,
        row_rotation: 0.0,
        origin_y: 45.678,
        col_rotation: 0.0,
        pixel_height: -0.001,
    };

    let mut dataset = VrtDataset::new(1000, 1000);
    dataset = dataset.with_geo_transform(gt);

    let source = VrtSource::simple("/test.tif", 1);
    let band = VrtBand::simple(1, RasterDataType::Float32, source);
    dataset.add_band(band);

    let xml = VrtXmlWriter::write(&dataset).expect("Should write");
    let parsed = VrtXmlParser::parse(&xml).expect("Should parse");

    assert!(parsed.geo_transform.is_some());
    let parsed_gt = parsed.geo_transform.expect("Should have transform");
    assert!((parsed_gt.origin_x - gt.origin_x).abs() < 0.0001);
    assert!((parsed_gt.pixel_width - gt.pixel_width).abs() < 0.0001);
}

#[test]
fn test_is_vrt_detection() {
    let vrt_xml = b"<?xml version=\"1.0\"?>\n<VRTDataset rasterXSize=\"512\" rasterYSize=\"512\">";
    assert!(oxigdal_vrt::is_vrt(vrt_xml));

    let vrt_no_decl = b"<VRTDataset rasterXSize=\"512\" rasterYSize=\"512\">";
    assert!(oxigdal_vrt::is_vrt(vrt_no_decl));

    let not_vrt = b"GIF89a";
    assert!(!oxigdal_vrt::is_vrt(not_vrt));

    let tiff = b"\x49\x49\x2A\x00";
    assert!(!oxigdal_vrt::is_vrt(tiff));

    let short = b"<VRT";
    assert!(!oxigdal_vrt::is_vrt(short));
}
