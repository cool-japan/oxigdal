//! UTM zone definitions for the EPSG database.

#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;

use super::types::{CrsType, EpsgDatabase, EpsgDefinition};

/// Register all UTM zone definitions into the database.
pub(crate) fn register_utm_zones(db: &mut EpsgDatabase) {
    register_wgs84_utm(db);
    register_jgd2011_utm(db);
    register_gda2020_utm(db);
    register_etrs89_utm(db);
    register_nad83_utm(db);
    register_nad27_utm(db);
    register_gda94_mga(db);
    register_sirgas_utm(db);
    register_ed50_utm(db);
    register_wgs72_utm(db);
    register_cgcs2000_utm(db);
    register_pulkovo_gk(db);
    register_jgd2000_plane_rect(db);
}

fn register_wgs84_utm(db: &mut EpsgDatabase) {
    // UTM Zones (Zone 1N to 60N for WGS84)
    for zone in 1..=60 {
        let code = 32600 + zone;
        let central_meridian = -183 + (zone as i32 * 6);
        db.add_definition(EpsgDefinition {
            code,
            name: format!("WGS 84 / UTM zone {}N", zone),
            proj_string: format!("+proj=utm +zone={} +datum=WGS84 +units=m +no_defs", zone),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "Between {}°E and {}°E, northern hemisphere",
                central_meridian - 3,
                central_meridian + 3
            ),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });
    }

    // UTM Zones (Zone 1S to 60S for WGS84)
    for zone in 1..=60 {
        let code = 32700 + zone;
        let central_meridian = -183 + (zone as i32 * 6);
        db.add_definition(EpsgDefinition {
            code,
            name: format!("WGS 84 / UTM zone {}S", zone),
            proj_string: format!(
                "+proj=utm +zone={} +south +datum=WGS84 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "Between {}°E and {}°E, southern hemisphere",
                central_meridian - 3,
                central_meridian + 3
            ),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });
    }

    // WGS 84 / UTM zone 37N (override check)
    db.add_definition(EpsgDefinition {
        code: 32637,
        name: "WGS 84 / UTM zone 37N (override check)".to_string(),
        proj_string: "+proj=utm +zone=37 +datum=WGS84 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "34°E to 40°E, northern hemisphere".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });
}

fn register_jgd2011_utm(db: &mut EpsgDatabase) {
    // JGD2011 UTM zones EPSG:6669–6687 (zones 51N–60N with JGD2011 datum)
    for zone in 51u32..=60 {
        let code = 6618 + zone;
        let central_meridian = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("JGD2011 / UTM zone {}N", zone),
            proj_string: format!("+proj=utm +zone={} +ellps=GRS80 +units=m +no_defs", zone),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "Japan — {}°E to {}°E",
                central_meridian - 3,
                central_meridian + 3
            ),
            unit: "metre".to_string(),
            datum: "JGD2011".to_string(),
        });
    }
}

fn register_gda2020_utm(db: &mut EpsgDatabase) {
    // GDA2020 UTM zones EPSG:7845–7858 (zones 48S–60S)
    for zone in 48u32..=60 {
        let code = 7797 + zone;
        let central_meridian = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("GDA2020 / MGA zone {}", zone),
            proj_string: format!(
                "+proj=utm +zone={} +south +ellps=GRS80 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "Australia — {}°E to {}°E",
                central_meridian - 3,
                central_meridian + 3
            ),
            unit: "metre".to_string(),
            datum: "GDA2020".to_string(),
        });
    }

    // GDA2020 / MGA zones (EPSG:7844 + 20 zones)
    for zone in 49u32..=60 {
        let code = 7844 + zone;
        let lon_0 = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("GDA2020 / MGA zone {}", zone),
            proj_string: format!(
                "+proj=utm +zone={} +south +ellps=GRS80 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("Australia — {}°E to {}°E", lon_0 - 3, lon_0 + 3),
            unit: "metre".to_string(),
            datum: "GDA2020".to_string(),
        });
    }
}

