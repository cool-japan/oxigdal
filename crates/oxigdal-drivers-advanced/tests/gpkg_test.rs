//! GeoPackage format tests.

use oxigdal_drivers_advanced::Result;
use oxigdal_drivers_advanced::gpkg::*;
use std::str::FromStr;
use tempfile::NamedTempFile;

#[test]
fn test_gpkg_creation() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let gpkg = GeoPackage::create(temp_file.path())?;
    assert_eq!(gpkg.version(), GpkgVersion::V1_3);
    Ok(())
}

#[test]
fn test_gpkg_open_close() -> Result<()> {
    let temp_file = NamedTempFile::new()?;

    // Create
    {
        let _gpkg = GeoPackage::create(temp_file.path())?;
    }

    // Reopen
    let gpkg = GeoPackage::open(temp_file.path())?;
    assert_eq!(gpkg.version(), GpkgVersion::V1_3);

    Ok(())
}

#[test]
fn test_gpkg_feature_table_creation() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let mut gpkg = GeoPackage::create(temp_file.path())?;

    let table = gpkg.create_feature_table("test_features", GeometryType::Point, 4326)?;
    assert_eq!(table.name(), "test_features");
    assert_eq!(table.geometry_type(), GeometryType::Point);
    assert_eq!(table.srs_id(), 4326);

    Ok(())
}

#[test]
fn test_gpkg_list_tables() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let mut gpkg = GeoPackage::create(temp_file.path())?;

    let tables = gpkg.feature_tables()?;
    assert!(tables.is_empty());

    gpkg.create_feature_table("table1", GeometryType::Point, 4326)?;
    gpkg.create_feature_table("table2", GeometryType::Polygon, 4326)?;

    let tables = gpkg.feature_tables()?;
    assert_eq!(tables.len(), 2);

    Ok(())
}

#[test]
fn test_gpkg_feature_count() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let mut gpkg = GeoPackage::create(temp_file.path())?;

    let table = gpkg.create_feature_table("test_features", GeometryType::Point, 4326)?;
    let count = table.count(gpkg.connection())?;
    assert_eq!(count, 0);

    Ok(())
}

#[test]
fn test_gpkg_tile_matrix_set() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let mut gpkg = GeoPackage::create(temp_file.path())?;

    let extent = Extent::new(-180.0, -90.0, 180.0, 90.0);
    let tms = gpkg.create_tile_matrix_set("test_tiles", 4326, extent)?;

    assert_eq!(tms.name(), "test_tiles");
    assert_eq!(tms.extent().min_x, -180.0);

    Ok(())
}

#[test]
fn test_gpkg_integrity() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let gpkg = GeoPackage::create(temp_file.path())?;
    assert!(gpkg.check_integrity()?);
    Ok(())
}

#[test]
fn test_geometry_type() {
    assert_eq!(GeometryType::Point.as_str(), "POINT");
    let gt = GeometryType::from_str("POINT");
    assert!(gt.is_ok());
    if let Ok(g) = gt {
        assert_eq!(g, GeometryType::Point);
    }
    let gt = GeometryType::from_str("polygon");
    assert!(gt.is_ok());
    if let Ok(g) = gt {
        assert_eq!(g, GeometryType::Polygon);
    }
}

#[test]
fn test_extent() {
    let extent = Extent::new(-180.0, -90.0, 180.0, 90.0);
    assert_eq!(extent.width(), 360.0);
    assert_eq!(extent.height(), 180.0);

    assert!(extent.contains(0.0, 0.0));
    assert!(!extent.contains(200.0, 0.0));
}

#[test]
fn test_extent_intersects() {
    let e1 = Extent::new(0.0, 0.0, 10.0, 10.0);
    let e2 = Extent::new(5.0, 5.0, 15.0, 15.0);
    let e3 = Extent::new(20.0, 20.0, 30.0, 30.0);

    assert!(e1.intersects(&e2));
    assert!(!e1.intersects(&e3));
}

#[test]
fn test_extent_expand() {
    let mut extent = Extent::new(0.0, 0.0, 10.0, 10.0);
    extent.expand(15.0, 15.0);
    assert_eq!(extent.max_x, 15.0);
    assert_eq!(extent.max_y, 15.0);

    extent.expand(-5.0, -5.0);
    assert_eq!(extent.min_x, -5.0);
}

#[test]
fn test_tile_matrix() {
    let matrix = TileMatrix::new(0, 4, 4, 256, 256, 10.0, 10.0);
    assert_eq!(matrix.zoom_level, 0);
    assert_eq!(matrix.pixel_dimensions(), (1024, 1024));

    assert!(matrix.is_valid_tile(0, 0));
    assert!(matrix.is_valid_tile(3, 3));
    assert!(!matrix.is_valid_tile(4, 4));
}

#[test]
fn test_srs_definitions() {
    let wgs84 = Srs::wgs84();
    assert_eq!(wgs84.id, 4326);
    assert_eq!(wgs84.organization, "EPSG");

    let undefined = Srs::undefined_cartesian();
    assert_eq!(undefined.id, -1);
}

#[test]
fn test_rtree_index() {
    let mut index = RTreeIndex::new();
    assert!(index.is_empty());

    index.insert(1, 0.0, 0.0, 10.0, 10.0);
    index.insert(2, 20.0, 20.0, 30.0, 30.0);
    assert_eq!(index.len(), 2);

    let results = index.query(5.0, 5.0, 15.0, 15.0);
    assert!(results.contains(&1));

    index.remove(1);
    assert_eq!(index.len(), 1);
}
