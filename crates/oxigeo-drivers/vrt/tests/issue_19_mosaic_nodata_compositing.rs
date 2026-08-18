//! Regression tests for cool-japan/oxigeo#19 — GDAL's nodata-skip compositing
//! for overlapping `<ComplexSource>`s.
//!
//! `gdalbuildvrt` mosaics of real scenes overlap by tens to hundreds of pixels,
//! and each source carries `<NODATA>0</NODATA>`. GDAL applies sources in
//! document order and *skips* any source pixel equal to that source's nodata,
//! so where tile A is nodata and tile B has data, B's pixels survive.
//!
//! oxigeo consulted only the first source covering a pixel and let it claim the
//! pixel unconditionally, so a nodata pixel from tile A masked tile B's valid
//! data — rendering as holes along every tile-overlap band while GDAL rendered
//! the same VRT seamlessly.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_core::types::{GeoTransform, NoDataValue, RasterDataType};
use oxigeo_geotiff::writer::{GeoTiffWriter, OverviewResampling, WriterConfig};
use oxigeo_vrt::{PixelRect, VrtReader, VrtXmlParser};
use std::path::{Path, PathBuf};

/// Both tiles are 8x8 and overlap in a 4-pixel-wide band.
const TILE: u64 = 8;
const OVERLAP: u64 = 4;
/// Mosaic width: two tiles side by side minus the shared overlap.
const MOSAIC_W: u64 = TILE * 2 - OVERLAP;

/// Value tile B contributes inside the overlap.
const B_VALUE: u8 = 200;
/// Value tile A contributes outside the overlap.
const A_VALUE: u8 = 100;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxigeo_issue19_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn geo_transform() -> GeoTransform {
    GeoTransform {
        origin_x: 100.0,
        pixel_width: 0.01,
        row_rotation: 0.0,
        origin_y: 20.0,
        col_rotation: 0.0,
        pixel_height: -0.01,
    }
}

/// Writes an 8x8 Byte GeoTIFF whose pixel values come from `value_at`.
fn write_tile(path: &Path, value_at: impl Fn(u64, u64) -> u8) {
    let config = WriterConfig::new(TILE, TILE, 1, RasterDataType::UInt8)
        .with_compression(oxigeo_geotiff::tiff::Compression::None)
        .with_predictor(oxigeo_geotiff::tiff::Predictor::None)
        .with_geo_transform(geo_transform())
        .with_epsg_code(4326)
        .with_nodata(NoDataValue::Float(0.0))
        .with_overviews(false, OverviewResampling::Nearest);

    let mut data = vec![0u8; (TILE * TILE) as usize];
    for y in 0..TILE {
        for x in 0..TILE {
            data[(y * TILE + x) as usize] = value_at(x, y);
        }
    }

    let mut writer =
        GeoTiffWriter::create(path, config, Default::default()).expect("create geotiff writer");
    writer.write(&data).expect("write geotiff");
}

/// Builds the two-source mosaic. Tile A is placed first (document order) and is
/// **nodata (0) across the whole overlap**; tile B follows and carries valid
/// data there. Under GDAL, the overlap reads back as B's data.
///
/// `declare_nodata` controls whether the sources carry `<NODATA>0</NODATA>`,
/// so the test can show the element is what drives the behaviour.
fn build_mosaic(dir: &Path, declare_nodata: bool) -> PathBuf {
    let tile_a = dir.join("tile_a.tif");
    let tile_b = dir.join("tile_b.tif");

    // Tile A: valid on its left half, nodata across the overlap band on the right.
    write_tile(&tile_a, |x, _| if x < TILE - OVERLAP { A_VALUE } else { 0 });
    // Tile B: valid everywhere.
    write_tile(&tile_b, |_, _| B_VALUE);

    let nodata_el = if declare_nodata {
        "\n      <NODATA>0</NODATA>"
    } else {
        ""
    };

    let vrt = format!(
        r#"<VRTDataset rasterXSize="{MOSAIC_W}" rasterYSize="{TILE}">
  <SRS>EPSG:4326</SRS>
  <VRTRasterBand dataType="Byte" band="1">
    <NoDataValue>0</NoDataValue>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">tile_a.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="{TILE}" ySize="{TILE}" />
      <DstRect xOff="0" yOff="0" xSize="{TILE}" ySize="{TILE}" />{nodata_el}
    </ComplexSource>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">tile_b.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="{TILE}" ySize="{TILE}" />
      <DstRect xOff="{}" yOff="0" xSize="{TILE}" ySize="{TILE}" />{nodata_el}
    </ComplexSource>
  </VRTRasterBand>
</VRTDataset>
"#,
        TILE - OVERLAP
    );

    let path = dir.join("mosaic.vrt");
    std::fs::write(&path, vrt).expect("write vrt");
    path
}

fn read_row(vrt_path: &Path) -> Vec<u8> {
    let mut dataset = VrtXmlParser::parse_file(vrt_path).expect("parse mosaic vrt");
    dataset.vrt_path = Some(vrt_path.to_path_buf());
    let reader = VrtReader::from_dataset(dataset).expect("reader");

    let buffer = reader
        .read_window(1, PixelRect::new(0, 0, MOSAIC_W, 1))
        .expect("read mosaic row");
    buffer.as_bytes().to_vec()
}