fn register_etrs89_utm(db: &mut EpsgDatabase) {
    // ETRS89 / UTM zone 32N (EPSG:25832)
    db.add_definition(EpsgDefinition {
        code: 25832,
        name: "ETRS89 / UTM zone 32N".to_string(),
        proj_string: "+proj=utm +zone=32 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Europe - 6°E to 12°E".to_string(),
        unit: "metre".to_string(),
        datum: "ETRS89".to_string(),
    });

    // ETRS89 / UTM zone 33N and 34N
    for zone in [33u32, 34u32] {
        let code = 25800 + zone;
        let central_meridian = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("ETRS89 / UTM zone {}N", zone),
            proj_string: format!(
                "+proj=utm +zone={} +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "Europe — {}°E to {}°E",
                central_meridian - 3,
                central_meridian + 3
            ),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });
    }

    // ETRS89 / UTM zones 28N–37N (full range for Europe)
    for zone in 28u32..=37 {
        let code = 25800 + zone;
        if code == 25832 || code == 25833 || code == 25834 {
            continue; // already added
        }
        let central_meridian = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("ETRS89 / UTM zone {}N", zone),
            proj_string: format!(
                "+proj=utm +zone={} +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "Europe — {}°E to {}°E",
                central_meridian - 3,
                central_meridian + 3
            ),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });
    }

    // Norwegian EUREF89 UTM zones 25835 and 25836
    db.add_definition(EpsgDefinition {
        code: 25835,
        name: "ETRS89 / UTM zone 35N".to_string(),
        proj_string: "+proj=utm +zone=35 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Europe — 24°E to 30°E".to_string(),
        unit: "metre".to_string(),
        datum: "ETRS89".to_string(),
    });

    db.add_definition(EpsgDefinition {
        code: 25836,
        name: "ETRS89 / UTM zone 36N".to_string(),
        proj_string: "+proj=utm +zone=36 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Europe — 30°E to 36°E".to_string(),
        unit: "metre".to_string(),
        datum: "ETRS89".to_string(),
    });

    // Spain — ETRS89 / UTM zone 29N-31N
    for zone in 29u32..=31 {
        let code = 25800 + zone;
        let central_meridian = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("ETRS89 / UTM zone {}N", zone),
            proj_string: format!(
                "+proj=utm +zone={} +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "Spain — {}°W to {}°E",
                -(central_meridian - 3),
                central_meridian + 3
            ),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });
    }
}

fn register_nad83_utm(db: &mut EpsgDatabase) {
    // NAD83 / UTM zone 10N (US West Coast)
    db.add_definition(EpsgDefinition {
        code: 26910,
        name: "NAD83 / UTM zone 10N".to_string(),
        proj_string: "+proj=utm +zone=10 +datum=NAD83 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "North America - 126°W to 120°W".to_string(),
        unit: "metre".to_string(),
        datum: "NAD83".to_string(),
    });

    // NAD83 UTM zones 11N–18N (US/Canada coverage)
    let nad83_utm_zones: &[(u32, u32, &str)] = &[
        (26911, 11, "Between 120°W and 114°W"),
        (26912, 12, "Between 114°W and 108°W"),
        (26913, 13, "Between 108°W and 102°W"),
        (26914, 14, "Between 102°W and 96°W"),
        (26915, 15, "Between 96°W and 90°W"),
        (26916, 16, "Between 90°W and 84°W"),
        (26917, 17, "Between 84°W and 78°W"),
        (26918, 18, "Between 78°W and 72°W"),
        (26919, 19, "Between 72°W and 66°W"),
    ];
    for (code, zone, aou) in nad83_utm_zones {
        db.add_definition(EpsgDefinition {
            code: *code,
            name: format!("NAD83 / UTM zone {}N", zone),
            proj_string: format!("+proj=utm +zone={} +datum=NAD83 +units=m +no_defs", zone),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: aou.to_string(),
            unit: "metre".to_string(),
            datum: "NAD83".to_string(),
        });
    }

    // NAD83 UTM zones 1–22N (zones 10–20 already added individually, skip those)
    for zone in 1u32..=22 {
        if (10..=20).contains(&zone) {
            continue;
        }
        let code = 26900 + zone;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("NAD83 / UTM zone {}N", zone),
            proj_string: format!("+proj=utm +zone={} +datum=NAD83 +units=m +no_defs", zone),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "North America".to_string(),
            unit: "metre".to_string(),
            datum: "NAD83".to_string(),
        });
    }
}

