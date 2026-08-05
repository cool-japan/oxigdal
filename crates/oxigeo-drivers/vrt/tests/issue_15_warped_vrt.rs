//! Regression tests for cool-japan/oxigeo#15 — Warped VRT (`<GDALWarpOptions>`)
//! support.
//!
//! The reporter opened a `gdalwarp -of VRT` product: a
//! `VRTDataset subClass="VRTWarpedDataset"` whose bands carry no sources and
//! whose pixels come from a `<GDALWarpOptions>` block naming a **nested** VRT
//! mosaic and an EPSG:4326 → EPSG:3857 reprojection. The fixtures below mirror
//! that structure exactly, down to the full GDAL WKT and the `relativeToVRT`
//! paths.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_core::types::{GeoTransform, NoDataValue, RasterDataType};
use oxigeo_geotiff::writer::{GeoTiffWriter, OverviewResampling, WriterConfig};
use oxigeo_proj::{Coordinate, Transformer};
use oxigeo_vrt::{PixelRect, VrtReader, VrtXmlParser, WarpKernel, WarpResampleAlg};
use std::path::{Path, PathBuf};

/// The exact `<SourceSRS>` of the VRT attached to the issue: a WKT1 tree whose
/// *first* `AUTHORITY` is the spheroid's (7030), not the CRS's.
const ISSUE_15_SOURCE_SRS: &str = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AXIS["Latitude",NORTH],AXIS["Longitude",EAST],AUTHORITY["EPSG","4326"]]"#;

/// The exact `<TargetSRS>` of the VRT attached to the issue.
const ISSUE_15_TARGET_SRS: &str = r#"PROJCS["WGS 84 / Pseudo-Mercator",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]],PROJECTION["Mercator_1SP"],PARAMETER["central_meridian",0],PARAMETER["scale_factor",1],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],EXTENSION["PROJ4","+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +wktext +no_defs"],AUTHORITY["EPSG","3857"]]"#;

const SRC_SIZE: u64 = 64;
const DST_SIZE: u64 = 48;

