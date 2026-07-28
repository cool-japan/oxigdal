//! South American national grid CRS registrations.

use super::super::types::{CrsType, EpsgDatabase, EpsgDefinition};
use alloc::string::ToString;

pub(super) fn register_south_american_grids(db: &mut EpsgDatabase) {
    let sa_grids: &[(u32, &str, &str, &str, &str)] = &[
        // SIRGAS 2000 zones (Brazil)
        (
            31981,
            "SIRGAS 2000 / UTM zone 17S",
            "+proj=utm +zone=17 +south +ellps=GRS80 +units=m +no_defs",
            "Brazil West",
            "SIRGAS 2000",
        ),
        (
            31982,
            "SIRGAS 2000 / UTM zone 18S",
            "+proj=utm +zone=18 +south +ellps=GRS80 +units=m +no_defs",
            "Brazil",
            "SIRGAS 2000",
        ),
        (
            31983,
            "SIRGAS 2000 / UTM zone 19S",
            "+proj=utm +zone=19 +south +ellps=GRS80 +units=m +no_defs",
            "Brazil",
            "SIRGAS 2000",
        ),
        (
            31984,
            "SIRGAS 2000 / UTM zone 20S",
            "+proj=utm +zone=20 +south +ellps=GRS80 +units=m +no_defs",
            "Brazil",
            "SIRGAS 2000",
        ),
        (
            31985,
            "SIRGAS 2000 / UTM zone 21S",
            "+proj=utm +zone=21 +south +ellps=GRS80 +units=m +no_defs",
            "Brazil",
            "SIRGAS 2000",
        ),
        (
            31986,
            "SIRGAS 2000 / UTM zone 22S",
            "+proj=utm +zone=22 +south +ellps=GRS80 +units=m +no_defs",
            "Brazil East",
            "SIRGAS 2000",
        ),
        (
            31987,
            "SIRGAS 2000 / UTM zone 23S",
            "+proj=utm +zone=23 +south +ellps=GRS80 +units=m +no_defs",
            "Brazil",
            "SIRGAS 2000",
        ),
        (
            31988,
            "SIRGAS 2000 / UTM zone 24S",
            "+proj=utm +zone=24 +south +ellps=GRS80 +units=m +no_defs",
            "Brazil East",
            "SIRGAS 2000",
        ),
        (
            31989,
            "SIRGAS 2000 / UTM zone 25S",
            "+proj=utm +zone=25 +south +ellps=GRS80 +units=m +no_defs",
            "Brazil + Atlantic",
            "SIRGAS 2000",
        ),
        // Argentine grids
        (
            5343,
            "POSGAR 2007 / Argentina zone 1",
            "+proj=tmerc +lat_0=-90 +lon_0=-72 +k=1 +x_0=1500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Argentina zone 1",
            "POSGAR 2007",
        ),
        (
            5344,
            "POSGAR 2007 / Argentina zone 2",
            "+proj=tmerc +lat_0=-90 +lon_0=-69 +k=1 +x_0=2500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Argentina zone 2",
            "POSGAR 2007",
        ),
        (
            5345,
            "POSGAR 2007 / Argentina zone 3",
            "+proj=tmerc +lat_0=-90 +lon_0=-66 +k=1 +x_0=3500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Argentina zone 3",
            "POSGAR 2007",
        ),
        (
            5346,
            "POSGAR 2007 / Argentina zone 4",
            "+proj=tmerc +lat_0=-90 +lon_0=-63 +k=1 +x_0=4500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Argentina zone 4",
            "POSGAR 2007",
        ),
        (
            5347,
            "POSGAR 2007 / Argentina zone 5",
            "+proj=tmerc +lat_0=-90 +lon_0=-60 +k=1 +x_0=5500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Argentina zone 5",
            "POSGAR 2007",
        ),
        (
            5348,
            "POSGAR 2007 / Argentina zone 6",
            "+proj=tmerc +lat_0=-90 +lon_0=-57 +k=1 +x_0=6500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Argentina zone 6",
            "POSGAR 2007",
        ),
        (
            5349,
            "POSGAR 2007 / Argentina zone 7",
            "+proj=tmerc +lat_0=-90 +lon_0=-54 +k=1 +x_0=7500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Argentina zone 7",
            "POSGAR 2007",
        ),
        // Chilean grid
        (
            5361,
            "SIRGAS-Chile 2002 / UTM zone 18S",
            "+proj=utm +zone=18 +south +ellps=GRS80 +units=m +no_defs",
            "Chile North",
            "SIRGAS-Chile",
        ),
        (
            5362,
            "SIRGAS-Chile 2002 / UTM zone 19S",
            "+proj=utm +zone=19 +south +ellps=GRS80 +units=m +no_defs",
            "Chile Central",
            "SIRGAS-Chile",
        ),
        // Colombian grids
        (
            3116,
            "MAGNA-SIRGAS / Colombia Bogota zone",
            "+proj=tmerc +lat_0=4.5962 +lon_0=-74.0775 +k=1 +x_0=1000000 +y_0=1000000 +ellps=GRS80 +units=m +no_defs",
            "Colombia Bogota",
            "MAGNA-SIRGAS",
        ),
        (
            3117,
            "MAGNA-SIRGAS / Colombia East Central zone",
            "+proj=tmerc +lat_0=4.5962 +lon_0=-71.0775 +k=1 +x_0=1000000 +y_0=1000000 +ellps=GRS80 +units=m +no_defs",
            "Colombia East Central",
            "MAGNA-SIRGAS",
        ),
        (
            3118,
            "MAGNA-SIRGAS / Colombia East zone",
            "+proj=tmerc +lat_0=4.5962 +lon_0=-68.0775 +k=1 +x_0=1000000 +y_0=1000000 +ellps=GRS80 +units=m +no_defs",
            "Colombia East",
            "MAGNA-SIRGAS",
        ),
        (
            3114,
            "MAGNA-SIRGAS / Colombia Far West zone",
            "+proj=tmerc +lat_0=4.5962 +lon_0=-80.0775 +k=1 +x_0=1000000 +y_0=1000000 +ellps=GRS80 +units=m +no_defs",
            "Colombia Far West",
            "MAGNA-SIRGAS",
        ),
        (
            3115,
            "MAGNA-SIRGAS / Colombia West zone",
            "+proj=tmerc +lat_0=4.5962 +lon_0=-77.0775 +k=1 +x_0=1000000 +y_0=1000000 +ellps=GRS80 +units=m +no_defs",
            "Colombia West",
            "MAGNA-SIRGAS",
        ),
        // Peruvian grid
        (
            5389,
            "Peru96 / UTM zone 17S",
            "+proj=utm +zone=17 +south +ellps=GRS80 +units=m +no_defs",
            "Peru West",
            "Peru96",
        ),
        (
            5390,
            "Peru96 / UTM zone 18S",
            "+proj=utm +zone=18 +south +ellps=GRS80 +units=m +no_defs",
            "Peru Central",
            "Peru96",
        ),
        (
            5391,
            "Peru96 / UTM zone 19S",
            "+proj=utm +zone=19 +south +ellps=GRS80 +units=m +no_defs",
            "Peru East",
            "Peru96",
        ),
    ];

    for &(code, name, proj_string, area, datum) in sa_grids {
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