fn register_nad27_utm(db: &mut EpsgDatabase) {
    // NAD27 UTM zones 10N–20N
    for zone in 10u32..=20 {
        let code = 26700 + zone;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("NAD27 / UTM zone {}N", zone),
            proj_string: format!("+proj=utm +zone={} +datum=NAD27 +units=m +no_defs", zone),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "North America".to_string(),
            unit: "metre".to_string(),
            datum: "NAD27".to_string(),
        });
    }

    // NAD27 UTM zones 1–9N
    for zone in 1u32..=9 {
        let code = 26700 + zone;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("NAD27 / UTM zone {}N", zone),
            proj_string: format!("+proj=utm +zone={} +datum=NAD27 +units=m +no_defs", zone),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "North America".to_string(),
            unit: "metre".to_string(),
            datum: "NAD27".to_string(),
        });
    }
}

fn register_gda94_mga(db: &mut EpsgDatabase) {
    // GDA94 MGA zones EPSG:28348–28356 (zones 48S–56S)
    for zone in 48u32..=56 {
        let code = 27892 + zone;
        let central_meridian = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("GDA94 / MGA zone {}", zone),
            proj_string: format!(
                "+proj=utm +zone={} +south +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "Australia — {}°E to {}°E",
                central_meridian - 3,
                central_meridian + 3
            ),
            unit: "metre".to_string(),
            datum: "GDA94".to_string(),
        });
    }

    // GDA94 / MGA zones not yet added (zones 57–60)
    for zone in 57u32..=60 {
        let code = 28300 + zone;
        let lon_0 = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("GDA94 / MGA zone {}", zone),
            proj_string: format!(
                "+proj=utm +zone={} +south +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("Australia — {}°E to {}°E", lon_0 - 3, lon_0 + 3),
            unit: "metre".to_string(),
            datum: "GDA94".to_string(),
        });
    }
}

fn register_sirgas_utm(db: &mut EpsgDatabase) {
    // SIRGAS 2000 UTM zones for South America
    for zone in 17u32..=25 {
        let code = 31960 + zone;
        let central_meridian = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("SIRGAS 2000 / UTM zone {}S", zone),
            proj_string: format!(
                "+proj=utm +zone={} +south +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "South America — {}°W to {}°W",
                -central_meridian + 3,
                -central_meridian - 3
            ),
            unit: "metre".to_string(),
            datum: "SIRGAS2000".to_string(),
        });
    }

    // SIRGAS 2000 / UTM South zones 17S–25S (duplicate-safe)
    for zone in 17u32..=25 {
        let code = 31960 + zone;
        let lon_0 = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("SIRGAS 2000 / UTM zone {}S", zone),
            proj_string: format!(
                "+proj=utm +zone={} +south +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "South America — {}°W to {}°W, southern hemisphere",
                -(lon_0 - 3),
                -(lon_0 + 3)
            ),
            unit: "metre".to_string(),
            datum: "SIRGAS 2000".to_string(),
        });
    }
}

fn register_ed50_utm(db: &mut EpsgDatabase) {
    // Turkey — ED50 / UTM zones 35N-37N
    for zone in 35u32..=37 {
        let utm_code = 23000 + zone;
        db.add_definition(EpsgDefinition {
            code: utm_code,
            name: format!("ED50 / UTM zone {}N", zone),
            proj_string: format!(
                "+proj=utm +zone={} +ellps=intl +towgs84=-87,-98,-121,0,0,0,0 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("Turkey — UTM zone {}N", zone),
            unit: "metre".to_string(),
            datum: "ED50".to_string(),
        });
    }

    // ED50 / UTM North zones (EPSG:23028–23038)
    for zone in 28u32..=38 {
        let code = 23000 + zone;
        let lon_0 = (zone as i32 - 1) * 6 - 177;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("ED50 / UTM zone {}N", zone),
            proj_string: format!(
                "+proj=utm +zone={} +ellps=intl +towgs84=-87,-98,-121 +units=m +no_defs",
                zone
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("Europe (historical) — {}°E to {}°E", lon_0 - 3, lon_0 + 3),
            unit: "metre".to_string(),
            datum: "ED50".to_string(),
        });
    }
}

