//! African national grid CRS registrations.

use super::super::types::{CrsType, EpsgDatabase, EpsgDefinition};
use alloc::string::ToString;

pub(super) fn register_african_grids(db: &mut EpsgDatabase) {
    let african_grids: &[(u32, &str, &str, &str, &str)] = &[
        // South African grids (Hartebeesthoek94 / Lo-series)
        (
            2046,
            "Hartebeesthoek94 / Lo15",
            "+proj=tmerc +lat_0=0 +lon_0=15 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo15",
            "Hartebeesthoek94",
        ),
        (
            2047,
            "Hartebeesthoek94 / Lo17",
            "+proj=tmerc +lat_0=0 +lon_0=17 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo17",
            "Hartebeesthoek94",
        ),
        (
            2048,
            "Hartebeesthoek94 / Lo19",
            "+proj=tmerc +lat_0=0 +lon_0=19 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo19",
            "Hartebeesthoek94",
        ),
        (
            2049,
            "Hartebeesthoek94 / Lo21",
            "+proj=tmerc +lat_0=0 +lon_0=21 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo21",
            "Hartebeesthoek94",
        ),
        (
            2050,
            "Hartebeesthoek94 / Lo23",
            "+proj=tmerc +lat_0=0 +lon_0=23 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo23",
            "Hartebeesthoek94",
        ),
        (
            2051,
            "Hartebeesthoek94 / Lo25",
            "+proj=tmerc +lat_0=0 +lon_0=25 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo25",
            "Hartebeesthoek94",
        ),
        (
            2052,
            "Hartebeesthoek94 / Lo27",
            "+proj=tmerc +lat_0=0 +lon_0=27 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo27",
            "Hartebeesthoek94",
        ),
        (
            2053,
            "Hartebeesthoek94 / Lo29",
            "+proj=tmerc +lat_0=0 +lon_0=29 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo29",
            "Hartebeesthoek94",
        ),
        (
            2054,
            "Hartebeesthoek94 / Lo31",
            "+proj=tmerc +lat_0=0 +lon_0=31 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo31",
            "Hartebeesthoek94",
        ),
        (
            2055,
            "Hartebeesthoek94 / Lo33",
            "+proj=tmerc +lat_0=0 +lon_0=33 +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            "South Africa Lo33",
            "Hartebeesthoek94",
        ),
        // Egyptian grids
        (
            22992,
            "Egypt 1907 / Blue Belt",
            "+proj=tmerc +lat_0=30 +lon_0=35 +k=1 +x_0=300000 +y_0=1100000 +ellps=helmert +units=m +no_defs",
            "Egypt East (Sinai)",
            "Egypt 1907",
        ),
        (
            22993,
            "Egypt 1907 / Purple Belt",
            "+proj=tmerc +lat_0=30 +lon_0=33 +k=1 +x_0=300000 +y_0=1100000 +ellps=helmert +units=m +no_defs",
            "Egypt Central",
            "Egypt 1907",
        ),
        (
            22994,
            "Egypt 1907 / Extended Purple Belt",
            "+proj=tmerc +lat_0=30 +lon_0=27 +k=1 +x_0=700000 +y_0=200000 +ellps=helmert +units=m +no_defs",
            "Egypt West",
            "Egypt 1907",
        ),
        // Nigerian grids
        (
            26331,
            "Minna / UTM zone 31N",
            "+proj=utm +zone=31 +ellps=clrk80 +units=m +no_defs",
            "Nigeria West",
            "Minna",
        ),
        (
            26332,
            "Minna / UTM zone 32N",
            "+proj=utm +zone=32 +ellps=clrk80 +units=m +no_defs",
            "Nigeria East",
            "Minna",
        ),
        // Kenyan grid
        (
            21097,
            "Arc 1960 / UTM zone 37S",
            "+proj=utm +zone=37 +south +ellps=clrk80 +units=m +no_defs",
            "Kenya",
            "Arc 1960",
        ),
        // Moroccan grid
        (
            26191,
            "Merchich / Nord Maroc",
            "+proj=lcc +lat_1=33.3 +lat_0=33.3 +lon_0=-5.4 +k_0=0.999625769 +x_0=500000 +y_0=300000 +a=6378249.2 +b=6356515 +units=m +no_defs",
            "Morocco North",
            "Merchich",
        ),
        (
            26192,
            "Merchich / Sud Maroc",
            "+proj=lcc +lat_1=29.7 +lat_0=29.7 +lon_0=-5.4 +k_0=0.999615596 +x_0=500000 +y_0=300000 +a=6378249.2 +b=6356515 +units=m +no_defs",
            "Morocco South",
            "Merchich",
        ),
        // Tunisian grid
        (
            22300,
            "Carthage / UTM zone 32N",
            "+proj=utm +zone=32 +a=6378249.2 +b=6356515 +units=m +no_defs",
            "Tunisia",
            "Carthage",
        ),
        // Algerian grid
        (
            30791,
            "Nord Sahara 1959 / UTM zone 29N",
            "+proj=utm +zone=29 +ellps=clrk80ign +units=m +no_defs",
            "Algeria West",
            "Nord Sahara 1959",
        ),
        (
            30792,
            "Nord Sahara 1959 / UTM zone 30N",
            "+proj=utm +zone=30 +ellps=clrk80ign +units=m +no_defs",
            "Algeria Central",
            "Nord Sahara 1959",
        ),
        (
            30793,
            "Nord Sahara 1959 / UTM zone 31N",
            "+proj=utm +zone=31 +ellps=clrk80ign +units=m +no_defs",
            "Algeria East",
            "Nord Sahara 1959",
        ),
        (
            30794,
            "Nord Sahara 1959 / UTM zone 32N",
            "+proj=utm +zone=32 +ellps=clrk80ign +units=m +no_defs",
            "Algeria Far East",
            "Nord Sahara 1959",
        ),
        // Tanzanian grid
        (
            21036,
            "Arc 1960 / UTM zone 36S",
            "+proj=utm +zone=36 +south +ellps=clrk80 +units=m +no_defs",
            "Tanzania",
            "Arc 1960",
        ),
        // Ghana grid
        (
            25000,
            "Leigon / Ghana Metre Grid",
            "+proj=tmerc +lat_0=4.666667 +lon_0=-1.0 +k=0.99975 +x_0=274319.7391 +y_0=0 +ellps=clrk80 +units=m +no_defs",
            "Ghana",
            "Leigon",
        ),
    ];

    for &(code, name, proj_string, area, datum) in african_grids {
        db.add_definition(EpsgDefinition {
            code,
            name: name.to_string(),
            proj_string: proj_string.to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: area.to_string(),
            unit: "metre".to_string(),
            datum: datum.to_string(),
        });
    }
}
