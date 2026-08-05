//! Regression tests for cool-japan/oxigeo#15 — reading a VRT through the
//! `oxigeo::Dataset` facade.
//!
//! The reporter's program (attached to the issue) is a tiler that does exactly
//! this against a `gdalwarp -of VRT` product:
//!
//! ```ignore
//! let ds = Dataset::open(&task.vrt_path)?;
//! let gt = ds.geotransform().context("dataset has no GeoTransform")?;
//! let raster_w = ds.width() as f64;
//! let raw = ds.read_window_interleaved(Some(&bands), xoff, yoff, xsize, ysize)?;
//! let nodata = ds.read_window(0, 0, 0, 1, 1)?.nodata();
//! ```
//!
//! Two facade-level defects broke it:
//!
//! 1. `Dataset::open` routed every `.vrt` through the catch-all arm of
//!    `open_raster`, producing a **zero-filled** `DatasetInfo`: `width()` and
//!    `height()` returned `0`, `band_count()` returned `0`, and
//!    `geotransform()` returned `None` — for a file that states all of them in
//!    plain XML, and with no error raised anywhere.
//! 2. Every pixel-read entry point was gated on `DatasetFormat::GeoTiff`, so
//!    `read_window_interleaved` answered `NotSupported` for a VRT.
//!
//! These tests pin both, for a plain VRT mosaic and for the warped VRT the
//! issue is actually about.

#![cfg(all(feature = "vrt", feature = "geotiff"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo::core_types::types::NoDataValue;
use oxigeo::geotiff::writer::{GeoTiffWriter, OverviewResampling, WriterConfig};
use oxigeo::{Dataset, DatasetFormat, GeoTransform, RasterDataType};
use oxigeo_proj::{Coordinate, Transformer};
use std::path::{Path, PathBuf};

const SRC_SIZE: u64 = 64;
const DST_SIZE: u64 = 48;

