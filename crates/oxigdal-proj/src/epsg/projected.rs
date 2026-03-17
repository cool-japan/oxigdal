//! Non-UTM projected CRS definitions for the EPSG database.

#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;

use super::types::{CrsType, EpsgDatabase, EpsgDefinition};

/// Register all non-UTM projected CRS definitions into the database.
pub(crate) fn register_projected_crs(db: &mut EpsgDatabase) {
    register_mercator(db);
    register_national_grids(db);
    register_world_projections(db);
    register_us_state_planes(db);
    register_regional_systems(db);
    register_additional_projected(db);
}

fn register_mercator(db: &mut EpsgDatabase) {
    // Web Mercator
    db.add_definition(EpsgDefinition {
        code: 3857,
        name: "WGS 84 / Pseudo-Mercator".to_string(),
        proj_string: "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +wktext +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World between 85.06°S and 85.06°N".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // WGS 84 / World Mercator
    db.add_definition(EpsgDefinition {
        code: 3395,
        name: "WGS 84 / World Mercator".to_string(),
        proj_string: "+proj=merc +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World between 80°S and 84°N".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // WGS 84 / PDC Mercator
    db.add_definition(EpsgDefinition {
        code: 3832,
        name: "WGS 84 / PDC Mercator".to_string(),
        proj_string: "+proj=merc +lon_0=150 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Pacific Ocean area".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });
}

fn register_national_grids(db: &mut EpsgDatabase) {
    // OSGB 1936 / British National Grid
    db.add_definition(EpsgDefinition {
        code: 27700,
        name: "OSGB 1936 / British National Grid".to_string(),
        proj_string: "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 +ellps=airy +datum=OSGB36 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "United Kingdom".to_string(),
        unit: "metre".to_string(),
        datum: "OSGB36".to_string(),
    });

    // German DHDN Gauss-Kruger zones
    let dhdn_zones = [
        (31466u32, 2u32, 6000000.0_f64, "Germany — 6°E to 8°E"),
        (31467, 3, 9000000.0, "Germany — 8°E to 10°E"),
        (31468, 4, 12000000.0, "Germany — 10°E to 12°E"),
        (31469, 5, 15000000.0, "Germany — 12°E to 14°E"),
    ];
    for (code, zone, fe, aou) in dhdn_zones {
        let lon_0 = zone as f64 * 3.0;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("DHDN / 3-degree Gauss-Kruger zone {}", zone),
            proj_string: format!(
                "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0={} +y_0=0 +ellps=bessel +towgs84=598.1,73.7,418.2,0.202,0.045,2.455,6.7 +units=m +no_defs",
                lon_0, fe as u64
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: aou.to_string(),
            unit: "metre".to_string(),
            datum: "DHDN".to_string(),
        });
    }

    // French RGF93 — Lambert-93 (EPSG:2154)
    db.add_definition(EpsgDefinition {
        code: 2154,
        name: "RGF93 v1 / Lambert-93".to_string(),
        proj_string: "+proj=lcc +lat_0=46.5 +lon_0=3 +lat_1=44 +lat_2=49 +x_0=700000 +y_0=6600000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "France".to_string(),
        unit: "metre".to_string(),
        datum: "RGF93".to_string(),
    });

    // RGF93 CC zones (EPSG:3942–3950)
    for (zone_idx, code) in (3942u32..=3950).enumerate() {
        let lat_0 = 42.0 + zone_idx as f64;
        let lat_1 = lat_0 - 0.75;
        let lat_2 = lat_0 + 0.75;
        let y_0 = 1_200_000.0 + zone_idx as f64 * 1_000_000.0;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("RGF93 v1 / CC{}", 42 + zone_idx),
            proj_string: format!(
                "+proj=lcc +lat_0={} +lon_0=3 +lat_1={} +lat_2={} +x_0=1700000 +y_0={} +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                lat_0, lat_1, lat_2, y_0 as u64
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("France — CC{} zone", 42 + zone_idx),
            unit: "metre".to_string(),
            datum: "RGF93".to_string(),
        });
    }

    // TM65 Irish Grid (EPSG:29903)
    db.add_definition(EpsgDefinition {
        code: 29903,
        name: "TM65 / Irish Grid".to_string(),
        proj_string: "+proj=tmerc +lat_0=53.5 +lon_0=-8 +k=1.000035 +x_0=200000 +y_0=250000 +ellps=mod_airy +towgs84=482.5,-130.6,564.6,-1.042,-0.214,-0.631,8.15 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Ireland".to_string(),
        unit: "metre".to_string(),
        datum: "TM65".to_string(),
    });

    // Canadian zones
    let canadian_zones: &[(u32, &str, &str)] = &[
        (
            2294,
            "NAD83 / Prince Edward I.",
            "+proj=tmerc +lat_0=0 +lon_0=-63 +k=0.9999 +x_0=300000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2295,
            "NAD83 / Nova Scotia 2010",
            "+proj=tmerc +lat_0=0 +lon_0=-61.5 +k=0.9999 +x_0=300000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2296,
            "NAD83 / Sterea Netherlands",
            "+proj=sterea +lat_0=52.15617 +lon_0=5.38721 +k=0.9999079 +x_0=155000 +y_0=463000 +ellps=bessel +towgs84=565.4,50.3,465.2,0,0,0,0 +units=m +no_defs",
        ),
        (
            3157,
            "NAD83(CSRS) / UTM zone 9N",
            "+proj=utm +zone=9 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
        ),
        (
            3158,
            "NAD83(CSRS) / UTM zone 10N",
            "+proj=utm +zone=10 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
        ),
    ];
    for (code, name, proj) in canadian_zones {
        db.add_definition(EpsgDefinition {
            code: *code,
            name: name.to_string(),
            proj_string: proj.to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Canada".to_string(),
            unit: "metre".to_string(),
            datum: "NAD83".to_string(),
        });
    }

    // MGI / Transverse Mercator (Austria)
    db.add_definition(EpsgDefinition {
        code: 31257,
        name: "MGI / Austria GK West".to_string(),
        proj_string: "+proj=tmerc +lat_0=0 +lon_0=10.33333 +k=1 +x_0=0 +y_0=-5000000 +ellps=bessel +towgs84=577.326,90.129,463.919,5.137,1.474,5.297,2.4232 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Austria West".to_string(),
        unit: "metre".to_string(),
        datum: "MGI".to_string(),
    });

    // Swiss CH1903 / LV95 (EPSG:2056)
    db.add_definition(EpsgDefinition {
        code: 2056,
        name: "CH1903+ / LV95".to_string(),
        proj_string: "+proj=somerc +lat_0=46.9524056 +lon_0=7.4395833 +k_0=1 +x_0=2600000 +y_0=1200000 +ellps=bessel +towgs84=674.374,15.056,405.346,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Switzerland".to_string(),
        unit: "metre".to_string(),
        datum: "CH1903+".to_string(),
    });

    // NZGD2000 / New Zealand Transverse Mercator 2000 (EPSG:2193)
    db.add_definition(EpsgDefinition {
        code: 2193,
        name: "NZGD2000 / New Zealand Transverse Mercator 2000".to_string(),
        proj_string: "+proj=tmerc +lat_0=0 +lon_0=173 +k=0.9996 +x_0=1600000 +y_0=10000000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "New Zealand".to_string(),
        unit: "metre".to_string(),
        datum: "NZGD2000".to_string(),
    });

    // Amersfoort / RD New (EPSG:28992)
    db.add_definition(EpsgDefinition {
        code: 28992,
        name: "Amersfoort / RD New".to_string(),
        proj_string: "+proj=sterea +lat_0=52.15617 +lon_0=5.38721 +k=0.9999079 +x_0=155000 +y_0=463000 +ellps=bessel +towgs84=565.4,50.3,465.2,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Netherlands".to_string(),
        unit: "metre".to_string(),
        datum: "Amersfoort".to_string(),
    });

    // Belgium — Belgian Lambert 2008 (EPSG:3812)
    db.add_definition(EpsgDefinition {
        code: 3812,
        name: "ETRS89 / Belgian Lambert 2008".to_string(),
        proj_string: "+proj=lcc +lat_0=50.797815 +lon_0=4.35921583 +lat_1=49.8333333 +lat_2=51.1666667 +x_0=649328 +y_0=665262 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Belgium".to_string(),
        unit: "metre".to_string(),
        datum: "ETRS89".to_string(),
    });

    // Sweden — SWEREF99 TM (EPSG:3006)
    db.add_definition(EpsgDefinition {
        code: 3006,
        name: "SWEREF99 TM".to_string(),
        proj_string: "+proj=utm +zone=33 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Sweden".to_string(),
        unit: "metre".to_string(),
        datum: "SWEREF99".to_string(),
    });

    // Poland — ETRS89 / Poland CS92 (EPSG:2180)
    db.add_definition(EpsgDefinition {
        code: 2180,
        name: "ETRS89 / Poland CS92".to_string(),
        proj_string: "+proj=tmerc +lat_0=0 +lon_0=19 +k=0.9993 +x_0=500000 +y_0=-5300000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Poland".to_string(),
        unit: "metre".to_string(),
        datum: "ETRS89".to_string(),
    });

    // Czech / Slovak — S-JTSK (EPSG:5514)
    db.add_definition(EpsgDefinition {
        code: 5514,
        name: "S-JTSK/05 (Ferro) / Krovak East North".to_string(),
        proj_string: "+proj=krovak +lat_0=49.5 +lon_0=24.8333333 +alpha=30.2881397 +k=0.9999 +x_0=0 +y_0=0 +ellps=bessel +pm=ferro +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Czech Republic and Slovakia".to_string(),
        unit: "metre".to_string(),
        datum: "S-JTSK".to_string(),
    });

    // Israel — ITM (EPSG:2039)
    db.add_definition(EpsgDefinition {
        code: 2039,
        name: "Israel 1993 / Israeli TM Grid".to_string(),
        proj_string: "+proj=tmerc +lat_0=31.7343936 +lon_0=35.2045169 +k=1.0000067 +x_0=219529.584 +y_0=626907.39 +ellps=GRS80 +towgs84=-48,55,52,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Israel".to_string(),
        unit: "metre".to_string(),
        datum: "Israel".to_string(),
    });

    // Finland — ETRS-TM35FIN (EPSG:3067)
    db.add_definition(EpsgDefinition {
        code: 3067,
        name: "ETRS89 / TM35FIN(E,N)".to_string(),
        proj_string: "+proj=utm +zone=35 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Finland".to_string(),
        unit: "metre".to_string(),
        datum: "ETRS89".to_string(),
    });

    // Denmark — DKTM1 to DKTM4
    for (zone, lon_0, code) in [
        (1u32, 9.0_f64, 4093u32),
        (2, 10.0, 4094),
        (3, 11.0, 4095),
        (4, 12.0, 4096),
    ] {
        db.add_definition(EpsgDefinition {
            code,
            name: format!("ETRS89 / DKTM{}", zone),
            proj_string: format!("+proj=tmerc +lat_0=0 +lon_0={} +k=0.9999 +x_0=200000 +y_0=-5000000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs", lon_0),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("Denmark — DKTM{}", zone),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });
    }

    // Korea — Korea 2000 / Unified CS (EPSG:5179)
    db.add_definition(EpsgDefinition {
        code: 5179,
        name: "Korea 2000 / Unified CS".to_string(),
        proj_string: "+proj=tmerc +lat_0=38 +lon_0=127.5 +k=0.9996 +x_0=1000000 +y_0=2000000 +ellps=GRS80 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "South Korea".to_string(),
        unit: "metre".to_string(),
        datum: "GRS80".to_string(),
    });

    // Morocco — Nord Maroc zone (EPSG:26191)
    db.add_definition(EpsgDefinition {
        code: 26191,
        name: "Merchich / Nord Maroc".to_string(),
        proj_string: "+proj=lcc +lat_0=33.3 +lon_0=-5.4 +lat_1=35.1666667 +lat_2=31.5 +x_0=500000 +y_0=300000 +ellps=clrk80ign +towgs84=31,146,47,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Morocco".to_string(),
        unit: "metre".to_string(),
        datum: "Merchich".to_string(),
    });

    // GDM2000 / Peninsula RSO (EPSG:3376)
    db.add_definition(EpsgDefinition {
        code: 3376,
        name: "GDM2000 / Peninsula RSO".to_string(),
        proj_string: "+proj=omerc +lat_0=4 +lonc=102.25 +alpha=323.1301023611 +k=0.99984 +x_0=804671 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Malaysia".to_string(),
        unit: "metre".to_string(),
        datum: "GDM2000".to_string(),
    });
}

fn register_world_projections(db: &mut EpsgDatabase) {
    // ETRS89 / LCC Europe (EPSG:3034)
    db.add_definition(EpsgDefinition {
        code: 3034,
        name: "ETRS89 / LCC Europe".to_string(),
        proj_string: "+proj=lcc +lat_1=35 +lat_2=65 +lat_0=52 +lon_0=10 +x_0=4000000 +y_0=2800000 +ellps=GRS80 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Europe".to_string(),
        unit: "metre".to_string(),
        datum: "ETRS89".to_string(),
    });

    // US National Atlas Equal Area (EPSG:2163)
    db.add_definition(EpsgDefinition {
        code: 2163,
        name: "US National Atlas Equal Area".to_string(),
        proj_string:
            "+proj=laea +lat_0=45 +lon_0=-100 +x_0=0 +y_0=0 +a=6370997 +b=6370997 +units=m +no_defs"
                .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "United States".to_string(),
        unit: "metre".to_string(),
        datum: "Sphere".to_string(),
    });

    // Polar stereographic
    db.add_definition(EpsgDefinition {
        code: 3413,
        name: "WGS 84 / NSIDC Sea Ice Polar Stereographic North".to_string(),
        proj_string: "+proj=stere +lat_0=90 +lat_ts=70 +lon_0=-45 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Northern hemisphere - north of 60°N".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    db.add_definition(EpsgDefinition {
        code: 3031,
        name: "WGS 84 / Antarctic Polar Stereographic".to_string(),
        proj_string: "+proj=stere +lat_0=-90 +lat_ts=-71 +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Antarctica".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // ETRS89-extended / LAEA Europe (EPSG:3035)
    db.add_definition(EpsgDefinition {
        code: 3035,
        name: "ETRS89-extended / LAEA Europe".to_string(),
        proj_string: "+proj=laea +lat_0=52 +lon_0=10 +x_0=4321000 +y_0=3210000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Europe".to_string(),
        unit: "metre".to_string(),
        datum: "ETRS89".to_string(),
    });

    // World Sinusoidal (EPSG:54008)
    db.add_definition(EpsgDefinition {
        code: 54008,
        name: "World Sinusoidal".to_string(),
        proj_string: "+proj=sinu +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // World Mollweide (EPSG:54009)
    db.add_definition(EpsgDefinition {
        code: 54009,
        name: "World Mollweide".to_string(),
        proj_string: "+proj=moll +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // World Robinson (EPSG:54030)
    db.add_definition(EpsgDefinition {
        code: 54030,
        name: "World Robinson".to_string(),
        proj_string: "+proj=robin +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // World Equal Earth (EPSG:8857)
    db.add_definition(EpsgDefinition {
        code: 8857,
        name: "WGS 84 / Equal Earth Greenwich".to_string(),
        proj_string: "+proj=eqearth +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // WGS 84 / Plate Carree (EPSG:32662)
    db.add_definition(EpsgDefinition {
        code: 32662,
        name: "WGS 84 / Plate Carree".to_string(),
        proj_string:
            "+proj=eqc +lat_ts=0 +lat_0=0 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // WGS 84 / Arctic Polar Stereographic (EPSG:3995)
    db.add_definition(EpsgDefinition {
        code: 3995,
        name: "WGS 84 / Arctic Polar Stereographic".to_string(),
        proj_string: "+proj=stere +lat_0=90 +lat_ts=71 +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Arctic".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // WGS84 / North Pole LAEA Atlantic (EPSG:3574)
    db.add_definition(EpsgDefinition {
        code: 3574,
        name: "WGS 84 / North Pole LAEA Atlantic".to_string(),
        proj_string: "+proj=laea +lat_0=90 +lon_0=-40 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "North Polar region — Atlantic sector".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // WGS84 North Pole LAEA Europe (EPSG:3575)
    db.add_definition(EpsgDefinition {
        code: 3575,
        name: "WGS 84 / North Pole LAEA Europe".to_string(),
        proj_string: "+proj=laea +lat_0=90 +lon_0=10 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "North Polar region — European sector".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // World Azimuthal Equidistant (EPSG:54032)
    db.add_definition(EpsgDefinition {
        code: 54032,
        name: "World Azimuthal Equidistant".to_string(),
        proj_string: "+proj=aeqd +lat_0=0 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // World Gnomonic (EPSG:54016)
    db.add_definition(EpsgDefinition {
        code: 54016,
        name: "World Gnomonic".to_string(),
        proj_string: "+proj=gnom +lat_0=90 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Arctic region".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // Eckert IV (EPSG:54012)
    db.add_definition(EpsgDefinition {
        code: 54012,
        name: "World Eckert IV".to_string(),
        proj_string: "+proj=eck4 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });

    // Eckert VI (EPSG:54010)
    db.add_definition(EpsgDefinition {
        code: 54010,
        name: "World Eckert VI".to_string(),
        proj_string: "+proj=eck6 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "World".to_string(),
        unit: "metre".to_string(),
        datum: "WGS84".to_string(),
    });
}

fn register_us_state_planes(db: &mut EpsgDatabase) {
    let us_state_planes: &[(u32, &str, &str)] = &[
        (
            32100,
            "NAD83 / Montana",
            "+proj=lcc +lat_0=44.25 +lon_0=-109.5 +lat_1=45 +lat_2=49 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32104,
            "NAD83 / Nebraska",
            "+proj=lcc +lat_0=39.83333 +lon_0=-100 +lat_1=40 +lat_2=43 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32107,
            "NAD83 / New Jersey",
            "+proj=tmerc +lat_0=38.83333 +lon_0=-74.5 +k=0.9999 +x_0=150000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32110,
            "NAD83 / New York East",
            "+proj=tmerc +lat_0=38.83333 +lon_0=-74.5 +k=0.9999 +x_0=150000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32111,
            "NAD83 / New York Central",
            "+proj=tmerc +lat_0=40 +lon_0=-76.58333 +k=0.9999375 +x_0=250000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32112,
            "NAD83 / New York West",
            "+proj=tmerc +lat_0=40 +lon_0=-78.58333 +k=0.9999375 +x_0=350000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32113,
            "NAD83 / New York Long Island",
            "+proj=lcc +lat_0=40.16667 +lon_0=-74 +lat_1=40.66667 +lat_2=41.03333 +x_0=300000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32118,
            "NAD83 / Ohio North",
            "+proj=lcc +lat_0=39.66667 +lon_0=-82.5 +lat_1=40.43333 +lat_2=41.7 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32119,
            "NAD83 / North Carolina",
            "+proj=lcc +lat_0=33.75 +lon_0=-79 +lat_1=34.33333 +lat_2=36.16667 +x_0=609601.22 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32120,
            "NAD83 / North Dakota North",
            "+proj=lcc +lat_0=47 +lon_0=-100.5 +lat_1=47.43333 +lat_2=48.73333 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32128,
            "NAD83 / Oregon North",
            "+proj=lcc +lat_0=43.66667 +lon_0=-120.5 +lat_1=44.33333 +lat_2=46 +x_0=2500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32133,
            "NAD83 / Pennsylvania South",
            "+proj=lcc +lat_0=39.33333 +lon_0=-77.75 +lat_1=39.93333 +lat_2=40.96667 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32140,
            "NAD83 / Texas Central",
            "+proj=lcc +lat_0=29.66667 +lon_0=-100.33333 +lat_1=30.11667 +lat_2=31.88333 +x_0=700000 +y_0=3000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32145,
            "NAD83 / Vermont",
            "+proj=tmerc +lat_0=42.5 +lon_0=-72.5 +k=0.9999643 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32148,
            "NAD83 / Washington North",
            "+proj=lcc +lat_0=47 +lon_0=-120.83333 +lat_1=47.5 +lat_2=48.73333 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32149,
            "NAD83 / Washington South",
            "+proj=lcc +lat_0=45.33333 +lon_0=-120.5 +lat_1=45.83333 +lat_2=47.33333 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32150,
            "NAD83 / Wisconsin Central",
            "+proj=lcc +lat_0=43.83333 +lon_0=-90 +lat_1=44.25 +lat_2=45.5 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32154,
            "NAD83 / Wyoming East",
            "+proj=tmerc +lat_0=40.5 +lon_0=-105.16667 +k=0.9999375 +x_0=200000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32158,
            "NAD83 / Wyoming West",
            "+proj=tmerc +lat_0=40.5 +lon_0=-110.08333 +k=0.9999375 +x_0=800000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
    ];
    for (code, name, proj) in us_state_planes {
        db.add_definition(EpsgDefinition {
            code: *code,
            name: name.to_string(),
            proj_string: proj.to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "United States".to_string(),
            unit: "metre".to_string(),
            datum: "NAD83".to_string(),
        });
    }

    // Additional NAD83 State Planes (Alaska)
    let more_state_planes: &[(u32, &str, &str)] = &[
        (
            32061,
            "NAD83 / Alaska zone 1",
            "+proj=omerc +lat_0=57 +lonc=-133.6666667 +alpha=323.1301 +k=0.9999 +x_0=5000000 +y_0=-5000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32064,
            "NAD83 / Alaska zone 4",
            "+proj=tmerc +lat_0=54 +lon_0=-150 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32065,
            "NAD83 / Alaska zone 5",
            "+proj=tmerc +lat_0=54 +lon_0=-156 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32066,
            "NAD83 / Alaska zone 6",
            "+proj=tmerc +lat_0=54 +lon_0=-162 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32067,
            "NAD83 / Alaska zone 7",
            "+proj=tmerc +lat_0=54 +lon_0=-168 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32068,
            "NAD83 / Alaska zone 8",
            "+proj=tmerc +lat_0=54 +lon_0=-166 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32069,
            "NAD83 / Alaska zone 9",
            "+proj=tmerc +lat_0=54 +lon_0=-168 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
    ];
    for (code, name, proj) in more_state_planes {
        db.add_definition(EpsgDefinition {
            code: *code,
            name: name.to_string(),
            proj_string: proj.to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "United States — Alaska".to_string(),
            unit: "metre".to_string(),
            datum: "NAD83".to_string(),
        });
    }
}

fn register_regional_systems(db: &mut EpsgDatabase) {
    // South Africa — Lo19-Lo33 series
    for lo in (19u32..=33).step_by(2) {
        let code = 2046 + (lo - 19) / 2;
        db.add_definition(EpsgDefinition {
            code,
            name: format!("Hartebeesthoek94 / Lo{}", lo),
            proj_string: format!(
                "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                lo
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("South Africa — Lo{} strip", lo),
            unit: "metre".to_string(),
            datum: "Hartebeesthoek94".to_string(),
        });
    }

    // India zone systems (EPSG:24378–24385)
    let india_zones: &[(u32, f64, &str)] = &[
        (24378, 68.0, "India zone I"),
        (24379, 74.0, "India zone IIa"),
        (24380, 80.0, "India zone IIb"),
        (24381, 80.0, "India zone IIIa"),
        (24382, 86.0, "India zone IIIb"),
        (24383, 86.0, "India zone IVa"),
        (24384, 90.0, "India zone IVb"),
        (24385, 90.0, "India zone 0"),
    ];
    for (code, lon_0, aou) in india_zones {
        db.add_definition(EpsgDefinition {
            code: *code,
            name: format!("Kalianpur 1937 / {}", aou),
            proj_string: format!(
                "+proj=lcc +lat_0=0 +lon_0={} +lat_1=26 +lat_2=30 +x_0=2743196.4 +y_0=914398.8 +a=6377276.345 +b=6356075.413 +units=m +no_defs",
                lon_0
            ),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: format!("India — {}", aou),
            unit: "metre".to_string(),
            datum: "Kalianpur1937".to_string(),
        });
    }
}

fn register_additional_projected(db: &mut EpsgDatabase) {
    // Trinidad 1903 / Trinidad Grid (EPSG:2314)
    db.add_definition(EpsgDefinition {
        code: 2314,
        name: "Trinidad 1903 / Trinidad Grid".to_string(),
        proj_string: "+proj=cass +lat_0=10.44166 +lon_0=-61.33333 +x_0=86501.46 +y_0=65379.013 +a=6378293.645 +b=6356617.987 +towgs84=-61.702,284.488,472.052,0,0,0,0 +units=lk +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Projected,
        area_of_use: "Trinidad and Tobago".to_string(),
        unit: "link".to_string(),
        datum: "Trinidad1903".to_string(),
    });
}
