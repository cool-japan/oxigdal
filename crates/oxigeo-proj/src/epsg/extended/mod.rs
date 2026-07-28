//! Extended EPSG definitions to reach 500+ codes.
//!
//! This module adds additional EPSG codes beyond the core set,
//! including State Plane Coordinate Systems, European national grids,
//! Asian/Pacific grids, South American grids, African grids, and polar systems.
//!
//! The actual definitions are partitioned across regional sub-modules to
//! keep individual files small and easy to maintain.

use super::types::EpsgDatabase;

mod additional_geographic;
mod african;
mod asian_pacific;
mod european;
mod nad83;
mod polar;
mod south_american;
mod vertical;

/// Register all extended EPSG definitions into the database.
pub(crate) fn register_extended_crs(db: &mut EpsgDatabase) {
    nad83::register_nad83_state_planes(db);
    european::register_european_national_grids(db);
    asian_pacific::register_asian_pacific_grids(db);
    south_american::register_south_american_grids(db);
    african::register_african_grids(db);
    polar::register_polar_grids(db);
    additional_geographic::register_additional_geographic(db);
    vertical::register_vertical_crs(db);
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::epsg::types::CrsType;

    #[test]
    fn test_nad83_state_planes_registered() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        // California zone 1
        assert!(db.contains(2225));
        // New York Long Island
        assert!(db.contains(2263));
        // Texas Central
        assert!(db.contains(2277));
    }

    #[test]
    fn test_nad83_2011_state_planes() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        assert!(db.contains(6355));
        assert!(db.contains(6401));
    }

    #[test]
    fn test_european_grids_registered() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        // SWEREF99 TM
        assert!(db.contains(3006));
        // Dutch RD New
        assert!(db.contains(28992));
        // Czech S-JTSK
        assert!(db.contains(5514));
        // Belgian Lambert
        assert!(db.contains(31370));
        // Portuguese TM06
        assert!(db.contains(3763));
    }

    #[test]
    fn test_asian_pacific_grids_registered() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        // CGCS2000 zone 13
        assert!(db.contains(4502));
        // Korea 2000 Unified CS
        assert!(db.contains(5179));
        // NZGD2000 TM
        assert!(db.contains(2193));
        // Singapore SVY21
        assert!(db.contains(3414));
        // Hong Kong
        assert!(db.contains(2326));
        // Taiwan TWD97
        assert!(db.contains(3826));
    }

    #[test]
    fn test_south_american_grids_registered() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        // SIRGAS 2000 zones
        assert!(db.contains(31981));
        assert!(db.contains(31985));
        // POSGAR 2007 Argentina zones
        assert!(db.contains(5343));
        assert!(db.contains(5349));
        // Colombian MAGNA-SIRGAS zones
        assert!(db.contains(3116));
    }

    #[test]
    fn test_african_grids_registered() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        // South Africa Lo-series
        assert!(db.contains(2046));
        assert!(db.contains(2055));
        // Egyptian grids
        assert!(db.contains(22992));
        // Nigeria grids
        assert!(db.contains(26331));
        // Ghana grid
        assert!(db.contains(25000));
    }

    #[test]
    fn test_polar_grids_registered() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        // Arctic NSIDC
        assert!(db.contains(3411));
        assert!(db.contains(3413));
        // Antarctic PS
        assert!(db.contains(3031));
        assert!(db.contains(3976));
        // UPS
        assert!(db.contains(32661));
        assert!(db.contains(32761));
    }

    #[test]
    fn test_vertical_crs_registered() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        // NAVD88
        assert!(db.contains(5703));
        // EGM96
        assert!(db.contains(5773));
        // EGM2008
        assert!(db.contains(3855));
        let navd88 = db.lookup(5703).expect("NAVD88 should exist");
        assert_eq!(navd88.crs_type, CrsType::Vertical);
        assert_eq!(navd88.unit, "metre");
    }

    #[test]
    fn test_additional_geographic_registered() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        // ITRF2014
        assert!(db.contains(7789));
        // JGD2011
        assert!(db.contains(6668));
        // GDA2020
        assert!(db.contains(7844));
        // NAD83(2011)
        assert!(db.contains(6318));
        let jgd2011 = db.lookup(6668).expect("JGD2011 should exist");
        assert_eq!(jgd2011.crs_type, CrsType::Geographic);
        assert_eq!(jgd2011.unit, "degree");
    }

    #[test]
    fn test_extended_crs_count() {
        let mut db = EpsgDatabase {
            definitions: Default::default(),
        };
        register_extended_crs(&mut db);
        // Should have added 300+ new definitions
        assert!(
            db.len() >= 300,
            "Extended CRS should have at least 300 definitions, got {}",
            db.len()
        );
    }

    #[test]
    fn test_full_database_above_500() {
        let db = EpsgDatabase::new();
        assert!(
            db.len() >= 500,
            "Full database should have 500+ definitions, got {}",
            db.len()
        );
    }
}
