//! Integration tests for GeoParquet round-trip read/write
#![allow(clippy::panic)]

use oxigdal_geoparquet::geometry::{Coordinate, Geometry, LineString, Point, Polygon};
use oxigdal_geoparquet::metadata::{Crs, GeometryColumnMetadata};
use oxigdal_geoparquet::{GeoParquetReader, GeoParquetWriter};

#[test]
fn test_roundtrip_points() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("test_roundtrip_points.parquet");

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

    // Cleanup
    let _ = std::fs::remove_file(&path);

    Ok(())
}

#[test]
fn test_roundtrip_linestrings() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("test_roundtrip_linestrings.parquet");

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

    // Cleanup
    let _ = std::fs::remove_file(&path);

    Ok(())
}

#[test]
fn test_roundtrip_polygons() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("test_roundtrip_polygons.parquet");

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

    // Cleanup
    let _ = std::fs::remove_file(&path);

    Ok(())
}

#[test]
fn test_metadata_preservation() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("test_metadata.parquet");

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

    // Cleanup
    let _ = std::fs::remove_file(&path);

    Ok(())
}

#[test]
fn test_batch_writing() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("test_batch.parquet");

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

    // Cleanup
    let _ = std::fs::remove_file(&path);

    Ok(())
}