/// Pixel value of the source raster: a ramp in `x` only, so the expected value
/// at any sample point is an exact function of the source column.
fn source_value(x: u64) -> u8 {
    (20 + x * 3) as u8
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxigeo_issue15_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn src_geo_transform() -> GeoTransform {
    GeoTransform {
        origin_x: 100.0,
        pixel_width: 0.01,
        row_rotation: 0.0,
        origin_y: 20.0,
        col_rotation: 0.0,
        pixel_height: -0.01,
    }
}

/// Writes the single-band EPSG:4326 GeoTIFF the fixtures mosaic and warp.
fn write_source_tif(path: &Path) {
    let config = WriterConfig::new(SRC_SIZE, SRC_SIZE, 1, RasterDataType::UInt8)
        .with_compression(oxigeo_geotiff::tiff::Compression::None)
        .with_predictor(oxigeo_geotiff::tiff::Predictor::None)
        .with_geo_transform(src_geo_transform())
        .with_epsg_code(4326)
        .with_nodata(NoDataValue::Float(0.0))
        .with_overviews(false, OverviewResampling::Nearest);

    let mut data = vec![0u8; (SRC_SIZE * SRC_SIZE) as usize];
    for y in 0..SRC_SIZE {
        for x in 0..SRC_SIZE {
            data[(y * SRC_SIZE + x) as usize] = source_value(x);
        }
    }

    let mut writer =
        GeoTiffWriter::create(path, config, Default::default()).expect("create geotiff writer");
    writer.write(&data).expect("write geotiff");
}

/// The destination geotransform: the EPSG:3857 bounding box of the source,
/// gridded at `DST_SIZE`².
fn dst_geo_transform() -> GeoTransform {
    let forward = Transformer::from_epsg(4326, 3857).expect("4326 -> 3857");
    let gt = src_geo_transform();

    let corners = [
        (gt.origin_x, gt.origin_y),
        (gt.origin_x + gt.pixel_width * SRC_SIZE as f64, gt.origin_y),
        (gt.origin_x, gt.origin_y + gt.pixel_height * SRC_SIZE as f64),
        (
            gt.origin_x + gt.pixel_width * SRC_SIZE as f64,
            gt.origin_y + gt.pixel_height * SRC_SIZE as f64,
        ),
    ];

    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (lon, lat) in corners {
        let c = forward
            .transform(&Coordinate::new(lon, lat))
            .expect("corner transform");
        min_x = min_x.min(c.x);
        max_x = max_x.max(c.x);
        min_y = min_y.min(c.y);
        max_y = max_y.max(c.y);
    }

    GeoTransform {
        origin_x: min_x,
        pixel_width: (max_x - min_x) / DST_SIZE as f64,
        row_rotation: 0.0,
        origin_y: max_y,
        col_rotation: 0.0,
        pixel_height: -(max_y - min_y) / DST_SIZE as f64,
    }
}

fn format_gt(gt: &GeoTransform) -> String {
    format!(
        "{:.17},{:.17},{:.17},{:.17},{:.17},{:.17}",
        gt.origin_x, gt.pixel_width, gt.row_rotation, gt.origin_y, gt.col_rotation, gt.pixel_height
    )
}

/// Writes the raw (non-warped) VRT mosaic that the warped VRT wraps, using a
/// `relativeToVRT="1"` source path exactly as `gdalbuildvrt` does.
fn write_raw_vrt(dir: &Path, tif_name: &str) -> PathBuf {
    let gt = src_geo_transform();
    let xml = format!(
        r#"<VRTDataset rasterXSize="{size}" rasterYSize="{size}">
  <SRS dataAxisToSRSAxisMapping="2,1">{srs}</SRS>
  <GeoTransform>{gt}</GeoTransform>
  <VRTRasterBand dataType="Byte" band="1">
    <NoDataValue>0</NoDataValue>
    <ColorInterp>Gray</ColorInterp>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">{tif}</SourceFilename>
      <SourceBand>1</SourceBand>
      <SourceProperties RasterXSize="{size}" RasterYSize="{size}" DataType="Byte" BlockXSize="{size}" BlockYSize="1" />
      <SrcRect xOff="0" yOff="0" xSize="{size}" ySize="{size}" />
      <DstRect xOff="0" yOff="0" xSize="{size}" ySize="{size}" />
      <NODATA>0</NODATA>
    </ComplexSource>
  </VRTRasterBand>
</VRTDataset>"#,
        size = SRC_SIZE,
        srs = ISSUE_15_SOURCE_SRS,
        gt = format_gt(&gt),
        tif = tif_name,
    );

    let path = dir.join("raw.vrt");
    std::fs::write(&path, xml).expect("write raw vrt");
    path
}

/// Writes the warped VRT, mirroring the attachment's element structure.
fn write_warped_vrt(dir: &Path, source_name: &str, resample: &str) -> PathBuf {
    let dst_gt = dst_geo_transform();
    let xml = format!(
        r#"<VRTDataset rasterXSize="{size}" rasterYSize="{size}" subClass="VRTWarpedDataset">
  <SRS dataAxisToSRSAxisMapping="1,2">{target_srs}</SRS>
  <GeoTransform>{dst_gt}</GeoTransform>
  <Metadata domain="IMAGE_STRUCTURE">
    <MDI key="INTERLEAVE">PIXEL</MDI>
  </Metadata>
  <VRTRasterBand dataType="Byte" band="1" subClass="VRTWarpedRasterBand">
    <NoDataValue>0</NoDataValue>
    <ColorInterp>Gray</ColorInterp>
  </VRTRasterBand>
  <BlockXSize>512</BlockXSize>
  <BlockYSize>128</BlockYSize>
  <GDALWarpOptions>
    <WarpMemoryLimit>6.71089e+07</WarpMemoryLimit>
    <ResampleAlg>{resample}</ResampleAlg>
    <WorkingDataType>Byte</WorkingDataType>
    <Option name="SRS_AXIS_MAPPING_STRATEGY">TRADITIONAL_GIS_ORDER</Option>
    <Option name="INIT_DEST">NO_DATA</Option>
    <Option name="ERROR_OUT_IF_EMPTY_SOURCE_WINDOW">FALSE</Option>
    <SourceDataset relativeToVRT="1">{source}</SourceDataset>
    <Transformer>
      <ApproxTransformer>
        <MaxError>0.125</MaxError>
        <BaseTransformer>
          <GenImgProjTransformer>
            <SrcGeoTransform>{src_gt}</SrcGeoTransform>
            <SrcInvGeoTransform>-10000,100,0,2000,0,-100</SrcInvGeoTransform>
            <DstGeoTransform>{dst_gt}</DstGeoTransform>
            <DstInvGeoTransform>0,1,0,0,0,-1</DstInvGeoTransform>
            <ReprojectTransformer>
              <ReprojectionTransformer>
                <SourceSRS>{source_srs}</SourceSRS>
                <TargetSRS>{target_srs}</TargetSRS>
                <Options>
                  <Option key="CENTER_LONG">100.32</Option>
                </Options>
              </ReprojectionTransformer>
            </ReprojectTransformer>
          </GenImgProjTransformer>
        </BaseTransformer>
      </ApproxTransformer>
    </Transformer>
    <BandList>
      <BandMapping src="1" dst="1">
        <SrcNoDataReal>0</SrcNoDataReal>
        <SrcNoDataImag>0</SrcNoDataImag>
        <DstNoDataReal>0</DstNoDataReal>
        <DstNoDataImag>0</DstNoDataImag>
      </BandMapping>
    </BandList>
  </GDALWarpOptions>
</VRTDataset>"#,
        size = DST_SIZE,
        source_srs = ISSUE_15_SOURCE_SRS,
        target_srs = ISSUE_15_TARGET_SRS,
        src_gt = format_gt(&src_geo_transform()),
        dst_gt = format_gt(&dst_gt),
        source = source_name,
        resample = resample,
    );

    let path = dir.join("warped.vrt");
    std::fs::write(&path, xml).expect("write warped vrt");
    path
}

/// Builds the full fixture: GeoTIFF → raw VRT mosaic → warped VRT.
fn build_fixture(name: &str, resample: &str) -> PathBuf {
    let dir = scratch_dir(name);
    write_source_tif(&dir.join("src.tif"));
    write_raw_vrt(&dir, "src.tif");
    write_warped_vrt(&dir, "raw.vrt", resample)
}

/// The independent reprojection oracle: destination pixel centre → source
/// pixel, computed without the driver so the assertions below are a real
/// cross-check rather than a restatement of the warp engine.
///
/// Built **once per test**, deliberately. `Transformer::from_epsg` is a cheap
/// embedded-registry lookup under this crate's own feature set, but a
/// `--workspace --all-features` build unifies `oxigeo-proj/proj-db` in and
/// routes construction through the PROJ.db reader instead; rebuilding the
/// transformer per pixel turned a 0.1 s test into one that blew past nextest's
/// 120 s `terminate-after`.
struct ReprojectionOracle {
    inverse: Transformer,
    dst_gt: GeoTransform,
    src_gt: GeoTransform,
}

impl ReprojectionOracle {
    fn new() -> Self {
        Self {
            inverse: Transformer::from_epsg(3857, 4326).expect("3857 -> 4326"),
            dst_gt: dst_geo_transform(),
            src_gt: src_geo_transform(),
        }
    }

    /// The source pixel the centre of destination pixel (`dst_col`, `dst_row`)
    /// maps to, or `None` where the inverse projection is undefined.
    fn source_pixel(&self, dst_col: u64, dst_row: u64) -> Option<(f64, f64)> {
        let geo_x = self.dst_gt.origin_x + (dst_col as f64 + 0.5) * self.dst_gt.pixel_width;
        let geo_y = self.dst_gt.origin_y + (dst_row as f64 + 0.5) * self.dst_gt.pixel_height;

        let c = self
            .inverse
            .transform(&Coordinate::new(geo_x, geo_y))
            .ok()?;

        Some((
            (c.x - self.src_gt.origin_x) / self.src_gt.pixel_width,
            (c.y - self.src_gt.origin_y) / self.src_gt.pixel_height,
        ))
    }
}

#[test]
fn test_issue_15_warped_vrt_opens() {
    let vrt = build_fixture("opens", "NearestNeighbour");

    // Before the fix this failed with
    // "Band 1 validation failed: Band must have at least one source or a pixel
    // function", because <GDALWarpOptions> was skipped by the parser and a
    // warped band legitimately has no sources.
    let reader = VrtReader::open(&vrt).expect("warped VRT must open");

    assert!(reader.is_warped(), "subClass + warp block means warped");
    assert_eq!(reader.width(), DST_SIZE);
    assert_eq!(reader.height(), DST_SIZE);
    assert_eq!(reader.band_count(), 1);
    assert!(reader.geo_transform().is_some());
}

#[test]
fn test_issue_15_warp_options_parsed() {
    let vrt = build_fixture("parse", "Bilinear");
    let dataset = VrtXmlParser::parse_file(&vrt).expect("parse warped VRT");

    let warp = dataset.warp_options.expect("<GDALWarpOptions> must parse");
    assert_eq!(warp.resample_alg, WarpResampleAlg::Bilinear);
    assert_eq!(warp.resample_alg.kernel(), WarpKernel::Bilinear);
    assert_eq!(warp.working_data_type, Some(RasterDataType::UInt8));
    assert_eq!(warp.warp_memory_limit, Some(6.71089e+07));

    // Warp-level `<Option name=...>` entries.
    assert_eq!(warp.option("INIT_DEST"), Some("NO_DATA"));
    assert_eq!(
        warp.option("SRS_AXIS_MAPPING_STRATEGY"),
        Some("TRADITIONAL_GIS_ORDER")
    );
    assert_eq!(
        warp.option("ERROR_OUT_IF_EMPTY_SOURCE_WINDOW"),
        Some("FALSE")
    );

    // The source dataset, with its relativeToVRT flag.
    let source = warp.source_dataset.as_ref().expect("<SourceDataset>");
    assert_eq!(source.path.to_string_lossy(), "raw.vrt");
    assert!(
        source.relative_to_vrt,
        "relativeToVRT=\"1\" must be honoured"
    );

    // The transformer chain, reached through ApproxTransformer/BaseTransformer.
    let transformer = warp.transformer.as_ref().expect("<GenImgProjTransformer>");
    assert_eq!(transformer.max_error, Some(0.125));
    let src_gt = transformer.src_geo_transform.expect("<SrcGeoTransform>");
    assert!((src_gt.origin_x - 100.0).abs() < 1e-9);
    assert!((src_gt.pixel_width - 0.01).abs() < 1e-12);
    assert!(transformer.dst_geo_transform.is_some());

    let reprojection = transformer
        .reprojection
        .as_ref()
        .expect("<ReprojectionTransformer>");
    assert_eq!(
        reprojection.source_srs.as_deref(),
        Some(ISSUE_15_SOURCE_SRS)
    );
    assert_eq!(
        reprojection.target_srs.as_deref(),
        Some(ISSUE_15_TARGET_SRS)
    );
    // Reprojection `<Option key=...>` entries are kept apart from warp options.
    assert_eq!(reprojection.option("CENTER_LONG"), Some("100.32"));
    assert!(warp.option("CENTER_LONG").is_none());

    // The band list.
    assert_eq!(warp.band_mappings.len(), 1);
    let mapping = warp.band_mappings[0];
    assert_eq!((mapping.src, mapping.dst), (1, 1));
    assert_eq!(mapping.src_nodata_real, Some(0.0));
    assert_eq!(mapping.dst_nodata_real, Some(0.0));
    assert_eq!(warp.source_band_for(1), 1);
}

#[test]
fn test_issue_15_warped_read_matches_reprojection() {
    let vrt = build_fixture("nearest", "NearestNeighbour");
    let reader = VrtReader::open(&vrt).expect("open warped VRT");

    let buffer = reader
        .read_window(1, PixelRect::new(0, 0, DST_SIZE, DST_SIZE))
        .expect("warped read must succeed");
    let pixels = buffer.as_bytes();
    assert_eq!(pixels.len(), (DST_SIZE * DST_SIZE) as usize);

    let oracle = ReprojectionOracle::new();
    let mut checked = 0usize;
    for row in 0..DST_SIZE {
        for col in 0..DST_SIZE {
            let actual = pixels[(row * DST_SIZE + col) as usize];
            let Some((px, py)) = oracle.source_pixel(col, row) else {
                continue;
            };

            // Only assert well inside the source, where nearest-neighbour
            // cannot be pulled across the edge by sub-pixel rounding.
            if px < 1.0 || py < 1.0 || px >= SRC_SIZE as f64 - 1.0 || py >= SRC_SIZE as f64 - 1.0 {
                continue;
            }

            // Exact: the destination pixel *centre* must map to the source
            // pixel the independent reprojection names. A tolerance of one
            // source column here would be 3 value units — exactly the size of
            // the half-pixel convention error this pins.
            let expected = source_value(px.floor() as u64);
            assert_eq!(
                actual,
                expected,
                "dst ({col},{row}) -> src ({px:.3},{py:.3}) must sample source column {}",
                px.floor() as u64
            );
            checked += 1;
        }
    }

    assert!(
        checked > 500,
        "expected the warp to cover most of the grid, checked only {checked}"
    );
}

#[test]
fn test_issue_15_warped_read_bilinear() {
    let vrt = build_fixture("bilinear", "Bilinear");
    let reader = VrtReader::open(&vrt).expect("open warped VRT");

    let buffer = reader
        .read_window(1, PixelRect::new(0, 0, DST_SIZE, DST_SIZE))
        .expect("bilinear warped read");
    let pixels = buffer.as_bytes();

    let oracle = ReprojectionOracle::new();
    let mut checked = 0usize;
    for row in 0..DST_SIZE {
        for col in 0..DST_SIZE {
            let actual = f64::from(pixels[(row * DST_SIZE + col) as usize]);
            let Some((px, py)) = oracle.source_pixel(col, row) else {
                continue;
            };
            if px < 2.0 || py < 2.0 || px >= SRC_SIZE as f64 - 2.0 || py >= SRC_SIZE as f64 - 2.0 {
                continue;
            }

            // The source ramp is linear in x, so bilinear interpolation has a
            // closed form: 20 + 3 * (px - 0.5), clamped to the sample grid.
            let expected = 20.0 + 3.0 * (px - 0.5);
            assert!(
                (actual - expected).abs() <= 2.0,
                "dst ({col},{row}) -> src px {px:.3}: got {actual}, expected ~{expected:.2}"
            );
            checked += 1;
        }
    }

    assert!(checked > 400, "checked only {checked} pixels");
}

#[test]
fn test_issue_15_warped_windowed_read_matches_full_read() {
    let vrt = build_fixture("window", "NearestNeighbour");
    let reader = VrtReader::open(&vrt).expect("open warped VRT");

    let full = reader
        .read_window(1, PixelRect::new(0, 0, DST_SIZE, DST_SIZE))
        .expect("full read");
    let full = full.as_bytes();

    // A sub-window must be bit-identical to the same region of the full read:
    // the source rectangle is derived per window, so an off-by-one there would
    // show up as a shifted or NoData-filled tile.
    let (wx, wy, ww, wh) = (7u64, 11u64, 13u64, 9u64);
    let sub = reader
        .read_window(1, PixelRect::new(wx, wy, ww, wh))
        .expect("windowed read");
    let sub = sub.as_bytes();

    for row in 0..wh {
        for col in 0..ww {
            let from_full = full[((wy + row) * DST_SIZE + wx + col) as usize];
            let from_sub = sub[(row * ww + col) as usize];
            assert_eq!(
                from_full, from_sub,
                "window pixel ({col},{row}) disagrees with the full read"
            );
        }
    }
}

#[test]
fn test_issue_15_warped_corner_is_nodata() {
    // The EPSG:3857 bounding box of a lat/lon rectangle is not itself the
    // image, so INIT_DEST=NO_DATA must leave the uncovered margin at 0 rather
    // than filling it with edge pixels.
    let vrt = build_fixture("nodata", "NearestNeighbour");
    let reader = VrtReader::open(&vrt).expect("open warped VRT");

    let buffer = reader
        .read_window(1, PixelRect::new(0, 0, DST_SIZE, DST_SIZE))
        .expect("warped read");
    let pixels = buffer.as_bytes();

    // The centre is inside the source and must carry data.
    let centre = pixels[((DST_SIZE / 2) * DST_SIZE + DST_SIZE / 2) as usize];
    assert!(centre > 0, "centre of the warp must not be NoData");

    assert_eq!(buffer.nodata(), NoDataValue::Float(0.0));
}

#[test]
fn test_issue_15_nested_vrt_source_is_read() {
    // The warp source is a VRT, not a GeoTIFF; before the fix SourceDataset
    // only ever tried GeoTiffReader::open and rejected it.
    let dir = scratch_dir("nested");
    write_source_tif(&dir.join("src.tif"));
    let raw = write_raw_vrt(&dir, "src.tif");

    let raw_reader = VrtReader::open(&raw).expect("nested VRT opens on its own");
    assert!(!raw_reader.is_warped());
    let raw_pixels = raw_reader
        .read_window(1, PixelRect::new(0, 0, SRC_SIZE, SRC_SIZE))
        .expect("read raw mosaic through relativeToVRT source");
    assert_eq!(
        raw_pixels.as_bytes()[(10 * SRC_SIZE + 10) as usize],
        source_value(10),
        "relativeToVRT=\"1\" source must resolve against the VRT's directory"
    );

    let warped = write_warped_vrt(&dir, "raw.vrt", "NearestNeighbour");
    let reader = VrtReader::open(&warped).expect("warped VRT over a nested VRT");
    let buffer = reader
        .read_window(1, PixelRect::new(0, 0, DST_SIZE, DST_SIZE))
        .expect("warped read through nested VRT");
    assert!(
        buffer.as_bytes().iter().any(|b| *b > 0),
        "warping a nested VRT must produce data"
    );
}

#[test]
fn test_issue_15_empty_source_window_is_distinguishable() {
    // A warp over a sparse mosaic routinely lands in a gap, which must read as
    // NoData rather than fail. That leniency keys on a dedicated error variant
    // so it cannot also swallow real failures (allocation overflow, internal
    // bounds bugs), which stay `InvalidWindow`.
    let dir = scratch_dir("empty");
    write_source_tif(&dir.join("src.tif"));
    let raw = write_raw_vrt(&dir, "src.tif");

    let reader = VrtReader::open(&raw).expect("open raw vrt");
    let err = reader
        .read_window(1, PixelRect::new(0, 0, SRC_SIZE, SRC_SIZE))
        .err();
    assert!(err.is_none(), "a covered window must read");

    // The mosaic's only source covers the whole raster, so ask past it.
    let dataset = VrtXmlParser::parse_file(&raw).expect("parse");
    let shifted = VrtReader::from_dataset(dataset).expect("reader");
    match shifted.read_window(1, PixelRect::new(SRC_SIZE + 10, SRC_SIZE + 10, 4, 4)) {
        Err(oxigeo_vrt::VrtError::EmptyWindow { .. }) => {}
        other => panic!("expected EmptyWindow, got {other:?}"),
    }
}

#[test]
fn test_issue_15_xml_entities_survive_parsing() {
    // quick-xml reports `&quot;` as its own event, which the parser dropped —
    // so a WKT `<SRS>` written by this crate (whose writer escapes quotes) read
    // back with every `"` missing, silently corrupting the CRS.
    let xml = r#"<VRTDataset rasterXSize="4" rasterYSize="4">
  <SRS>GEOGCS[&quot;WGS 84&quot;,AUTHORITY[&quot;EPSG&quot;,&quot;4326&quot;]]</SRS>
  <VRTRasterBand dataType="Byte" band="1">
    <SimpleSource>
      <SourceFilename relativeToVRT="1">a &amp; b/&#116;ile.tif</SourceFilename>
      <SourceBand>1</SourceBand>
    </SimpleSource>
  </VRTRasterBand>
</VRTDataset>"#;

    let dataset = VrtXmlParser::parse(xml).expect("parse");
    assert_eq!(
        dataset.srs.as_deref(),
        Some(r#"GEOGCS["WGS 84",AUTHORITY["EPSG","4326"]]"#)
    );

    let source = &dataset.bands[0].sources[0];
    // `&amp;` and the numeric char ref `&#116;` (= 't') must both resolve.
    assert_eq!(source.filename.path.to_string_lossy(), "a & b/tile.tif");
    assert!(source.filename.relative_to_vrt);
}

#[test]
fn test_issue_15_warp_options_survive_a_write_roundtrip() {
    // Writing a parsed warped VRT back out used to drop <GDALWarpOptions>
    // entirely, producing a VRTWarpedDataset that described no pixels.
    let vrt = build_fixture("roundtrip", "Bilinear");
    let parsed = VrtXmlParser::parse_file(&vrt).expect("parse");

    let xml = oxigeo_vrt::VrtXmlWriter::write(&parsed).expect("write");
    assert!(xml.contains("<GDALWarpOptions>"), "warp block must be kept");
    assert!(
        xml.contains("relativeToVRT=\"1\""),
        "relative flag must be kept"
    );

    let reparsed = VrtXmlParser::parse(&xml).expect("re-parse");
    // The re-parsed dataset is still a usable warped VRT.
    assert!(reparsed.is_warped());

    let original = parsed.warp_options.expect("original warp options");
    let round = reparsed.warp_options.expect("round-tripped warp options");

    assert_eq!(round.resample_alg, original.resample_alg);
    assert_eq!(round.working_data_type, original.working_data_type);
    assert_eq!(round.option("INIT_DEST"), original.option("INIT_DEST"));
    assert_eq!(round.band_mappings, original.band_mappings);

    let (a, b) = (
        original.transformer.expect("original transformer"),
        round.transformer.expect("round-tripped transformer"),
    );
    assert_eq!(b.max_error, a.max_error);
    let (src_a, src_b) = (
        a.src_geo_transform.expect("src gt"),
        b.src_geo_transform.expect("src gt"),
    );
    // Full precision must survive: a truncated geotransform shifts the raster.
    assert_eq!(src_b.origin_x.to_bits(), src_a.origin_x.to_bits());
    assert_eq!(src_b.pixel_width.to_bits(), src_a.pixel_width.to_bits());

    let dst_a = a.dst_geo_transform.expect("dst gt");
    let dst_b = b.dst_geo_transform.expect("dst gt");
    assert_eq!(dst_b.origin_x.to_bits(), dst_a.origin_x.to_bits());
    assert_eq!(dst_b.pixel_height.to_bits(), dst_a.pixel_height.to_bits());

    let (rep_a, rep_b) = (
        a.reprojection.expect("original reprojection"),
        b.reprojection.expect("round-tripped reprojection"),
    );
    assert_eq!(rep_b.source_srs, rep_a.source_srs);
    assert_eq!(rep_b.target_srs, rep_a.target_srs);
    assert_eq!(rep_b.option("CENTER_LONG"), Some("100.32"));
}

#[test]
fn test_issue_15_warped_vrt_without_warp_options_is_rejected() {
    // A VRTWarpedDataset with no <GDALWarpOptions> describes no pixels at all;
    // it must fail loudly rather than silently reading as zeros.
    let xml = r#"<VRTDataset rasterXSize="16" rasterYSize="16" subClass="VRTWarpedDataset">
  <VRTRasterBand dataType="Byte" band="1" subClass="VRTWarpedRasterBand">
    <NoDataValue>0</NoDataValue>
  </VRTRasterBand>
</VRTDataset>"#;
    let dataset = VrtXmlParser::parse(xml).expect("parse");
    assert!(dataset.warp_options.is_none());
    assert!(!dataset.is_warped());
    assert!(
        VrtReader::from_dataset(dataset).is_err(),
        "a warped VRT with no warp block must be rejected"
    );
}
