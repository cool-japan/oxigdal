//! GeoPackage metadata structures.

use super::connection::GpkgConnection;
use crate::error::Result;
use serde::{Deserialize, Serialize};

/// GeoPackage metadata container.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpkgMetadata {
    /// Application name
    pub application_name: Option<String>,
    /// User version
    pub user_version: Option<i32>,
    /// Database size in bytes
    pub size: Option<i64>,
}

impl GpkgMetadata {
    /// Read metadata from GeoPackage.
    pub fn read(conn: &GpkgConnection) -> Result<Self> {
        let user_version: i32 = conn
            .connection()
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .unwrap_or(0);

        let size = conn.size().ok();

        Ok(Self {
            application_name: None,
            user_version: Some(user_version),
            size,
        })
    }
}

/// Spatial extent (bounding box).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Extent {
    /// Minimum X coordinate
    pub min_x: f64,
    /// Minimum Y coordinate
    pub min_y: f64,
    /// Maximum X coordinate
    pub max_x: f64,
    /// Maximum Y coordinate
    pub max_y: f64,
}

impl Extent {
    /// Create new extent.
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Get width.
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Get height.
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Check if point is within extent.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Check if extent intersects another.
    pub fn intersects(&self, other: &Extent) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Expand extent to include point.
    pub fn expand(&mut self, x: f64, y: f64) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }
}

/// Spatial Reference System information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Srs {
    /// SRS name
    pub name: String,
    /// SRS ID
    pub id: i32,
    /// Organization (e.g., EPSG)
    pub organization: String,
    /// Organization coordsys ID
    pub organization_id: i32,
    /// WKT definition
    pub definition: String,
    /// Description
    pub description: Option<String>,
}

impl Srs {
    /// Create WGS84 SRS.
    pub fn wgs84() -> Self {
        Self {
            name: "WGS 84".to_string(),
            id: 4326,
            organization: "EPSG".to_string(),
            organization_id: 4326,
            definition: "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563]],PRIMEM[\"Greenwich\",0],UNIT[\"degree\",0.0174532925199433]]".to_string(),
            description: Some("WGS 84 geographic coordinate system".to_string()),
        }
    }

    /// Create undefined Cartesian SRS.
    pub fn undefined_cartesian() -> Self {
        Self {
            name: "Undefined Cartesian SRS".to_string(),
            id: -1,
            organization: "NONE".to_string(),
            organization_id: -1,
            definition: "undefined".to_string(),
            description: Some("undefined cartesian coordinate reference system".to_string()),
        }
    }

    /// Create undefined geographic SRS.
    pub fn undefined_geographic() -> Self {
        Self {
            name: "Undefined Geographic SRS".to_string(),
            id: 0,
            organization: "NONE".to_string(),
            organization_id: 0,
            definition: "undefined".to_string(),
            description: Some("undefined geographic coordinate reference system".to_string()),
        }
    }

    /// Load SRS from database.
    pub fn load(conn: &GpkgConnection, srs_id: i32) -> Result<Self> {
        let row: (String, String, i32, String, Option<String>) = conn.connection().query_row(
            "SELECT srs_name, organization, organization_coordsys_id, definition, description
             FROM gpkg_spatial_ref_sys WHERE srs_id = ?1",
            [srs_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;

        Ok(Self {
            name: row.0,
            id: srs_id,
            organization: row.1,
            organization_id: row.2,
            definition: row.3,
            description: row.4,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extent_creation() {
        let extent = Extent::new(-180.0, -90.0, 180.0, 90.0);
        assert_eq!(extent.width(), 360.0);
        assert_eq!(extent.height(), 180.0);
    }

    #[test]
    fn test_extent_contains() {
        let extent = Extent::new(0.0, 0.0, 10.0, 10.0);
        assert!(extent.contains(5.0, 5.0));
        assert!(extent.contains(0.0, 0.0));
        assert!(extent.contains(10.0, 10.0));
        assert!(!extent.contains(-1.0, 5.0));
        assert!(!extent.contains(11.0, 5.0));
    }

    #[test]
    fn test_extent_intersects() {
        let extent1 = Extent::new(0.0, 0.0, 10.0, 10.0);
        let extent2 = Extent::new(5.0, 5.0, 15.0, 15.0);
        let extent3 = Extent::new(20.0, 20.0, 30.0, 30.0);

        assert!(extent1.intersects(&extent2));
        assert!(extent2.intersects(&extent1));
        assert!(!extent1.intersects(&extent3));
    }

    #[test]
    fn test_extent_expand() {
        let mut extent = Extent::new(0.0, 0.0, 10.0, 10.0);
        extent.expand(15.0, 15.0);
        assert_eq!(extent.max_x, 15.0);
        assert_eq!(extent.max_y, 15.0);

        extent.expand(-5.0, -5.0);
        assert_eq!(extent.min_x, -5.0);
        assert_eq!(extent.min_y, -5.0);
    }

    #[test]
    fn test_srs_wgs84() {
        let srs = Srs::wgs84();
        assert_eq!(srs.id, 4326);
        assert_eq!(srs.organization, "EPSG");
        assert_eq!(srs.organization_id, 4326);
    }

    #[test]
    fn test_srs_undefined() {
        let srs = Srs::undefined_cartesian();
        assert_eq!(srs.id, -1);

        let srs = Srs::undefined_geographic();
        assert_eq!(srs.id, 0);
    }
}
