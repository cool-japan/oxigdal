//! Shared test-only fixture builders for `info`/`stats`/`merge` unit tests.
//!
//! Every format here is synthesized in-process (no checked-in binary blobs):
//! GeoPackage / PMTiles / GeoParquet go through their crate's own writer,
//! MBTiles is built via the Pure-Rust OxiSQL engine (mirroring
//! `oxigdal-mbtiles/tests/reader_test.rs`), and COPC / JPEG2000 are built
//! byte-for-byte the same way their own crates' unit tests do (those
//! builders are `#[cfg(test)]`-private upstream, so the minimal layouts are
//! duplicated here intentionally).
//!
//! This module only ever compiles under `#[cfg(test)]` (see
//! `commands/mod.rs`), so liberal use of `.expect()` here does not violate
//! the workspace's "no unwrap/expect in production code" policy.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Absolute path to the workspace root (two levels up from this crate).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Resolve a path to a real, checked-in demo fixture (e.g.
/// `demo/cog-viewer/iron-belt.fgb`), relative to the workspace root.
pub(crate) fn demo_fixture(relative: &str) -> PathBuf {
    workspace_root().join(relative)
}

/// Build a unique path inside `std::env::temp_dir()` for a synthesized
/// fixture, so parallel test runs never collide.
pub(crate) fn unique_temp_path(label: &str, extension: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut path = std::env::temp_dir();
    path.push(format!("oxigdal_cli_test_{label}_{pid}_{n}.{extension}"));
    path
}

fn write_temp_file(bytes: &[u8], label: &str, extension: &str) -> PathBuf {
    let path = unique_temp_path(label, extension);
    std::fs::write(&path, bytes).expect("write fixture file");
    path
}

// ── FlatGeobuf ──────────────────────────────────────────────────────────────

/// Build a tiny FlatGeobuf file with a homogeneous `Point`-typed header (3
/// point features) via [`oxigdal_flatgeobuf::FlatGeobufWriterBuilder`].
///
/// Deliberately avoids the checked-in `demo/cog-viewer/iron-belt.fgb`
/// fixture for feature-decoding tests: that file declares a
/// `GeometryCollection` header type but stores heterogeneous per-feature
/// geometries (2 polygons + 4 points). `FlatGeobufReader::read_feature`
/// always decodes using `header.geometry_type`
/// (`crates/oxigdal-drivers/flatgeobuf/src/reader.rs`), so it cannot decode
/// per-feature-typed geometries stored under a `GeometryCollection` header —
/// a pre-existing upstream limitation outside this crate's scope. Header-only
/// reads (feature count, extent, CRS) are unaffected and still use the demo
/// fixture.
pub(crate) fn flatgeobuf_fixture_path() -> PathBuf {
    use oxigdal_core::vector::{Feature, Geometry, Point};
    use oxigdal_flatgeobuf::{FlatGeobufWriterBuilder, GeometryType};
    use std::fs::File;
    use std::io::BufWriter;

    let path = unique_temp_path("fixture", "fgb");
    let file = File::create(&path).expect("create fgb fixture file");
    let buf_writer = BufWriter::new(file);

    let mut writer = FlatGeobufWriterBuilder::new(GeometryType::Point)
        .build(buf_writer)
        .expect("build FlatGeobuf writer");

    for (x, y) in [(139.7, 35.7), (-74.0, 40.7), (2.35, 48.85)] {
        let feature = Feature::new(Geometry::Point(Point::new(x, y)));
        writer.add_feature(&feature).expect("add feature");
    }

    writer.finish().expect("finish FlatGeobuf writer");
    path
}

/// Build a `GeometryCollection`-typed FlatGeobuf file (R-tree index enabled,
/// EPSG:4326 CRS, 4 point features) that mirrors the *shape* of the
/// `.gitignore`-d `demo/cog-viewer/iron-belt.fgb` demo fixture.
///
/// The demo fixture is produced out-of-band by the
/// `create_test_flatgeobuf_samples` example and is not checked in, so it can be
/// missing on a fresh checkout. This synthesized equivalent lets header-level
/// `info`/`stats` tests stay self-contained (exercising the same header-parse,
/// CRS, and R-tree-read paths) when the demo file is absent.
pub(crate) fn geometrycollection_fgb_fixture_path() -> PathBuf {
    use oxigdal_core::vector::{Feature, Geometry, Point};
    use oxigdal_flatgeobuf::FlatGeobufWriterBuilder;
    use oxigdal_flatgeobuf::header::{CrsInfo, GeometryType};
    use std::fs::File;
    use std::io::BufWriter;

    let path = unique_temp_path("gc_fixture", "fgb");
    let file = File::create(&path).expect("create fgb fixture file");
    let buf_writer = BufWriter::new(file);

    let mut writer = FlatGeobufWriterBuilder::new(GeometryType::GeometryCollection)
        .with_index()
        .with_crs(CrsInfo::from_epsg(4326))
        .build(buf_writer)
        .expect("build FlatGeobuf writer");

    for (x, y) in [(-2.9, 43.2), (-3.4, 42.7), (-2.4, 44.2), (-4.4, 43.7)] {
        let feature = Feature::new(Geometry::Point(Point::new(x, y)));
        writer.add_feature(&feature).expect("add feature");
    }

    writer.finish().expect("finish FlatGeobuf writer");
    path
}