/// The core of the issue: in the overlap, tile A is nodata and tile B is valid,
/// so the mosaic must return tile B's pixels — not nodata.
#[test]
fn test_issue_19_overlapping_source_fills_nodata_from_later_source() {
    let dir = scratch_dir("overlap");
    let vrt = build_mosaic(&dir, true);
    let row = read_row(&vrt);

    assert_eq!(row.len() as u64, MOSAIC_W);

    // Left of the overlap: only tile A covers it, and it is valid there.
    for (x, value) in row.iter().enumerate().take((TILE - OVERLAP) as usize) {
        assert_eq!(*value, A_VALUE, "column {x} should come from tile A");
    }

    // The overlap band: tile A is nodata, tile B is valid. GDAL yields B.
    for x in (TILE - OVERLAP)..TILE {
        assert_eq!(
            row[x as usize], B_VALUE,
            "column {x} is in the overlap: tile A is nodata there, so tile B's \
             valid pixel must survive (cool-japan/oxigeo#19)"
        );
    }

    // Right of the overlap: only tile B covers it.
    for x in TILE..MOSAIC_W {
        assert_eq!(
            row[x as usize], B_VALUE,
            "column {x} should come from tile B"
        );
    }

    assert!(
        !row.contains(&0),
        "no pixel may read back as nodata: every column is covered by a source \
         with valid data, got {row:?}"
    );
}

/// A source without its own `<NODATA>` falls back to the nodata the source
/// dataset declares — which is what GDAL compares against — so the overlap
/// still composites correctly.
#[test]
fn test_issue_19_falls_back_to_source_dataset_nodata() {
    let dir = scratch_dir("dataset_nodata");
    let vrt = build_mosaic(&dir, false);
    let row = read_row(&vrt);

    for x in (TILE - OVERLAP)..TILE {
        assert_eq!(
            row[x as usize], B_VALUE,
            "column {x}: tile A has no <NODATA> element, but its GeoTIFF declares \
             nodata 0, so tile B must still win the overlap"
        );
    }
}

/// Document order still decides between two sources that are *both* valid — the
/// nodata skip must not turn `FirstValid` into `LastValid`.
#[test]
fn test_issue_19_first_valid_still_wins_when_both_sources_have_data() {
    let dir = scratch_dir("both_valid");
    let tile_a = dir.join("tile_a.tif");
    let tile_b = dir.join("tile_b.tif");
    write_tile(&tile_a, |_, _| A_VALUE);
    write_tile(&tile_b, |_, _| B_VALUE);

    let vrt = format!(
        r#"<VRTDataset rasterXSize="{MOSAIC_W}" rasterYSize="{TILE}">
  <SRS>EPSG:4326</SRS>
  <VRTRasterBand dataType="Byte" band="1">
    <NoDataValue>0</NoDataValue>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">tile_a.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="{TILE}" ySize="{TILE}" />
      <DstRect xOff="0" yOff="0" xSize="{TILE}" ySize="{TILE}" />
      <NODATA>0</NODATA>
    </ComplexSource>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">tile_b.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="{TILE}" ySize="{TILE}" />
      <DstRect xOff="{}" yOff="0" xSize="{TILE}" ySize="{TILE}" />
      <NODATA>0</NODATA>
    </ComplexSource>
  </VRTRasterBand>
</VRTDataset>
"#,
        TILE - OVERLAP
    );
    let path = dir.join("mosaic.vrt");
    std::fs::write(&path, vrt).expect("write vrt");

    let row = read_row(&path);
    for x in (TILE - OVERLAP)..TILE {
        assert_eq!(
            row[x as usize], A_VALUE,
            "column {x}: both sources are valid, so the first in document order wins"
        );
    }
}

/// `<NODATA>` must actually be parsed off the `<ComplexSource>` — it was
/// previously discarded by the catch-all skip arm.
#[test]
fn test_issue_19_complex_source_nodata_is_parsed() {
    let dir = scratch_dir("parse_nodata");
    let vrt = build_mosaic(&dir, true);
    let dataset = VrtXmlParser::parse_file(&vrt).expect("parse");
    let band = dataset.get_band(0).expect("band 1");

    for (i, source) in band.sources.iter().enumerate() {
        assert_eq!(
            source.nodata,
            Some(NoDataValue::Float(0.0)),
            "source {i} must carry its <NODATA> value"
        );
    }
}

/// A pixel no source covers must read back as the band's NoDataValue, not as a
/// zero that merely happens to look like one.
#[test]
fn test_issue_19_uncovered_pixels_read_as_band_nodata() {
    let dir = scratch_dir("uncovered");
    let tile = dir.join("tile_a.tif");
    write_tile(&tile, |_, _| A_VALUE);

    // A 16-wide mosaic fed by a single 8-wide source leaves columns 8..16 with
    // no contributing source at all.
    let vrt = format!(
        r#"<VRTDataset rasterXSize="16" rasterYSize="{TILE}">
  <SRS>EPSG:4326</SRS>
  <VRTRasterBand dataType="Byte" band="1">
    <NoDataValue>255</NoDataValue>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">tile_a.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="{TILE}" ySize="{TILE}" />
      <DstRect xOff="0" yOff="0" xSize="{TILE}" ySize="{TILE}" />
    </ComplexSource>
  </VRTRasterBand>
</VRTDataset>
"#
    );
    let path = dir.join("mosaic.vrt");
    std::fs::write(&path, vrt).expect("write vrt");

    let mut dataset = VrtXmlParser::parse_file(&path).expect("parse");
    dataset.vrt_path = Some(path.clone());
    let reader = VrtReader::from_dataset(dataset).expect("reader");
    let buffer = reader
        .read_window(1, PixelRect::new(0, 0, 16, 1))
        .expect("read");
    let row = buffer.as_bytes();

    for x in 0..TILE {
        assert_eq!(row[x as usize], A_VALUE, "column {x} is covered");
    }
    for x in TILE..16 {
        assert_eq!(
            row[x as usize], 255,
            "column {x} has no source and must read as the band NoDataValue"
        );
    }
}
