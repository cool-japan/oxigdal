//! Regression tests for cool-japan/oxigeo#18 — the numeric formats GDAL
//! actually writes into VRT XML.
//!
//! `gdalbuildvrt` / `gdalwarp -of VRT` keep source and destination windows as
//! doubles and only round them at rasterisation time, so real-world VRTs carry
//! sub-pixel `SrcRect` / `DstRect` values such as `xOff="9783.50000000003"` or
//! `xSize="9889.75000000021"`. Parsing those attributes with `str::parse::<u64>`
//! rejected the file with `Invalid u64: invalid digit found in string`, which
//! made every GDAL-produced mosaic unreadable.
//!
//! The reporter also suspected `<WarpMemoryLimit>6.71089e+07</WarpMemoryLimit>`;
//! that element has been parsed as `f64` since 0.2.3, and
//! `test_issue_18_warp_memory_limit_scientific_notation` pins that so the two
//! offenders cannot be conflated again.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_vrt::VrtXmlParser;

/// A `gdalbuildvrt` mosaic carrying the exact sub-pixel rect values quoted in
/// the issue, including two sources that overlap on the shared edge.
const FRACTIONAL_RECT_MOSAIC: &str = r#"<VRTDataset rasterXSize="97937" rasterYSize="68582">
  <SRS>EPSG:4326</SRS>
  <GeoTransform>1.0e+02, 2.7e-06, 0.0, 2.0e+01, 0.0, -2.7e-06</GeoTransform>
  <VRTRasterBand dataType="Byte" band="1">
    <NoDataValue>0</NoDataValue>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">Laos_tile_L10_1600_623.TIF</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="9889.75000000021" ySize="9883" />
      <DstRect xOff="9783.50000000003" yOff="58698" xSize="9889" ySize="9884" />
      <NODATA>0</NODATA>
    </ComplexSource>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">Thailand_tile_L10_1599_623.TIF</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="9889" ySize="9883" />
      <DstRect xOff="0.499999999452594" yOff="58698" xSize="9889" ySize="9884" />
      <NODATA>0</NODATA>
    </ComplexSource>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">Laos_tile_L10_1601_624.TIF</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="9889" ySize="8446" />
      <DstRect xOff="0.37499999978717" yOff="60135.2500000001" xSize="9889" ySize="8446" />
      <NODATA>0</NODATA>
    </ComplexSource>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">Laos_tile_L10_1602_623.TIF</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="9889.75000000021" ySize="9883" />
      <DstRect xOff="88047.2499999998" yOff="9783" xSize="9889.75000000022" ySize="9883" />
      <NODATA>0</NODATA>
    </ComplexSource>
  </VRTRasterBand>
</VRTDataset>
"#;

/// The `<GDALWarpOptions>` block `gdalwarp -of VRT` emits, with the default
/// 64 MB warp memory limit serialised in `%g` scientific notation.
const WARPED_VRT: &str = r#"<VRTDataset rasterXSize="512" rasterYSize="512" subClass="VRTWarpedDataset">
  <SRS>EPSG:3857</SRS>
  <GeoTransform>1.1e+07, 3.0e+00, 0.0, 2.3e+06, 0.0, -3.0e+00</GeoTransform>
  <VRTRasterBand dataType="Byte" band="1" subClass="VRTWarpedRasterBand" />
  <BlockXSize>512</BlockXSize>
  <BlockYSize>128</BlockYSize>
  <GDALWarpOptions>
    <WarpMemoryLimit>6.71089e+07</WarpMemoryLimit>
    <ResampleAlg>Bilinear</ResampleAlg>
    <WorkingDataType>Byte</WorkingDataType>
    <SourceDataset relativeToVRT="1">z8_199_113_raw.vrt</SourceDataset>
    <BandList>
      <BandMapping src="1" dst="1" />
    </BandList>
  </GDALWarpOptions>
</VRTDataset>
"#;

/// The core of the issue: fractional `SrcRect` / `DstRect` values must parse.
#[test]
fn test_issue_18_fractional_rects_parse() {
    let dataset = VrtXmlParser::parse(FRACTIONAL_RECT_MOSAIC)
        .expect("fractional SrcRect/DstRect must parse (cool-japan/oxigeo#18)");

    let band = dataset.get_band(0).expect("band 1");
    assert_eq!(
        band.sources.len(),
        4,
        "all four sources must survive parsing"
    );
}

/// Sub-pixel offsets round to the nearest whole pixel.
#[test]
fn test_issue_18_fractional_rects_round_to_nearest() {
    let dataset = VrtXmlParser::parse(FRACTIONAL_RECT_MOSAIC).expect("parse");
    let band = dataset.get_band(0).expect("band 1");

    let dst0 = band.sources[0].dst_rect().expect("source 0 DstRect");
    // 9783.50000000003 rounds up, 58698 is already integral.
    assert_eq!(dst0.x_off, 9784);
    assert_eq!(dst0.y_off, 58698);

    let dst1 = band.sources[1].dst_rect().expect("source 1 DstRect");
    // 0.499999999452594 is below the half-pixel mark and rounds down to 0.
    assert_eq!(dst1.x_off, 0);

    let dst2 = band.sources[2].dst_rect().expect("source 2 DstRect");
    assert_eq!(dst2.x_off, 0);
    // 60135.2500000001 rounds down.
    assert_eq!(dst2.y_off, 60135);
}