fn source_value(x: u64, band: u64) -> u8 {
    (10 + x * 3 + band * 2) as u8
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxigeo_issue15_facade_{name}"));
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

/// A three-band EPSG:4326 GeoTIFF, so the RGB read path the reporter uses is
/// actually exercised.
fn write_source_tif(path: &Path) {
    let config = WriterConfig::new(SRC_SIZE, SRC_SIZE, 3, RasterDataType::UInt8)
        .with_compression(oxigeo::geotiff::tiff::Compression::None)
        .with_predictor(oxigeo::geotiff::tiff::Predictor::None)
        .with_geo_transform(src_geo_transform())
        .with_epsg_code(4326)
        .with_nodata(NoDataValue::Float(0.0))
        .with_overviews(false, OverviewResampling::Nearest);

    // Pixel-interleaved, as the writer expects for a chunky 3-band image.
    let mut data = vec![0u8; (SRC_SIZE * SRC_SIZE * 3) as usize];
    for y in 0..SRC_SIZE {
        for x in 0..SRC_SIZE {
            for band in 0..3u64 {
                data[((y * SRC_SIZE + x) * 3 + band) as usize] = source_value(x, band);
            }
        }
    }

    let mut writer =
        GeoTiffWriter::create(path, config, Default::default()).expect("create geotiff writer");
    writer.write(&data).expect("write geotiff");
}

fn format_gt(gt: &GeoTransform) -> String {
    format!(
        "{:.17},{:.17},{:.17},{:.17},{:.17},{:.17}",
        gt.origin_x, gt.pixel_width, gt.row_rotation, gt.origin_y, gt.col_rotation, gt.pixel_height
    )
}

/// A plain (non-warped) three-band VRT mosaic over the GeoTIFF.
fn write_raw_vrt(dir: &Path) -> PathBuf {
    let gt = src_geo_transform();
    let mut bands = String::new();
    for band in 1..=3 {
        bands.push_str(&format!(
            r#"  <VRTRasterBand dataType="Byte" band="{band}">
    <NoDataValue>0</NoDataValue>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">src.tif</SourceFilename>
      <SourceBand>{band}</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="{size}" ySize="{size}" />
      <DstRect xOff="0" yOff="0" xSize="{size}" ySize="{size}" />
    </ComplexSource>
  </VRTRasterBand>
"#,
            band = band,
            size = SRC_SIZE
        ));
    }

    let xml = format!(
        r#"<VRTDataset rasterXSize="{size}" rasterYSize="{size}">
  <SRS>EPSG:4326</SRS>
  <GeoTransform>{gt}</GeoTransform>
{bands}</VRTDataset>"#,
        size = SRC_SIZE,
        gt = format_gt(&gt),
        bands = bands
    );

    let path = dir.join("raw.vrt");
    std::fs::write(&path, xml).expect("write raw vrt");
    path
}

fn dst_geo_transform() -> GeoTransform {
    let forward = Transformer::from_epsg(4326, 3857).expect("4326 -> 3857");
    let gt = src_geo_transform();
    let (x1, y1) = (
        gt.origin_x + gt.pixel_width * SRC_SIZE as f64,
        gt.origin_y + gt.pixel_height * SRC_SIZE as f64,
    );

    let upper_left = forward
        .transform(&Coordinate::new(gt.origin_x, gt.origin_y))
        .expect("upper left");
    let lower_right = forward
        .transform(&Coordinate::new(x1, y1))
        .expect("lower right");

    GeoTransform {
        origin_x: upper_left.x,
        pixel_width: (lower_right.x - upper_left.x) / DST_SIZE as f64,
        row_rotation: 0.0,
        origin_y: upper_left.y,
        col_rotation: 0.0,
        pixel_height: (lower_right.y - upper_left.y) / DST_SIZE as f64,
    }
}

/// The warped VRT, in the shape `gdalwarp -of VRT` writes.
fn write_warped_vrt(dir: &Path) -> PathBuf {
    let dst_gt = dst_geo_transform();
    let mut bands = String::new();
    let mut mappings = String::new();
    for band in 1..=3 {
        bands.push_str(&format!(
            r#"  <VRTRasterBand dataType="Byte" band="{band}" subClass="VRTWarpedRasterBand">
    <NoDataValue>0</NoDataValue>
  </VRTRasterBand>
"#
        ));
        mappings.push_str(&format!(
            r#"      <BandMapping src="{band}" dst="{band}">
        <SrcNoDataReal>0</SrcNoDataReal>
        <DstNoDataReal>0</DstNoDataReal>
      </BandMapping>
"#
        ));
    }

    let xml = format!(
        r#"<VRTDataset rasterXSize="{size}" rasterYSize="{size}" subClass="VRTWarpedDataset">
  <SRS>EPSG:3857</SRS>
  <GeoTransform>{dst_gt}</GeoTransform>
{bands}  <BlockXSize>512</BlockXSize>
  <BlockYSize>128</BlockYSize>
  <GDALWarpOptions>
    <ResampleAlg>Bilinear</ResampleAlg>
    <WorkingDataType>Byte</WorkingDataType>
    <Option name="INIT_DEST">NO_DATA</Option>
    <SourceDataset relativeToVRT="1">raw.vrt</SourceDataset>
    <Transformer>
      <ApproxTransformer>
        <MaxError>0.125</MaxError>
        <BaseTransformer>
          <GenImgProjTransformer>
            <SrcGeoTransform>{src_gt}</SrcGeoTransform>
            <DstGeoTransform>{dst_gt}</DstGeoTransform>
            <ReprojectTransformer>
              <ReprojectionTransformer>
                <SourceSRS>EPSG:4326</SourceSRS>
                <TargetSRS>EPSG:3857</TargetSRS>
              </ReprojectionTransformer>
            </ReprojectTransformer>
          </GenImgProjTransformer>
        </BaseTransformer>
      </ApproxTransformer>
    </Transformer>
    <BandList>
{mappings}    </BandList>
  </GDALWarpOptions>
</VRTDataset>"#,
        size = DST_SIZE,
        src_gt = format_gt(&src_geo_transform()),
        dst_gt = format_gt(&dst_gt),
        bands = bands,
        mappings = mappings
    );

    let path = dir.join("warped.vrt");
    std::fs::write(&path, xml).expect("write warped vrt");
    path
}

#[test]
fn test_issue_15_facade_reports_plain_vrt_metadata() {
    let dir = scratch_dir("plain_meta");
    write_source_tif(&dir.join("src.tif"));
    let vrt = write_raw_vrt(&dir);

    let ds = Dataset::open(vrt.to_str().expect("utf-8 path")).expect("open VRT");

    assert_eq!(ds.format(), DatasetFormat::Vrt);
    // Before the fix these were 0 / 0 / 0 / None, with no error raised.
    assert_eq!(ds.width(), SRC_SIZE as u32);
    assert_eq!(ds.height(), SRC_SIZE as u32);
    assert_eq!(ds.band_count(), 3);
    assert_eq!(ds.data_type(), Some(RasterDataType::UInt8));

    let gt = ds.geotransform().expect("VRT declares a GeoTransform");
    assert!((gt.origin_x - 100.0).abs() < 1e-9);
    assert!((gt.pixel_width - 0.01).abs() < 1e-12);
    assert!((gt.origin_y - 20.0).abs() < 1e-9);
}

#[test]
fn test_issue_15_facade_reads_plain_vrt_pixels() {
    let dir = scratch_dir("plain_read");
    write_source_tif(&dir.join("src.tif"));
    let vrt = write_raw_vrt(&dir);
    let ds = Dataset::open(vrt.to_str().expect("utf-8 path")).expect("open VRT");

    // The exact call the reporter makes.
    let bands = [0u32, 1, 2];
    let raw: Vec<u8> = ds
        .read_window_interleaved(Some(&bands), 4, 6, 8, 5)
        .expect("read_window_interleaved must work for a VRT");
    assert_eq!(raw.len(), 8 * 5 * 3);

    for row in 0..5u64 {
        for col in 0..8u64 {
            let x = 4 + col;
            for band in 0..3u64 {
                let got = raw[((row * 8 + col) * 3 + band) as usize];
                assert_eq!(
                    got,
                    source_value(x, band),
                    "pixel ({col},{row}) band {band}"
                );
            }
        }
    }

    // The reporter also reads a 1×1 window purely to recover the NoData value.
    let probe = ds.read_window(0, 0, 0, 1, 1).expect("1x1 probe read");
    assert_eq!(probe.nodata(), NoDataValue::Float(0.0));

    // Single-band and whole-band reads too.
    let band0 = ds.read_band(0).expect("read_band");
    assert_eq!(band0.width(), SRC_SIZE);
    assert_eq!(band0.height(), SRC_SIZE);
}

#[test]
fn test_issue_15_facade_reads_warped_vrt() {
    let dir = scratch_dir("warped");
    write_source_tif(&dir.join("src.tif"));
    write_raw_vrt(&dir);
    let warped = write_warped_vrt(&dir);

    let ds = Dataset::open(warped.to_str().expect("utf-8 path")).expect("open warped VRT");

    assert_eq!(ds.width(), DST_SIZE as u32);
    assert_eq!(ds.height(), DST_SIZE as u32);
    assert_eq!(ds.band_count(), 3);

    // The warped VRT's own grid is EPSG:3857, so its origin must be the
    // projected one, not the geographic one it was warped from.
    let gt = ds.geotransform().expect("warped VRT GeoTransform");
    assert!(
        gt.origin_x > 1.0e7,
        "expected a Web Mercator easting, got {}",
        gt.origin_x
    );

    let bands = [0u32, 1, 2];
    let raw: Vec<u8> = ds
        .read_window_interleaved(Some(&bands), 8, 8, 16, 16)
        .expect("warped read through the facade");
    assert_eq!(raw.len(), 16 * 16 * 3);
    assert!(
        raw.iter().any(|v| *v > 0),
        "the warped window must contain data, not just NoData"
    );

    // The three bands differ by a constant offset in the source, and the warp
    // is per-band, so that offset must survive it.
    let mut compared = 0usize;
    for pixel in 0..16 * 16 {
        let (r, g) = (raw[pixel * 3], raw[pixel * 3 + 1]);
        if r == 0 || g == 0 {
            continue; // NoData
        }
        assert!(
            g.abs_diff(r).abs_diff(2) <= 1,
            "band 1 should sit ~2 above band 0, got {r} and {g}"
        );
        compared += 1;
    }
    assert!(compared > 50, "compared only {compared} pixels");
}

#[test]
fn test_issue_15_facade_rejects_unparseable_vrt() {
    // A file that sniffs as a VRT but is not one must be a typed error, never a
    // zero-filled descriptor.
    let dir = scratch_dir("broken");
    let path = dir.join("broken.vrt");
    std::fs::write(&path, b"<VRTDataset rasterXSize=\"4\" rasterYSize=\"4\">").expect("write");

    let result = Dataset::open(path.to_str().expect("utf-8 path"));
    assert!(result.is_err(), "a truncated VRT must not open silently");
}
