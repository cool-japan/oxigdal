//! Polar (Arctic and Antarctic) CRS registrations.

use super::super::types::{CrsType, EpsgDatabase, EpsgDefinition};

pub(super) fn register_polar_grids(db: &mut EpsgDatabase) {
    let polar_grids: &[(u32, &str, &str, &str, &str)] = &[
        // Arctic
        (
            3411,
            "NSIDC Sea Ice Polar Stereographic North",
            "+proj=stere +lat_0=90 +lat_ts=70 +lon_0=-45 +k=1 +x_0=0 +y_0=0 +a=6378273 +b=6356889.449 +units=m +no_defs",
            "Arctic (sea ice)",
            "Hughes 1980",
        ),
        (
            3413,
            "WGS 84 / NSIDC Sea Ice Polar Stereographic North",
            "+proj=stere +lat_0=90 +lat_ts=70 +lon_0=-45 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Arctic",
            "WGS84",
        ),
        (
            3995,
            "WGS 84 / Arctic Polar Stereographic",
            "+proj=stere +lat_0=90 +lat_ts=71 +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Arctic",
            "WGS84",
        ),
        (
            3996,
            "WGS 84 / IBCAO Polar Stereographic",
            "+proj=stere +lat_0=90 +lat_ts=75 +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Arctic (deep sea)",
            "WGS84",
        ),
        (
            5936,
            "WGS 84 / EPSG Arctic Regional zone A1",
            "+proj=lcc +lat_1=73.6666667 +lat_2=70.3333333 +lat_0=72.024 +lon_0=14 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Arctic zone A1",
            "WGS84",
        ),
        (
            5937,
            "WGS 84 / EPSG Arctic Regional zone A2",
            "+proj=lcc +lat_1=73.6666667 +lat_2=70.3333333 +lat_0=72.024 +lon_0=34 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Arctic zone A2",
            "WGS84",
        ),
        (
            5938,
            "WGS 84 / EPSG Arctic Regional zone A3",
            "+proj=lcc +lat_1=73.6666667 +lat_2=70.3333333 +lat_0=72.024 +lon_0=54 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Arctic zone A3",
            "WGS84",
        ),
        (
            5939,
            "WGS 84 / EPSG Arctic Regional zone A4",
            "+proj=lcc +lat_1=73.6666667 +lat_2=70.3333333 +lat_0=72.024 +lon_0=74 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Arctic zone A4",
            "WGS84",
        ),
        (
            5940,
            "WGS 84 / EPSG Arctic Regional zone A5",
            "+proj=lcc +lat_1=73.6666667 +lat_2=70.3333333 +lat_0=72.024 +lon_0=94 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Arctic zone A5",
            "WGS84",
        ),
        // Antarctic
        (
            3031,
            "WGS 84 / Antarctic Polar Stereographic",
            "+proj=stere +lat_0=-90 +lat_ts=-71 +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Antarctica",
            "WGS84",
        ),
        (
            3032,
            "WGS 84 / Australian Antarctic Polar Stereographic",
            "+proj=stere +lat_0=-90 +lat_ts=-71 +lon_0=70 +k=1 +x_0=6000000 +y_0=6000000 +datum=WGS84 +units=m +no_defs",
            "Australian Antarctic Territory",
            "WGS84",
        ),
        (
            3033,
            "WGS 84 / Australian Antarctic Lambert",
            "+proj=lcc +lat_1=-68.5 +lat_2=-74.5 +lat_0=-50 +lon_0=70 +x_0=6000000 +y_0=6000000 +datum=WGS84 +units=m +no_defs",
            "Australian Antarctic Territory",
            "WGS84",
        ),
        (
            3412,
            "NSIDC Sea Ice Polar Stereographic South",
            "+proj=stere +lat_0=-90 +lat_ts=-70 +lon_0=0 +k=1 +x_0=0 +y_0=0 +a=6378273 +b=6356889.449 +units=m +no_defs",
            "Antarctica (sea ice)",
            "Hughes 1980",
        ),
        (
            3976,
            "WGS 84 / NSIDC Sea Ice Polar Stereographic South",
            "+proj=stere +lat_0=-90 +lat_ts=-70 +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs",
            "Antarctica",
            "WGS84",
        ),
        // UPS (Universal Polar Stereographic)
        (
            32661,
            "WGS 84 / UPS North (E,N)",
            "+proj=stere +lat_0=90 +lon_0=0 +k=0.994 +x_0=2000000 +y_0=2000000 +datum=WGS84 +units=m +no_defs",
            "North Pole (north of 84°N)",
            "WGS84",
        ),
        (
            32761,
            "WGS 84 / UPS South (E,N)",
            "+proj=stere +lat_0=-90 +lon_0=0 +k=0.994 +x_0=2000000 +y_0=2000000 +datum=WGS84 +units=m +no_defs",
            "South Pole (south of 80°S)",
            "WGS84",
        ),
    ];

    for &(code, name, proj_string, area, datum) in polar_grids {
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
