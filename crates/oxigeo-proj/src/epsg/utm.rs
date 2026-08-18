//! UTM zone definitions for the EPSG database.

use alloc::format;
use alloc::string::ToString;

use super::types::{CrsType, EpsgDatabase, EpsgDefinition};

/// Register all UTM zone definitions into the database.
pub(crate) fn register_utm_zones(db: &mut EpsgDatabase) {
    register_wgs84_utm(db);
    register_jgd2011_plane_rect(db);
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
}

/// The 19 Japan Plane Rectangular CS zones: `(numeral, lat_0, lon_0)`.
///
/// Japan Plane Rectangular CS uses a **per-zone** origin latitude between 26°N
/// and 44°N (never 0°) together with a full-precision central meridian. Both
/// the JGD2000 (EPSG:2443–2461) and JGD2011 (EPSG:6669–6687) realizations of
/// the system share this same zone geometry — only the datum differs — so a
/// single table drives both families. Keeping them on one table is deliberate:
/// the historical registry carried two independent copies, and they drifted
/// (JGD2000 hardcoded `+lat_0=0` for 16 of 19 zones, which inverted Tokyo
/// 4,007 km into the Pacific).
///
/// Five-point-verified against PROJ 9.5.1 for every zone of both families; see
/// `tests/epsg_verified_registry_extended_test.rs`.
const JAPAN_PLANE_RECT_ZONES: &[(&str, &str, &str)] = &[
    ("I", "33", "129.5"),
    ("II", "33", "131"),
    ("III", "36", "132.166666666667"),
    ("IV", "33", "133.5"),
    ("V", "36", "134.333333333333"),
    ("VI", "36", "136"),
    ("VII", "36", "137.166666666667"),
    ("VIII", "36", "138.5"),
    ("IX", "36", "139.833333333333"),
    ("X", "40", "140.833333333333"),
    ("XI", "44", "140.25"),
    ("XII", "44", "142.25"),
    ("XIII", "44", "144.25"),
    ("XIV", "26", "142"),
    ("XV", "26", "127.5"),
    ("XVI", "26", "124"),
    ("XVII", "26", "131"),
    ("XVIII", "20", "136"),
    ("XIX", "26", "154"),
];

