//! Geographic CRS definitions (EPSG 4xxx codes and similar).

#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;

use super::types::{CrsType, EpsgDatabase, EpsgDefinition};

/// Register all geographic CRS definitions into the database.
pub(crate) fn register_geographic_crs(db: &mut EpsgDatabase) {
    // WGS84 - Most common geographic CRS
    db.add_definition(EpsgDefinition {
        code: 4326,
        name: "WGS 84".to_string(),
        proj_string: "+proj=longlat +datum=WGS84 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "World".to_string(),
        unit: "degree".to_string(),
        datum: "WGS84".to_string(),
    });

    // NAD83 - North American Datum 1983
    db.add_definition(EpsgDefinition {
        code: 4269,
        name: "NAD83".to_string(),
        proj_string: "+proj=longlat +datum=NAD83 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "North America".to_string(),
        unit: "degree".to_string(),
        datum: "NAD83".to_string(),
    });

    // ETRS89 - European Terrestrial Reference System 1989
    db.add_definition(EpsgDefinition {
        code: 4258,
        name: "ETRS89".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Europe".to_string(),
        unit: "degree".to_string(),
        datum: "ETRS89".to_string(),
    });

    // GDA94 - Geocentric Datum of Australia 1994
    db.add_definition(EpsgDefinition {
        code: 4283,
        name: "GDA94".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Australia".to_string(),
        unit: "degree".to_string(),
        datum: "GDA94".to_string(),
    });

    // JGD2000 - Japanese Geodetic Datum 2000
    db.add_definition(EpsgDefinition {
        code: 4612,
        name: "JGD2000".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Japan".to_string(),
        unit: "degree".to_string(),
        datum: "JGD2000".to_string(),
    });

    // NZGD2000 - New Zealand Geodetic Datum 2000
    db.add_definition(EpsgDefinition {
        code: 4167,
        name: "NZGD2000".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "New Zealand".to_string(),
        unit: "degree".to_string(),
        datum: "NZGD2000".to_string(),
    });

    // SIRGAS 2000
    db.add_definition(EpsgDefinition {
        code: 4674,
        name: "SIRGAS 2000".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Latin America".to_string(),
        unit: "degree".to_string(),
        datum: "SIRGAS2000".to_string(),
    });

    // China Geodetic Coordinate System 2000
    db.add_definition(EpsgDefinition {
        code: 4490,
        name: "China Geodetic Coordinate System 2000".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "China".to_string(),
        unit: "degree".to_string(),
        datum: "CGCS2000".to_string(),
    });

    // WGS 72
    db.add_definition(EpsgDefinition {
        code: 4322,
        name: "WGS 72".to_string(),
        proj_string: "+proj=longlat +ellps=WGS72 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "World".to_string(),
        unit: "degree".to_string(),
        datum: "WGS72".to_string(),
    });

    // JGD2011 geographic (EPSG:6668)
    db.add_definition(EpsgDefinition {
        code: 6668,
        name: "JGD2011".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Japan".to_string(),
        unit: "degree".to_string(),
        datum: "JGD2011".to_string(),
    });

    // GDA2020 geographic (EPSG:7844)
    db.add_definition(EpsgDefinition {
        code: 7844,
        name: "GDA2020".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Australia".to_string(),
        unit: "degree".to_string(),
        datum: "GDA2020".to_string(),
    });

    // NAD27 geographic (EPSG:4267)
    db.add_definition(EpsgDefinition {
        code: 4267,
        name: "NAD27".to_string(),
        proj_string: "+proj=longlat +datum=NAD27 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "North America".to_string(),
        unit: "degree".to_string(),
        datum: "NAD27".to_string(),
    });

    // Tokyo datum geographic (EPSG:4301)
    db.add_definition(EpsgDefinition {
        code: 4301,
        name: "Tokyo".to_string(),
        proj_string:
            "+proj=longlat +ellps=bessel +towgs84=-146.414,507.337,680.507,0,0,0,0 +no_defs"
                .to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Japan".to_string(),
        unit: "degree".to_string(),
        datum: "Tokyo".to_string(),
    });

    // DHDN geographic (EPSG:4314)
    db.add_definition(EpsgDefinition {
        code: 4314,
        name: "DHDN".to_string(),
        proj_string: "+proj=longlat +ellps=bessel +towgs84=598.1,-73.7,-418.2,0.202,0.045,-2.455,6.7 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Germany".to_string(),
        unit: "degree".to_string(),
        datum: "DHDN".to_string(),
    });

    // Pulkovo 1942 geographic (EPSG:4284)
    db.add_definition(EpsgDefinition {
        code: 4284,
        name: "Pulkovo 1942".to_string(),
        proj_string:
            "+proj=longlat +ellps=krass +towgs84=23.57,-140.95,-79.8,0,0.35,0.79,-0.22 +no_defs"
                .to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Russia and Eastern Europe".to_string(),
        unit: "degree".to_string(),
        datum: "Pulkovo1942".to_string(),
    });

    // MGI geographic (Austria, EPSG:4312)
    db.add_definition(EpsgDefinition {
        code: 4312,
        name: "MGI".to_string(),
        proj_string: "+proj=longlat +ellps=bessel +towgs84=577.326,90.129,463.919,5.137,1.474,5.297,2.4232 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Austria".to_string(),
        unit: "degree".to_string(),
        datum: "MGI".to_string(),
    });

    // Hartebeesthoek94 (EPSG:4148)
    db.add_definition(EpsgDefinition {
        code: 4148,
        name: "Hartebeesthoek94".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "South Africa".to_string(),
        unit: "degree".to_string(),
        datum: "Hartebeesthoek94".to_string(),
    });

    // PSAD56 (EPSG:4248)
    db.add_definition(EpsgDefinition {
        code: 4248,
        name: "PSAD56".to_string(),
        proj_string: "+proj=longlat +ellps=intl +towgs84=-296,519,-13,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "South America — PSAD56".to_string(),
        unit: "degree".to_string(),
        datum: "PSAD56".to_string(),
    });

    // REGVEN (EPSG:4189)
    db.add_definition(EpsgDefinition {
        code: 4189,
        name: "REGVEN".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Venezuela".to_string(),
        unit: "degree".to_string(),
        datum: "REGVEN".to_string(),
    });

    // Egyptian 1907 (EPSG:4229)
    db.add_definition(EpsgDefinition {
        code: 4229,
        name: "Egyptian 1907".to_string(),
        proj_string: "+proj=longlat +ellps=helmert +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Egypt".to_string(),
        unit: "degree".to_string(),
        datum: "Egyptian1907".to_string(),
    });

    // Minna (EPSG:4263)
    db.add_definition(EpsgDefinition {
        code: 4263,
        name: "Minna".to_string(),
        proj_string: "+proj=longlat +ellps=clrk80ign +towgs84=-92,-93,122,0,0,0,0 +no_defs"
            .to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Nigeria".to_string(),
        unit: "degree".to_string(),
        datum: "Minna".to_string(),
    });

    // KKJ (EPSG:4123)
    db.add_definition(EpsgDefinition {
        code: 4123,
        name: "KKJ".to_string(),
        proj_string: "+proj=longlat +ellps=intl +towgs84=-96.062,-82.428,-121.753,4.801,0.345,-1.376,1.496 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Finland".to_string(),
        unit: "degree".to_string(),
        datum: "KKJ".to_string(),
    });

    // POSGAR 98 (EPSG:4190)
    db.add_definition(EpsgDefinition {
        code: 4190,
        name: "POSGAR 98".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Argentina".to_string(),
        unit: "degree".to_string(),
        datum: "POSGAR98".to_string(),
    });

    // Korea — KGD2002 (EPSG:4737)
    db.add_definition(EpsgDefinition {
        code: 4737,
        name: "GRS 1980(IUGG, 1980)".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Korea".to_string(),
        unit: "degree".to_string(),
        datum: "GRS80".to_string(),
    });

    // JGD2000 geographic (duplicate safe — HashMap overwrites)
    db.add_definition(EpsgDefinition {
        code: 4612,
        name: "JGD2000".to_string(),
        proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "Japan".to_string(),
        unit: "degree".to_string(),
        datum: "JGD2000".to_string(),
    });
}
