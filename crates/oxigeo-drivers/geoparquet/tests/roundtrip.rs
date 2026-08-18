//! Integration tests for GeoParquet round-trip read/write
#![allow(clippy::panic)]

use arrow_array::{Array, ArrayRef, Int32Array, StringArray};
use arrow_schema::{DataType, Field};
use oxigeo_geoparquet::geometry::{Coordinate, Geometry, LineString, Point, Polygon};
use oxigeo_geoparquet::metadata::{Crs, GeometryColumnMetadata};
use oxigeo_geoparquet::{GeoParquetReader, GeoParquetWriter};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_gpq_roundtrip_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for TempPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn test_roundtrip_points() -> Result<(), Box<dyn std::error::Error>> {
    let path = TempPath::new("test_roundtrip_points.parquet");

    // Create test geometries
    let geometries = vec![
        Geometry::Point(Point::new_2d(-122.4, 37.8)),
        Geometry::Point(Point::new_2d(-118.2, 34.0)),
        Geometry::Point(Point::new_2d(-87.6, 41.9)),
    ];

    // Write geometries
    {
        let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());

        let mut writer = GeoParquetWriter::new(&path, "geometry", metadata)?;

        for geom in &geometries {
            writer.add_geometry(geom)?;
        }

        writer.finish()?;
    }

    // Read geometries back
    {
        let reader = GeoParquetReader::open(&path)?;

        assert_eq!(reader.num_rows(), 3);
        assert_eq!(reader.geometry_column_name(), "geometry");

        let read_geometries = reader.read_geometries(0)?;
        assert_eq!(read_geometries.len(), 3);

        // Verify first point
        if let Geometry::Point(point) = &read_geometries[0] {
            assert!((point.coord.x - (-122.4)).abs() < 1e-10);
            assert!((point.coord.y - 37.8).abs() < 1e-10);
        } else {
            panic!("Expected Point geometry");
        }
    }

    Ok(())
}

#[test]
fn test_roundtrip_linestrings() -> Result<(), Box<dyn std::error::Error>> {
    let path = TempPath::new("test_roundtrip_linestrings.parquet");

    // Create test geometries
    let coords1 = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 1.0),
        Coordinate::new_2d(2.0, 0.0),
    ];
    let linestring1 = LineString::new(coords1);

    let coords2 = vec![
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(20.0, 20.0),
    ];
    let linestring2 = LineString::new(coords2);

    let geometries = vec![
        Geometry::LineString(linestring1),
        Geometry::LineString(linestring2),
    ];

    // Write geometries
    {
        let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());

        let mut writer = GeoParquetWriter::new(&path, "geometry", metadata)?;

        for geom in &geometries {
            writer.add_geometry(geom)?;
        }

        writer.finish()?;
    }

    // Read geometries back
    {
        let reader = GeoParquetReader::open(&path)?;

        assert_eq!(reader.num_rows(), 2);

        let read_geometries = reader.read_geometries(0)?;
        assert_eq!(read_geometries.len(), 2);

        // Verify first linestring
        if let Geometry::LineString(linestring) = &read_geometries[0] {
            assert_eq!(linestring.coords.len(), 3);
            assert!((linestring.coords[0].x - 0.0).abs() < 1e-10);
            assert!((linestring.coords[2].x - 2.0).abs() < 1e-10);
        } else {
            panic!("Expected LineString geometry");
        }
    }

    Ok(())
}

#[test]
fn test_roundtrip_polygons() -> Result<(), Box<dyn std::error::Error>> {
    let path = TempPath::new("test_roundtrip_polygons.parquet");

    // Create test polygon
    let exterior_coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(10.0, 0.0),
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(0.0, 10.0),
        Coordinate::new_2d(0.0, 0.0),
    ];
    let exterior = LineString::new(exterior_coords);

    let hole_coords = vec![
        Coordinate::new_2d(2.0, 2.0),
        Coordinate::new_2d(8.0, 2.0),
        Coordinate::new_2d(8.0, 8.0),
        Coordinate::new_2d(2.0, 8.0),
        Coordinate::new_2d(2.0, 2.0),
    ];
    let hole = LineString::new(hole_coords);

    let polygon = Polygon::new(exterior, vec![hole]);

    let geometries = vec![Geometry::Polygon(polygon)];

    // Write geometries
    {
        let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());

        let mut writer = GeoParquetWriter::new(&path, "geometry", metadata)?;

        for geom in &geometries {
            writer.add_geometry(geom)?;
        }

        writer.finish()?;
    }

    // Read geometries back
    {
        let reader = GeoParquetReader::open(&path)?;

        assert_eq!(reader.num_rows(), 1);

        let read_geometries = reader.read_geometries(0)?;
        assert_eq!(read_geometries.len(), 1);

        // Verify polygon
        if let Geometry::Polygon(polygon) = &read_geometries[0] {
            assert_eq!(polygon.exterior.coords.len(), 5);
            assert_eq!(polygon.interiors.len(), 1);
            assert_eq!(polygon.interiors[0].coords.len(), 5);
        } else {
            panic!("Expected Polygon geometry");
        }
    }

    Ok(())
}