fn register_jgd2011_plane_rect(db: &mut EpsgDatabase) {
    // JGD2011 / Japan Plane Rectangular CS I–XIX (EPSG:6669–6687).
    //
    // Zones I–X were previously misregistered as "JGD2011 / UTM zone 51N–60N"
    // — a whole-family misassignment (the JGD2011 UTM zones actually live at
    // EPSG:6688–6692, registered below) that placed Japan Plane Rectangular
    // data ~4,000 km east into the Pacific. Zones XI–XIX were absent
    // entirely; they are now registered from the shared verified zone table.
    for (index, (numeral, lat_0, lon_0)) in JAPAN_PLANE_RECT_ZONES.iter().enumerate() {
        db.add_definition(EpsgDefinition {
            code: 6669 + index as u32,
            name: format!("JGD2011 / Japan Plane Rectangular CS {}", numeral),
            proj_string: format!(
                "+proj=tmerc +lat_0={} +lon_0={} +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +units=m +no_defs",
                lat_0, lon_0
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("Japan — zone {}", numeral),
            unit: "metre".to_string(),
            datum: "JGD2011".to_string(),
        });
    }

    // JGD2011 / UTM zones 51N–55N (EPSG:6688–6692) — the codes the Plane
    // Rectangular zones above used to squat on. Japan spans UTM zones 51–55.
    for zone in 51u32..=55 {
        let central_meridian = zone as i32 * 6 - 183;
        db.add_definition(EpsgDefinition {
            code: 6637 + zone,
            name: format!("JGD2011 / UTM zone {}N", zone),
            proj_string: format!("+proj=utm +zone={} +ellps=GRS80 +units=m +no_defs", zone),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!(
                "Japan — {}°E to {}°E, northern hemisphere",
                central_meridian - 3,
                central_meridian + 3
            ),
            unit: "metre".to_string(),
            datum: "JGD2011".to_string(),
        });
    }
}

fn register_gda2020_utm(db: &mut EpsgDatabase) {
    // GDA2020 / MGA zones 46–59 (EPSG:7846–7859).
    //
    // Two historical loops got this block wrong. The first mapped zone n to
    // `7797 + n`, which is off by three (EPSG:7846 is MGA zone 46, not 49)
    // and additionally overwrote EPSG:7845, which is "GDA2020 / GA LCC" — a
    // Lambert conic, not a UTM zone. The second loop mapped zone n to
    // `7844 + n`, registering MGA zones on EPSG:7893–7904: six codes that do
    // not exist in EPSG at all (7893–7898) plus "GDA2020 / Vicgrid" (7899)
    // and ITRF88–ITRF92 (7900–7904). Those five ITRF geographic CRSs were
    // consequently served as Australian UTM grids, a 3,352,000 km error —
    // the worst single defect found in the registry.
    for zone in 46u32..=59 {
        let central_meridian = zone as i32 * 6 - 183;
        db.add_definition(EpsgDefinition {
            code: 7800 + zone,
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

    // GDA2020 / Vicgrid (EPSG:7899) — the Victoria-wide Lambert conic that
    // the removed loop had claimed as "MGA zone 55".
    db.add_definition(EpsgDefinition {
        code: 7899,
        name: "GDA2020 / Vicgrid".to_string(),
        proj_string: "+proj=lcc +lat_0=-37 +lat_1=-36 +lat_2=-38 +lon_0=145 +x_0=2500000 +y_0=2500000 +ellps=GRS80 +units=m +no_defs"
                .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Australia — Victoria".to_string(),
        unit: "metre".to_string(),
        datum: "GDA2020".to_string(),
    });
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

/// Normalises a longitude in whole degrees into `(-180, 180]`.
///
/// Gauss-Kruger zones past 30 wrap the antimeridian, and PROJ states their
/// central meridians on the negative side — zone 31 is 177°W, not 183°E.
fn normalise_longitude_degrees(degrees: i32) -> i32 {
    ((degrees + 180) % 360) - 180
}

/// Central meridian of a **6°** Gauss-Kruger zone, in degrees east.
///
/// 6° zones are numbered from the prime meridian — zone 1 spans 0°–6°E, so
/// its central meridian is 3°E — giving `CM = 6 * zone - 3`. This is **not**
/// the UTM rule. The historical registry used the UTM formula
/// (`6 * zone - 183`) here, putting every Pulkovo 1942 and CGCS2000 6° zone
/// exactly 180° from its true meridian.
fn gauss_kruger_cm_6_degree(zone: u32) -> i32 {
    normalise_longitude_degrees(zone as i32 * 6 - 3)
}

/// Central meridian of a **3°** Gauss-Kruger zone, in degrees east.
///
/// 3° zones use a different convention from the 6° ones: they are numbered by
/// their own central meridian, so zone 25 is CM 75°E and `CM = 3 * zone`.
/// There is no half-zone offset — applying the 6° rule here shifts every zone
/// by 1.5°, about 95 km at Chinese latitudes.
fn gauss_kruger_cm_3_degree(zone: u32) -> i32 {
    normalise_longitude_degrees(zone as i32 * 3)
}

fn register_cgcs2000_utm(db: &mut EpsgDatabase) {
    // CGCS2000 Gauss-Kruger families (EPSG:4491–4554).
    //
    // The historical registry had this whole block shifted: EPSG:4491–4501
    // (6° zones 13–23) carried 3° zone parameters, EPSG:4513–4523 (3° zones
    // 25–35) carried 6° ones, and EPSG:4535–4545 (3° CM 78E–108E) were
    // registered as "CGCS2000 / UTM zone 43N–53N", a CRS family that does not
    // exist at those codes. Worst observed error was 32,614 km (EPSG:4526).
    //
    // EPSG lays the block out as four consecutive runs, each verified against
    // PROJ 9.5.1:
    //   4491–4501  6° zone 13–23   x_0 = zone * 1e6 + 500000  (zone-prefixed)
    //   4502–4512  6° CM 75E–135E  x_0 = 500000               (no prefix)
    //   4513–4533  3° zone 25–45   x_0 = zone * 1e6 + 500000
    //   4534–4554  3° CM 75E–135E  x_0 = 500000
    for (index, zone) in (13u32..=23).enumerate() {
        let lon_0 = gauss_kruger_cm_6_degree(zone);
        register_gauss_kruger_zone(
            db,
            4491 + index as u32,
            &format!("CGCS2000 / Gauss-Kruger zone {}", zone),
            lon_0,
            zone as u64 * 1_000_000 + 500_000,
            3,
        );
        register_gauss_kruger_zone(
            db,
            4502 + index as u32,
            &format!("CGCS2000 / Gauss-Kruger CM {}E", lon_0),
            lon_0,
            500_000,
            3,
        );
    }

    for (index, zone) in (25u32..=45).enumerate() {
        let lon_0 = gauss_kruger_cm_3_degree(zone);
        register_gauss_kruger_zone(
            db,
            4513 + index as u32,
            &format!("CGCS2000 / 3-degree Gauss-Kruger zone {}", zone),
            lon_0,
            zone as u64 * 1_000_000 + 500_000,
            2,
        );
        register_gauss_kruger_zone(
            db,
            4534 + index as u32,
            &format!("CGCS2000 / 3-degree Gauss-Kruger CM {}E", lon_0),
            lon_0,
            500_000,
            2,
        );
    }
}

/// Registers one CGCS2000 Gauss-Kruger CRS (`+proj=tmerc`, GRS80, unscaled).
fn register_gauss_kruger_zone(
    db: &mut EpsgDatabase,
    code: u32,
    name: &str,
    lon_0: i32,
    x_0: u64,
    half_width: i32,
) {
    db.add_definition(EpsgDefinition {
        code,
        name: name.to_string(),
        proj_string: format!(
            "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0={} +y_0=0 +ellps=GRS80 +units=m +no_defs",
            lon_0, x_0
        ),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: format!(
            "China — {}°E to {}°E",
            lon_0 - half_width,
            lon_0 + half_width
        ),
        unit: "metre".to_string(),
        datum: "CGCS2000".to_string(),
    });
}

fn register_pulkovo_gk(db: &mut EpsgDatabase) {
    // Pulkovo 1942 / Gauss-Kruger zones 4–32 (EPSG:28404–28432).
    //
    // The central meridian previously used the UTM formula, placing every
    // zone 180° from its true position (zone 4 sat at 153°W instead of 21°E)
    // — up to 14,880 km of error. Note that these codes retain a residual
    // datum-shift error against PROJ: PROJ selects a Pulkovo 1942 → WGS 84
    // transformation from its own database rather than from the `+towgs84`
    // in the PROJ string, which `transform::datum_shift` does not implement.
    // The projection itself is five-point-verified.
    for zone in 4u32..=32 {
        let lon_0 = gauss_kruger_cm_6_degree(zone);
        db.add_definition(EpsgDefinition {
            code: 28400 + zone,
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
    // JGD2000 / Japan Plane Rectangular CS I–XIX (EPSG:2443–2461).
    //
    // Every zone now comes from the shared `JAPAN_PLANE_RECT_ZONES` table.
    // The historical registration hardcoded `+lat_0=0` for 16 of the 19 zones
    // (only VIII–X had been corrected), which put zone IX (Tokyo) 4,007 km
    // into the Pacific on inversion; the worst zone was XI at 4,899 km.
    for (index, (numeral, lat_0, lon_0)) in JAPAN_PLANE_RECT_ZONES.iter().enumerate() {
        db.add_definition(EpsgDefinition {
            code: 2443 + index as u32,
            name: format!("JGD2000 / Japan Plane Rectangular CS {}", numeral),
            proj_string: format!(
                "+proj=tmerc +lat_0={} +lon_0={} +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +units=m +no_defs",
                lat_0, lon_0
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("Japan — zone {}", numeral),
            unit: "metre".to_string(),
            datum: "JGD2000".to_string(),
        });
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// Regression test for a leftover debug "(override check)" registration
    /// that used to clobber the correctly-computed EPSG:32637 definition
    /// registered by the `1..=60` WGS84 UTM zone loop with a wrong name and
    /// an incorrect, hard-coded area-of-use string.
    #[test]
    fn test_epsg_32637_is_not_overridden_by_leftover_debug_registration() {
        let mut db = EpsgDatabase::new();
        db.definitions.clear();
        register_wgs84_utm(&mut db);

        let def = db.lookup(32637).expect("EPSG:32637 must be registered");
        assert_eq!(
            def.name, "WGS 84 / UTM zone 37N",
            "name must not carry the leftover '(override check)' suffix"
        );
        assert!(
            def.area_of_use.contains("36") && def.area_of_use.contains("42"),
            "area_of_use must be the loop-computed 36°E–42°E range, got: {}",
            def.area_of_use
        );
        assert!(!def.area_of_use.contains("34"));
        assert!(!def.area_of_use.contains("40"));
    }

    #[test]
    fn test_all_wgs84_utm_zones_registered_exactly_once_with_correct_names() {
        let mut db = EpsgDatabase::new();
        db.definitions.clear();
        register_wgs84_utm(&mut db);

        for zone in 1..=60u32 {
            let north = db
                .lookup(32600 + zone)
                .unwrap_or_else(|_| panic!("zone {zone}N must be registered"));
            assert_eq!(north.name, format!("WGS 84 / UTM zone {zone}N"));

            let south = db
                .lookup(32700 + zone)
                .unwrap_or_else(|_| panic!("zone {zone}S must be registered"));
            assert_eq!(south.name, format!("WGS 84 / UTM zone {zone}S"));
        }
    }
}