// ── GeoPackage ──────────────────────────────────────────────────────────────

/// Build a tiny valid GeoPackage (EPSG:4326, one point feature table with 3
/// rows) in-memory via [`oxigdal_gpkg::GeoPackageBuilder`].
pub(crate) fn gpkg_fixture_bytes() -> Vec<u8> {
    oxigdal_gpkg::GeoPackageBuilder::new(4326)
        .add_feature_table(
            "cities",
            "POINT",
            vec![(1, 139.7, 35.7), (2, -74.0, 40.7), (3, 2.35, 48.85)],
        )
        .build()
        .expect("build GeoPackage fixture")
}

/// Write [`gpkg_fixture_bytes`] to a fresh temp file and return its path.
pub(crate) fn gpkg_fixture_path() -> PathBuf {
    write_temp_file(&gpkg_fixture_bytes(), "fixture", "gpkg")
}

// ── MBTiles ─────────────────────────────────────────────────────────────────

/// Build a real on-disk `.mbtiles` (SQLite) archive using the same
/// Pure-Rust OxiSQL engine as `oxigdal_mbtiles::MBTilesReader`, mirroring
/// `oxigdal-mbtiles/tests/reader_test.rs::build_test_mbtiles`.
///
/// Schema: `metadata(name, value)` + `tiles(zoom_level, tile_column,
/// tile_row, tile_data)`, with 3 tiles across zoom levels 0-1.
pub(crate) fn mbtiles_fixture_path() -> PathBuf {
    use oxisql_core::{Connection, ToSqlValue};
    use oxisql_sqlite_compat::SqliteConnection;

    let path = unique_temp_path("mbtiles", "mbtiles");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let path_str = path.to_string_lossy().into_owned();
    let conn = rt
        .block_on(SqliteConnection::open(&path_str))
        .expect("open sqlite connection");

    rt.block_on(conn.execute_batch(
        "CREATE TABLE metadata (name TEXT, value TEXT);
         CREATE TABLE tiles (
             zoom_level  INTEGER,
             tile_column INTEGER,
             tile_row    INTEGER,
             tile_data   BLOB
         );",
    ))
    .expect("create schema");

    for (name, value) in &[
        ("name", "oxigdal-cli-fixture"),
        ("format", "png"),
        ("bounds", "-180,-85,180,85"),
        ("minzoom", "0"),
        ("maxzoom", "1"),
    ] {
        rt.block_on(conn.execute(
            "INSERT INTO metadata (name, value) VALUES ($1, $2)",
            &[name as &dyn ToSqlValue, value as &dyn ToSqlValue],
        ))
        .expect("insert metadata row");
    }

    let tiles: &[(i64, i64, i64, Vec<u8>)] = &[
        (0, 0, 0, vec![0x89, 0x50, 0x4e, 0x47]),
        (1, 0, 0, vec![0x01, 0x02, 0x03, 0x04]),
        (1, 1, 1, vec![0x05, 0x06, 0x07, 0x08]),
    ];
    for (z, col, row, blob) in tiles {
        let blob_owned: Vec<u8> = blob.clone();
        rt.block_on(conn.execute(
            "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES ($1, $2, $3, $4)",
            &[z as &dyn ToSqlValue, col, row, &blob_owned],
        ))
        .expect("insert tile row");
    }

    // Force a WAL checkpoint so the on-disk file is self-contained before a
    // separate process (or reader instance) opens it.
    rt.block_on(conn.execute_batch("PRAGMA wal_checkpoint"))
        .expect("checkpoint wal");

    path
}

// ── GeoParquet ──────────────────────────────────────────────────────────────