#[test]
fn test_metadata_preservation() -> Result<(), Box<dyn std::error::Error>> {
    let path = TempPath::new("test_metadata.parquet");

    // Write with specific CRS
    {
        let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());

        let mut writer = GeoParquetWriter::new(&path, "geom", metadata)?;

        let point = Geometry::Point(Point::new_2d(0.0, 0.0));
        writer.add_geometry(&point)?;

        writer.finish()?;
    }

    // Verify metadata is preserved
    {
        let reader = GeoParquetReader::open(&path)?;

        let metadata = reader.metadata();
        assert_eq!(metadata.primary_column, "geom");

        let column_meta = metadata.primary_column_metadata()?;
        assert!(column_meta.crs.is_some());
    }

    Ok(())
}

#[test]
fn test_batch_writing() -> Result<(), Box<dyn std::error::Error>> {
    let path = TempPath::new("test_batch.parquet");

    // Write many geometries to test batch flushing
    {
        let metadata = GeometryColumnMetadata::new_wkb();

        let mut writer = GeoParquetWriter::new(&path, "geometry", metadata)?.with_batch_size(100);

        // Add 250 geometries (should trigger multiple batch flushes)
        for i in 0..250 {
            let point = Geometry::Point(Point::new_2d(i as f64, i as f64));
            writer.add_geometry(&point)?;
        }

        writer.finish()?;
    }

    // Verify all geometries were written
    {
        let reader = GeoParquetReader::open(&path)?;
        assert_eq!(reader.num_rows(), 250);
    }

    Ok(())
}

// ── In-memory sources: `open()` vs `from_bytes()` ──────────────────────────────

/// `GeoParquetReader::from_bytes` must be indistinguishable from
/// `GeoParquetReader::open` on the very same file: identical metadata,
/// identical geometries, identical attribute values.
///
/// Regression for the reader being path-only — it used to hold an
/// `Arc<File>` and had no way to consume a Parquet image that never touched the
/// filesystem.
#[test]
fn test_open_and_from_bytes_agree() -> Result<(), Box<dyn std::error::Error>> {
    let path = TempPath::new("test_from_bytes.parquet");

    let geometries = vec![
        Geometry::Point(Point::new_2d(-122.4, 37.8)),
        Geometry::Point(Point::new_2d(-118.2, 34.0)),
        Geometry::Point(Point::new_2d(-87.6, 41.9)),
    ];
    let names = ["san francisco", "los angeles", "chicago"];
    let populations = [873_965_i32, 3_898_747, 2_746_388];

    // Write geometries plus two attribute columns.
    {
        let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
        let mut writer = GeoParquetWriter::new(&path, "geometry", metadata)?
            .add_field(Field::new("name", DataType::Utf8, false))?
            .add_field(Field::new("population", DataType::Int32, false))?;

        for ((geom, name), pop) in geometries.iter().zip(names).zip(populations) {
            writer.add_row(
                geom,
                &[
                    Arc::new(StringArray::from(vec![name])) as ArrayRef,
                    Arc::new(Int32Array::from(vec![pop])) as ArrayRef,
                ],
            )?;
        }
        writer.finish()?;
    }

    let from_path = GeoParquetReader::open(&path)?;
    let raw = std::fs::read(&path)?;
    let from_memory = GeoParquetReader::from_bytes(raw)?;

    // Metadata parity.
    assert_eq!(from_memory.num_rows(), from_path.num_rows());
    assert_eq!(from_memory.num_row_groups(), from_path.num_row_groups());
    assert_eq!(
        from_memory.geometry_column_name(),
        from_path.geometry_column_name()
    );
    assert_eq!(
        from_memory.metadata().primary_column,
        from_path.metadata().primary_column
    );
    assert_eq!(from_memory.schema(), from_path.schema());

    // Geometry parity — and both must match what was written.
    let path_geoms = from_path.read_geometries(0)?;
    let memory_geoms = from_memory.read_geometries(0)?;
    assert_eq!(path_geoms, geometries);
    assert_eq!(memory_geoms, path_geoms);

    // Property parity, column by column.
    let path_batch = from_path.read_row_group(0)?;
    let memory_batch = from_memory.read_row_group(0)?;
    assert_eq!(memory_batch, path_batch);

    let read_names = memory_batch
        .column_by_name("name")
        .ok_or("missing name column")?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or("name column is not a StringArray")?;
    let read_pops = memory_batch
        .column_by_name("population")
        .ok_or("missing population column")?
        .as_any()
        .downcast_ref::<Int32Array>()
        .ok_or("population column is not an Int32Array")?;

    assert_eq!(read_names.len(), names.len());
    for (i, (name, pop)) in names.iter().zip(populations).enumerate() {
        assert_eq!(read_names.value(i), *name);
        assert_eq!(read_pops.value(i), pop);
    }

    // The in-memory reader must also support the pushdown path.
    let filtered = GeoParquetReader::from_bytes(std::fs::read(&path)?)?
        .with_bbox_filter((-123.0, 33.0, -117.0, 38.0))
        .read_pushdown()?;
    let total: usize = filtered.iter().map(|b| b.num_rows()).sum();
    assert_eq!(
        total, 2,
        "only the two west-coast points intersect the bbox"
    );

    Ok(())
}

/// `from_bytes` on a buffer that is not a Parquet image must fail cleanly
/// rather than panic.
#[test]
fn test_from_bytes_rejects_garbage() {
    let result = GeoParquetReader::from_bytes(b"not a parquet file at all".to_vec());
    assert!(result.is_err(), "garbage input must be rejected");
}
