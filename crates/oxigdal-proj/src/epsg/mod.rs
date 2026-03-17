//! EPSG code database with common coordinate reference systems.
//!
//! This module provides a built-in database of common EPSG codes used in geospatial applications.
//! The database includes approximately 100 of the most commonly used coordinate reference systems,
//! including WGS84, Web Mercator, UTM zones, and common national grids.

mod geographic;
mod projected;
mod types;
mod utm;

pub use types::*;

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn test_epsg_database_creation() {
        let db = EpsgDatabase::new();
        assert!(!db.is_empty());
        assert!(db.len() > 100); // Should have at least 100 codes
    }

    #[test]
    fn test_wgs84_lookup() {
        let db = EpsgDatabase::new();
        let wgs84 = db.lookup(4326);
        assert!(wgs84.is_ok());
        let wgs84 = wgs84.expect("WGS84 should exist");
        assert_eq!(wgs84.code, 4326);
        assert_eq!(wgs84.name, "WGS 84");
        assert_eq!(wgs84.crs_type, CrsType::Geographic);
        assert_eq!(wgs84.datum, "WGS84");
    }

    #[test]
    fn test_web_mercator_lookup() {
        let db = EpsgDatabase::new();
        let web_merc = db.lookup(3857);
        assert!(web_merc.is_ok());
        let web_merc = web_merc.expect("Web Mercator should exist");
        assert_eq!(web_merc.code, 3857);
        assert_eq!(web_merc.crs_type, CrsType::Projected);
        assert_eq!(web_merc.unit, "metre");
    }

    #[test]
    fn test_utm_zones() {
        let db = EpsgDatabase::new();

        // Test UTM zone 1N
        let utm_1n = db.lookup(32601);
        assert!(utm_1n.is_ok());
        let utm_1n = utm_1n.expect("UTM 1N should exist");
        assert!(utm_1n.name.contains("UTM zone 1N"));

        // Test UTM zone 60N
        let utm_60n = db.lookup(32660);
        assert!(utm_60n.is_ok());

        // Test UTM zone 1S
        let utm_1s = db.lookup(32701);
        assert!(utm_1s.is_ok());

        // Test UTM zone 60S
        let utm_60s = db.lookup(32760);
        assert!(utm_60s.is_ok());
    }

    #[test]
    fn test_nonexistent_code() {
        let db = EpsgDatabase::new();
        let result = db.lookup(99999);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(Error::EpsgCodeNotFound { code: 99999 })
        ));
    }

    #[test]
    fn test_contains() {
        let db = EpsgDatabase::new();
        assert!(db.contains(4326));
        assert!(db.contains(3857));
        assert!(!db.contains(99999));
    }

    #[test]
    fn test_codes_sorted() {
        let db = EpsgDatabase::new();
        let codes = db.codes();
        assert!(!codes.is_empty());

        // Check that codes are sorted
        for i in 1..codes.len() {
            assert!(codes[i - 1] < codes[i]);
        }
    }

    #[test]
    fn test_custom_definition() {
        let mut db = EpsgDatabase::new();
        let custom = EpsgDefinition {
            code: 99999,
            name: "Custom CRS".to_string(),
            proj_string: "+proj=longlat +datum=WGS84 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Custom area".to_string(),
            unit: "degree".to_string(),
            datum: "WGS84".to_string(),
        };

        db.add_definition(custom.clone());
        assert!(db.contains(99999));

        let retrieved = db.lookup(99999);
        assert!(retrieved.is_ok());
        assert_eq!(retrieved.expect("should exist").name, "Custom CRS");
    }

    #[test]
    fn test_global_lookup() {
        let wgs84 = lookup_epsg(4326);
        assert!(wgs84.is_ok());

        assert!(contains_epsg(4326));
        assert!(!contains_epsg(99999));

        let codes = available_epsg_codes();
        assert!(!codes.is_empty());
    }
}