/// Build a tiny GeoParquet file (WGS84, WKB-encoded points, no attribute
/// columns — the writer's `add_row`/`add_field` attribute path is not yet
/// implemented upstream) via [`oxigdal_geoparquet::GeoParquetWriter`].
pub(crate) fn geoparquet_fixture_path() -> PathBuf {
    use oxigdal_geoparquet::geometry::{Geometry, Point};
    use oxigdal_geoparquet::{Crs, GeoParquetWriter, GeometryColumnMetadata};

    let path = unique_temp_path("fixture", "parquet");
    let column_metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
    let mut writer = GeoParquetWriter::new(&path, "geometry", column_metadata)
        .expect("create GeoParquet writer");

    for (x, y) in [(139.7, 35.7), (-74.0, 40.7), (2.35, 48.85), (0.0, 0.0)] {
        writer
            .add_geometry(&Geometry::Point(Point::new_2d(x, y)))
            .expect("add geometry");
    }

    writer.finish().expect("finish GeoParquet writer");
    path
}

// ── PMTiles ─────────────────────────────────────────────────────────────────

/// Build a tiny PMTiles v3 archive (2 zoom levels, 3 raster tiles) via
/// [`oxigdal_pmtiles::PmTilesBuilder`].
pub(crate) fn pmtiles_fixture_bytes() -> Vec<u8> {
    use oxigdal_pmtiles::{PmTilesBuilder, TileType};

    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 1);
    builder
        .add_tile(0, 0, 0, &[0x89, 0x50, 0x4e, 0x47])
        .expect("add tile 0/0/0");
    builder
        .add_tile(1, 0, 0, &[0x01, 0x02, 0x03, 0x04])
        .expect("add tile 1/0/0");
    builder
        .add_tile(1, 1, 1, &[0x05, 0x06, 0x07, 0x08])
        .expect("add tile 1/1/1");
    builder.auto_bounds();

    builder.build().expect("build PMTiles fixture")
}

/// Write [`pmtiles_fixture_bytes`] to a fresh temp file and return its path.
pub(crate) fn pmtiles_fixture_path() -> PathBuf {
    write_temp_file(&pmtiles_fixture_bytes(), "fixture", "pmtiles")
}

// ── JPEG2000 (raw J2K codestream) ────────────────────────────────────────────

/// Build a minimal valid J2K codestream: SOC + SIZ + COD + QCD + SOT + SOD +
/// EOC, 4x4 grayscale, 1 code-block, zero packet data. Byte-for-byte the
/// same layout as
/// `oxigdal-jpeg2000/src/reader.rs#build_minimal_j2k_4x4_grayscale` (that
/// helper is `#[cfg(test)]`-private upstream).
pub(crate) fn j2k_fixture_bytes() -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();

    // SOC: 0xFF4F
    out.extend_from_slice(&[0xFF, 0x4F]);

    // SIZ marker: 0xFF51, Lsiz = 41
    out.extend_from_slice(&[0xFF, 0x51]);
    out.extend_from_slice(&[0x00, 0x29]);
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
    out.push(0x07); // Ssiz: signed=0, precision-1=7 -> 8-bit unsigned
    out.push(0x01); // XRsiz = 1
    out.push(0x01); // YRsiz = 1

    // COD marker: 0xFF52, Lcod = 12
    out.extend_from_slice(&[0xFF, 0x52]);
    out.extend_from_slice(&[0x00, 0x0C]);
    out.push(0x00); // Scod
    out.push(0x00); // progression order = LRCP
    out.extend_from_slice(&[0x00, 0x01]); // num_layers = 1
    out.push(0x00); // mct = 0
    out.push(0x00); // num_levels = 0
    out.push(0x02); // xcb
    out.push(0x02); // ycb
    out.push(0x00); // code-block style
    out.push(0x01); // wavelet = 1 -> 5/3 reversible

    // QCD marker: 0xFF5C, Lqcd = 4
    out.extend_from_slice(&[0xFF, 0x5C]);
    out.extend_from_slice(&[0x00, 0x04]);
    out.push(0x00); // Sqcd = 0 (no quantization)
    out.push(0x00); // step size for LL subband

    // SOT marker: 0xFF90, Lsot = 10
    out.extend_from_slice(&[0xFF, 0x90]);
    out.extend_from_slice(&[0x00, 0x0A]);
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

/// Write [`j2k_fixture_bytes`] to a fresh temp file and return its path.
pub(crate) fn j2k_fixture_path() -> PathBuf {
    write_temp_file(&j2k_fixture_bytes(), "fixture", "jp2")
}

// ── COPC ────────────────────────────────────────────────────────────────────