/// Rounding is edge-consistent: `xOff` and `xOff + xSize` are each rounded and
/// the size derived from the difference. Rounding the size independently would
/// let a tile whose far edge coincides with its neighbour's near edge round to
/// two different integers and open a one-pixel seam.
#[test]
fn test_issue_18_rect_rounding_is_edge_consistent() {
    let dataset = VrtXmlParser::parse(FRACTIONAL_RECT_MOSAIC).expect("parse");
    let band = dataset.get_band(0).expect("band 1");

    // xOff = 9783.50000000003 (-> 9784), far edge = 19672.50000000003 (-> 19673).
    let dst0 = band.sources[0].dst_rect().expect("source 0 DstRect");
    assert_eq!(dst0.x_off, 9784);
    assert_eq!(dst0.x_size, 19673 - 9784, "size derived from rounded edges");

    // xOff = 88047.2499999998 (-> 88047), far edge = 97937.0 (-> 97937).
    let dst3 = band.sources[3].dst_rect().expect("source 3 DstRect");
    assert_eq!(dst3.x_off, 88047);
    assert_eq!(dst3.x_size, 97937 - 88047);

    // A source whose far edge lands exactly on the raster width must still end
    // there, so the mosaic has no uncovered right-hand column.
    assert_eq!(dst3.x_off + dst3.x_size, dataset.raster_x_size);
}

/// Fractional `SrcRect` sizes round the same way.
#[test]
fn test_issue_18_fractional_src_rect_sizes() {
    let dataset = VrtXmlParser::parse(FRACTIONAL_RECT_MOSAIC).expect("parse");
    let band = dataset.get_band(0).expect("band 1");

    let src0 = band.sources[0].src_rect().expect("source 0 SrcRect");
    assert_eq!(src0.x_off, 0);
    // 0 + 9889.75000000021 rounds to 9890.
    assert_eq!(src0.x_size, 9890);
    assert_eq!(src0.y_size, 9883);
}

/// Plain integer rects keep behaving exactly as before the fix.
#[test]
fn test_issue_18_integer_rects_unchanged() {
    let dataset = VrtXmlParser::parse(FRACTIONAL_RECT_MOSAIC).expect("parse");
    let band = dataset.get_band(0).expect("band 1");

    let src1 = band.sources[1].src_rect().expect("source 1 SrcRect");
    assert_eq!(
        (src1.x_off, src1.y_off, src1.x_size, src1.y_size),
        (0, 0, 9889, 9883)
    );
}

/// A sub-pixel source must not collapse to an empty rect: `PixelRect::intersect`
/// drops zero-sized rects and the source's pixels would vanish from the mosaic.
#[test]
fn test_issue_18_subpixel_source_does_not_collapse() {
    let xml = r#"<VRTDataset rasterXSize="16" rasterYSize="16">
  <VRTRasterBand dataType="Byte" band="1">
    <ComplexSource>
      <SourceFilename relativeToVRT="1">sliver.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="0.25" ySize="0.25" />
      <DstRect xOff="4.1" yOff="4.1" xSize="0.3" ySize="0.3" />
    </ComplexSource>
  </VRTRasterBand>
</VRTDataset>
"#;
    let dataset = VrtXmlParser::parse(xml).expect("parse");
    let band = dataset.get_band(0).expect("band 1");
    let dst = band.sources[0].dst_rect().expect("DstRect");
    assert!(
        dst.is_valid(),
        "sub-pixel source must keep a non-empty rect"
    );
    assert_eq!(dst.x_size, 1);
    assert_eq!(dst.y_size, 1);
}

/// Malformed rect values must still be rejected — accepting floats must not
/// turn a typo into a silently wrong window.
#[test]
fn test_issue_18_invalid_rect_value_is_rejected() {
    let xml = r#"<VRTDataset rasterXSize="16" rasterYSize="16">
  <VRTRasterBand dataType="Byte" band="1">
    <ComplexSource>
      <SourceFilename relativeToVRT="1">a.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="not-a-number" yOff="0" xSize="4" ySize="4" />
      <DstRect xOff="0" yOff="0" xSize="4" ySize="4" />
    </ComplexSource>
  </VRTRasterBand>
</VRTDataset>
"#;
    assert!(
        VrtXmlParser::parse(xml).is_err(),
        "a non-numeric rect attribute must be an error"
    );
}

/// Negative offsets are not representable in the `u64` pixel space; a silent
/// saturation to 0 would misplace the window, so they must be an explicit error.
#[test]
fn test_issue_18_negative_rect_offset_is_rejected() {
    let xml = r#"<VRTDataset rasterXSize="16" rasterYSize="16">
  <VRTRasterBand dataType="Byte" band="1">
    <ComplexSource>
      <SourceFilename relativeToVRT="1">a.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="-4.5" yOff="0" xSize="4" ySize="4" />
      <DstRect xOff="0" yOff="0" xSize="4" ySize="4" />
    </ComplexSource>
  </VRTRasterBand>
</VRTDataset>
"#;
    assert!(
        VrtXmlParser::parse(xml).is_err(),
        "a negative rect offset must be an error, not a saturating cast to 0"
    );
}

/// `<WarpMemoryLimit>` in scientific notation. This was **already** handled as
/// `f64` in 0.2.3 — the test pins it so the real offender (fractional rects)
/// cannot be misattributed again.
#[test]
fn test_issue_18_warp_memory_limit_scientific_notation() {
    let dataset = VrtXmlParser::parse(WARPED_VRT).expect("warped VRT must parse");
    let warp = dataset
        .warp_options
        .as_ref()
        .expect("GDALWarpOptions present");
    assert_eq!(warp.warp_memory_limit, Some(6.71089e+07));
}