fn register_wgs72_utm(db: &mut EpsgDatabase) {
    // WGS72 / UTM North zones 1–60 (EPSG:32201–32260)
    for zone in 1u32..=60 {
        let code = 32200 + zone;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("WGS 72 / UTM zone {}N", zone),
            proj_string: format!("+proj=utm +zone={} +ellps=WGS72 +towgs84=0,0,4.5,0,0,0.554,0.219 +units=m +no_defs", zone),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World".to_string(),
            unit: "metre".to_string(),
            datum: "WGS72".to_string(),
        });
    }
}

fn register_cgcs2000_utm(db: &mut EpsgDatabase) {
    // CGCS2000 / 3-degree Gauss-Kruger zones
    for zone in 25u32..=45 {
        let code = 4466 + zone;
        let lon_0 = zone as f64 * 3.0;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("CGCS2000 / 3-degree Gauss-Kruger zone {}", zone),
            proj_string: format!(
                "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0={} +y_0=0 +ellps=GRS80 +units=m +no_defs",
                lon_0,
                zone as u64 * 1_000_000 + 500_000
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("China — {}°E to {}°E", lon_0 as i32 - 2, lon_0 as i32 + 2),
            unit: "metre".to_string(),
            datum: "CGCS2000".to_string(),
        });
    }

    // CGCS2000 / 6-degree Gauss-Kruger zones
    for zone in 13u32..=23 {
        let code = 4513 + zone;
        let lon_0 = (zone as f64 - 1.0) * 6.0 - 177.0 + 6.0;
        let lon_0_precise = zone as f64 * 6.0 - 183.0;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("CGCS2000 / 6-degree Gauss-Kruger zone {}", zone),
            proj_string: format!(
                "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0={} +y_0=0 +ellps=GRS80 +units=m +no_defs",
                lon_0_precise,
                zone as u64 * 1_000_000 + 500_000
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("China — {}°E to {}°E", lon_0 as i32 - 3, lon_0 as i32 + 3),
            unit: "metre".to_string(),
            datum: "CGCS2000".to_string(),
        });
    }

    // CGCS2000 / UTM zones 43N–53N
    for zone in 43u32..=53 {
        let code = 4535 + (zone - 43);
        let lon_0 = (zone as f64 - 1.0) * 6.0 - 177.0;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("CGCS2000 / UTM zone {}N", zone),
            proj_string: format!("+proj=utm +zone={} +ellps=GRS80 +units=m +no_defs", zone),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("China — {}°E to {}°E", lon_0 as i32 - 3, lon_0 as i32 + 3),
            unit: "metre".to_string(),
            datum: "CGCS2000".to_string(),
        });
    }
}

fn register_pulkovo_gk(db: &mut EpsgDatabase) {
    // Gauss-Kruger zones for Russia (6° strips, Pulkovo 1942)
    for zone in 4u32..=32 {
        let code = 28400 + zone;
        let lon_0 = (zone as f64 - 1.0) * 6.0 - 177.0 + 6.0;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("Pulkovo 1942 / Gauss-Kruger zone {}", zone),
            proj_string: format!(
                "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0={} +y_0=0 +ellps=krass +towgs84=23.57,-140.95,-79.8,0,0.35,0.79,-0.22 +units=m +no_defs",
                lon_0, zone as u64 * 1_000_000 + 500_000
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("Russia — zone {}", zone),
            unit: "metre".to_string(),
            datum: "Pulkovo1942".to_string(),
        });
    }
}

fn register_jgd2000_plane_rect(db: &mut EpsgDatabase) {
    // JGD2000 / Japan Plane Rectangular CS zones I–XIX (EPSG:2443–2461)
    let jp_lon_cm = [
        129.5_f64, 131.0, 132.1667, 133.5, 134.3333, 136.0, 137.1667, 138.5, 139.8333, 140.8333,
        140.25, 142.25, 144.25, 142.0, 127.5, 124.0, 131.0, 136.0, 154.0,
    ];
    for (i, lon_cm) in jp_lon_cm.iter().enumerate() {
        let zone_num = i + 1;
        let code = 2442 + zone_num as u32;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("JGD2000 / Japan Plane Rectangular CS zone {}", zone_num),
            proj_string: format!("+proj=tmerc +lat_0=0 +lon_0={} +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs", lon_cm),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("Japan — zone {}", zone_num),
            unit: "metre".to_string(),
            datum: "JGD2000".to_string(),
        });
    }
}