/// Build a 227-byte LAS 1.2 public header with the given point count and
/// XYZ bounds. Layout mirrors `oxigdal-copc/tests/copc_test.rs#make_las_header`.
fn make_las_header(number_of_points: u32, bounds: ([f64; 3], [f64; 3])) -> Vec<u8> {
    let (min, max) = bounds;
    let mut data = vec![0u8; 227];
    data[0..4].copy_from_slice(b"LASF");
    data[24] = 1; // major
    data[25] = 2; // minor
    data[94..96].copy_from_slice(&227u16.to_le_bytes()); // header_size
    // offset_to_point_data is not consulted by `CopcReader::from_bytes` (VLR
    // chain walking uses header_size + per-VLR lengths instead), so an
    // approximate value here is fine.
    data[96..100].copy_from_slice(&227u32.to_le_bytes());
    data[100..104].copy_from_slice(&2u32.to_le_bytes()); // number_of_vlrs
    data[104] = 6; // point_data_format_id
    data[105..107].copy_from_slice(&30u16.to_le_bytes()); // point_data_record_length
    data[107..111].copy_from_slice(&number_of_points.to_le_bytes()); // legacy point count

    let scale = 0.001f64.to_le_bytes();
    data[131..139].copy_from_slice(&scale);
    data[139..147].copy_from_slice(&scale);
    data[147..155].copy_from_slice(&scale);

    data[179..187].copy_from_slice(&max[0].to_le_bytes());
    data[187..195].copy_from_slice(&min[0].to_le_bytes());
    data[195..203].copy_from_slice(&max[1].to_le_bytes());
    data[203..211].copy_from_slice(&min[1].to_le_bytes());
    data[211..219].copy_from_slice(&max[2].to_le_bytes());
    data[219..227].copy_from_slice(&min[2].to_le_bytes());
    data
}

/// Build a 160-byte `CopcInfo` VLR body (center/halfsize/spacing plus a
/// hierarchy pointer; the pointer target need not resolve to real hierarchy
/// page bytes since `CopcReader::from_bytes` only checks that the hierarchy
/// VLR *exists*, not that it decodes).
fn make_copc_info_payload() -> Vec<u8> {
    let mut data = vec![0u8; 160];
    data[0..8].copy_from_slice(&50.0f64.to_le_bytes()); // center_x
    data[8..16].copy_from_slice(&50.0f64.to_le_bytes()); // center_y
    data[16..24].copy_from_slice(&25.0f64.to_le_bytes()); // center_z
    data[24..32].copy_from_slice(&50.0f64.to_le_bytes()); // halfsize
    data[32..40].copy_from_slice(&1.0f64.to_le_bytes()); // spacing
    data[40..48].copy_from_slice(&500u64.to_le_bytes()); // root_hier_offset
    data[48..56].copy_from_slice(&16u64.to_le_bytes()); // root_hier_size
    data
}

/// Append a classic LAS VLR (54-byte header + payload) to `buf`.
fn append_vlr(buf: &mut Vec<u8>, user_id: &str, record_id: u16, payload: &[u8]) {
    buf.extend_from_slice(&[0u8; 2]); // reserved
    let mut uid_buf = [0u8; 16];
    let uid_bytes = user_id.as_bytes();
    let len = uid_bytes.len().min(16);
    uid_buf[..len].copy_from_slice(&uid_bytes[..len]);
    buf.extend_from_slice(&uid_buf);
    buf.extend_from_slice(&record_id.to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    buf.extend_from_slice(&[0u8; 32]); // description
    buf.extend_from_slice(payload);
}

/// Build a minimal but fully valid COPC file: LAS 1.2 header + COPC info VLR
/// (record 1) + hierarchy VLR (record 1000, opaque payload).
///
/// `CopcReader::from_bytes` only parses the header and the two VLRs — it
/// does not traverse the octree hierarchy — so `point_count`/`bounds` are
/// exactly what callers reading `header()` will observe.
pub(crate) fn copc_fixture_bytes(point_count: u32, bounds: ([f64; 3], [f64; 3])) -> Vec<u8> {
    let mut data = make_las_header(point_count, bounds);
    append_vlr(&mut data, "copc", 1, &make_copc_info_payload());
    append_vlr(&mut data, "copc", 1000, &[0u8; 16]);
    data
}

/// Write a [`copc_fixture_bytes`] archive to a fresh temp file and return
/// its path.
pub(crate) fn copc_fixture_path(point_count: u32, bounds: ([f64; 3], [f64; 3])) -> PathBuf {
    write_temp_file(&copc_fixture_bytes(point_count, bounds), "fixture", "copc")
}
