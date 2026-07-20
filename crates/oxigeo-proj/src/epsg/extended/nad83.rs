//! NAD83 State Plane Coordinate Systems (SPCS) registrations.
//!
//! Includes the legacy NAD83 zones (EPSG 2222-2289, 2195-2204, 32164-32166)
//! plus modern NAD83(2011) zones (EPSG 6355-6419).

use super::super::types::{CrsType, EpsgDatabase, EpsgDefinition};

pub(super) fn register_nad83_state_planes(db: &mut EpsgDatabase) {
    // NAD83 SPCS — major US state plane zones (Lambert / Transverse Mercator)
    let spcs: &[(u32, &str, &str)] = &[
        (
            2222,
            "NAD83 / Arizona East",
            "+proj=tmerc +lat_0=31 +lon_0=-110.1666667 +k=0.9999 +x_0=213360 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2223,
            "NAD83 / Arizona Central",
            "+proj=tmerc +lat_0=31 +lon_0=-111.9166667 +k=0.9999 +x_0=213360 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2224,
            "NAD83 / Arizona West",
            "+proj=tmerc +lat_0=31 +lon_0=-113.75 +k=0.999933333 +x_0=213360 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2225,
            "NAD83 / California zone 1",
            "+proj=lcc +lat_1=41.6666667 +lat_2=40 +lat_0=39.3333333 +lon_0=-122 +x_0=2000000 +y_0=500000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2226,
            "NAD83 / California zone 2",
            "+proj=lcc +lat_1=39.8333333 +lat_2=38.3333333 +lat_0=37.6666667 +lon_0=-122 +x_0=2000000 +y_0=500000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2227,
            "NAD83 / California zone 3",
            "+proj=lcc +lat_1=38.4333333 +lat_2=37.0666667 +lat_0=36.5 +lon_0=-120.5 +x_0=2000000 +y_0=500000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2228,
            "NAD83 / California zone 4",
            "+proj=lcc +lat_1=37.25 +lat_2=36 +lat_0=35.3333333 +lon_0=-119 +x_0=2000000 +y_0=500000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2229,
            "NAD83 / California zone 5",
            "+proj=lcc +lat_1=35.4666667 +lat_2=34.0333333 +lat_0=33.5 +lon_0=-118 +x_0=2000000 +y_0=500000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2230,
            "NAD83 / California zone 6",
            "+proj=lcc +lat_1=33.8833333 +lat_2=32.7833333 +lat_0=32.1666667 +lon_0=-116.25 +x_0=2000000 +y_0=500000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2231,
            "NAD83 / Colorado North",
            "+proj=lcc +lat_1=40.7833333 +lat_2=39.7166667 +lat_0=39.3333333 +lon_0=-105.5 +x_0=914401.8289 +y_0=304800.6096 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2232,
            "NAD83 / Colorado Central",
            "+proj=lcc +lat_1=39.75 +lat_2=38.45 +lat_0=37.8333333 +lon_0=-105.5 +x_0=914401.8289 +y_0=304800.6096 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2233,
            "NAD83 / Colorado South",
            "+proj=lcc +lat_1=38.4333333 +lat_2=37.2333333 +lat_0=36.6666667 +lon_0=-105.5 +x_0=914401.8289 +y_0=304800.6096 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2234,
            "NAD83 / Connecticut",
            "+proj=lcc +lat_1=41.8666667 +lat_2=41.2 +lat_0=40.8333333 +lon_0=-72.75 +x_0=304800.6096 +y_0=152400.3048 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2235,
            "NAD83 / Delaware",
            "+proj=tmerc +lat_0=38 +lon_0=-75.4166667 +k=0.999995 +x_0=200000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2236,
            "NAD83 / Florida East",
            "+proj=tmerc +lat_0=24.3333333 +lon_0=-81 +k=0.999941177 +x_0=200000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2237,
            "NAD83 / Florida West",
            "+proj=tmerc +lat_0=24.3333333 +lon_0=-82 +k=0.999941177 +x_0=200000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2238,
            "NAD83 / Florida North",
            "+proj=lcc +lat_1=30.75 +lat_2=29.5833333 +lat_0=29 +lon_0=-84.5 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2239,
            "NAD83 / Georgia East",
            "+proj=tmerc +lat_0=30 +lon_0=-82.1666667 +k=0.9999 +x_0=200000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2240,
            "NAD83 / Georgia West",
            "+proj=tmerc +lat_0=30 +lon_0=-84.1666667 +k=0.9999 +x_0=700000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2241,
            "NAD83 / Idaho East",
            "+proj=tmerc +lat_0=41.6666667 +lon_0=-112.1666667 +k=0.999947368 +x_0=200000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2242,
            "NAD83 / Idaho Central",
            "+proj=tmerc +lat_0=41.6666667 +lon_0=-114 +k=0.999947368 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2243,
            "NAD83 / Idaho West",
            "+proj=tmerc +lat_0=41.6666667 +lon_0=-115.75 +k=0.999933333 +x_0=800000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2244,
            "NAD83 / Indiana East",
            "+proj=tmerc +lat_0=37.5 +lon_0=-85.6666667 +k=0.999966667 +x_0=100000 +y_0=250000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2245,
            "NAD83 / Indiana West",
            "+proj=tmerc +lat_0=37.5 +lon_0=-87.0833333 +k=0.999966667 +x_0=900000 +y_0=250000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2246,
            "NAD83 / Kentucky North",
            "+proj=lcc +lat_1=38.9666667 +lat_2=37.9666667 +lat_0=37.5 +lon_0=-84.25 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2247,
            "NAD83 / Kentucky South",
            "+proj=lcc +lat_1=37.9333333 +lat_2=36.7333333 +lat_0=36.3333333 +lon_0=-85.75 +x_0=500000 +y_0=500000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2248,
            "NAD83 / Maryland",
            "+proj=lcc +lat_1=39.45 +lat_2=38.3 +lat_0=37.6666667 +lon_0=-77 +x_0=400000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2249,
            "NAD83 / Massachusetts Mainland",
            "+proj=lcc +lat_1=42.6833333 +lat_2=41.7166667 +lat_0=41 +lon_0=-71.5 +x_0=200000 +y_0=750000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2250,
            "NAD83 / Massachusetts Island",
            "+proj=lcc +lat_1=41.4833333 +lat_2=41.2833333 +lat_0=41 +lon_0=-70.5 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2251,
            "NAD83 / Michigan North",
            "+proj=lcc +lat_1=47.0833333 +lat_2=45.4833333 +lat_0=44.7833333 +lon_0=-87 +x_0=8000000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2252,
            "NAD83 / Michigan Central",
            "+proj=lcc +lat_1=45.7 +lat_2=44.1833333 +lat_0=43.3166667 +lon_0=-84.3666667 +x_0=6000000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2253,
            "NAD83 / Michigan South",
            "+proj=lcc +lat_1=43.6666667 +lat_2=42.1 +lat_0=41.5 +lon_0=-84.3666667 +x_0=4000000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2254,
            "NAD83 / Mississippi East",
            "+proj=tmerc +lat_0=29.5 +lon_0=-88.8333333 +k=0.99995 +x_0=300000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2255,
            "NAD83 / Mississippi West",
            "+proj=tmerc +lat_0=29.5 +lon_0=-90.3333333 +k=0.99995 +x_0=700000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2256,
            "NAD83 / Montana",
            "+proj=lcc +lat_1=49 +lat_2=45 +lat_0=44.25 +lon_0=-109.5 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2257,
            "NAD83 / Nebraska",
            "+proj=lcc +lat_1=43 +lat_2=40 +lat_0=39.8333333 +lon_0=-100 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2258,
            "NAD83 / Nevada East",
            "+proj=tmerc +lat_0=34.75 +lon_0=-115.5833333 +k=0.9999 +x_0=200000 +y_0=8000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2259,
            "NAD83 / Nevada Central",
            "+proj=tmerc +lat_0=34.75 +lon_0=-116.6666667 +k=0.9999 +x_0=500000 +y_0=6000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2260,
            "NAD83 / Nevada West",
            "+proj=tmerc +lat_0=34.75 +lon_0=-118.5833333 +k=0.9999 +x_0=800000 +y_0=4000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2261,
            "NAD83 / New Hampshire",
            "+proj=tmerc +lat_0=42.5 +lon_0=-71.6666667 +k=0.999966667 +x_0=300000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2262,
            "NAD83 / New Jersey",
            "+proj=tmerc +lat_0=38.8333333 +lon_0=-74.5 +k=0.9999 +x_0=150000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2263,
            "NAD83 / New York Long Island",
            "+proj=lcc +lat_1=41.0333333 +lat_2=40.6666667 +lat_0=40.1666667 +lon_0=-74 +x_0=300000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2264,
            "NAD83 / North Carolina",
            "+proj=lcc +lat_1=36.1666667 +lat_2=34.3333333 +lat_0=33.75 +lon_0=-79 +x_0=609601.22 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2265,
            "NAD83 / North Dakota North",
            "+proj=lcc +lat_1=48.7333333 +lat_2=47.4333333 +lat_0=47 +lon_0=-100.5 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2266,
            "NAD83 / North Dakota South",
            "+proj=lcc +lat_1=47.4833333 +lat_2=46.1833333 +lat_0=45.6666667 +lon_0=-100.5 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2267,
            "NAD83 / Oklahoma North",
            "+proj=lcc +lat_1=36.7666667 +lat_2=35.5666667 +lat_0=35 +lon_0=-98 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2268,
            "NAD83 / Oklahoma South",
            "+proj=lcc +lat_1=35.2333333 +lat_2=33.9333333 +lat_0=33.3333333 +lon_0=-98 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2269,
            "NAD83 / Oregon North",
            "+proj=lcc +lat_1=46 +lat_2=44.3333333 +lat_0=43.6666667 +lon_0=-120.5 +x_0=2500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2270,
            "NAD83 / Oregon South",
            "+proj=lcc +lat_1=44 +lat_2=42.3333333 +lat_0=41.6666667 +lon_0=-120.5 +x_0=1500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2271,
            "NAD83 / Pennsylvania North",
            "+proj=lcc +lat_1=41.95 +lat_2=40.8833333 +lat_0=40.1666667 +lon_0=-77.75 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2272,
            "NAD83 / Pennsylvania South",
            "+proj=lcc +lat_1=40.9666667 +lat_2=39.9333333 +lat_0=39.3333333 +lon_0=-77.75 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2273,
            "NAD83 / South Carolina",
            "+proj=lcc +lat_1=34.8333333 +lat_2=32.5 +lat_0=31.8333333 +lon_0=-81 +x_0=609600 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2274,
            "NAD83 / Tennessee",
            "+proj=lcc +lat_1=36.4166667 +lat_2=35.25 +lat_0=34.3333333 +lon_0=-86 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2275,
            "NAD83 / Texas North",
            "+proj=lcc +lat_1=36.1833333 +lat_2=34.65 +lat_0=34 +lon_0=-101.5 +x_0=200000 +y_0=1000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2276,
            "NAD83 / Texas North Central",
            "+proj=lcc +lat_1=33.9666667 +lat_2=32.1333333 +lat_0=31.6666667 +lon_0=-98.5 +x_0=600000 +y_0=2000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2277,
            "NAD83 / Texas Central",
            "+proj=lcc +lat_1=31.8833333 +lat_2=30.1166667 +lat_0=29.6666667 +lon_0=-100.3333333 +x_0=700000 +y_0=3000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2278,
            "NAD83 / Texas South Central",
            "+proj=lcc +lat_1=30.2833333 +lat_2=28.3833333 +lat_0=27.8333333 +lon_0=-99 +x_0=600000 +y_0=4000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2279,
            "NAD83 / Texas South",
            "+proj=lcc +lat_1=27.8333333 +lat_2=26.1666667 +lat_0=25.6666667 +lon_0=-98.5 +x_0=300000 +y_0=5000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2280,
            "NAD83 / Utah North",
            "+proj=lcc +lat_1=41.7833333 +lat_2=40.7166667 +lat_0=40.3333333 +lon_0=-111.5 +x_0=500000 +y_0=1000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2281,
            "NAD83 / Utah Central",
            "+proj=lcc +lat_1=40.65 +lat_2=39.0166667 +lat_0=38.3333333 +lon_0=-111.5 +x_0=500000 +y_0=2000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2282,
            "NAD83 / Utah South",
            "+proj=lcc +lat_1=38.35 +lat_2=37.2166667 +lat_0=36.6666667 +lon_0=-111.5 +x_0=500000 +y_0=3000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2283,
            "NAD83 / Virginia North",
            "+proj=lcc +lat_1=39.2 +lat_2=38.0333333 +lat_0=37.6666667 +lon_0=-78.5 +x_0=3500000 +y_0=2000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2284,
            "NAD83 / Virginia South",
            "+proj=lcc +lat_1=37.9666667 +lat_2=36.7666667 +lat_0=36.3333333 +lon_0=-78.5 +x_0=3500000 +y_0=1000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2285,
            "NAD83 / Washington North",
            "+proj=lcc +lat_1=48.7333333 +lat_2=47.5 +lat_0=47 +lon_0=-120.8333333 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2286,
            "NAD83 / Washington South",
            "+proj=lcc +lat_1=47.3333333 +lat_2=45.8333333 +lat_0=45.3333333 +lon_0=-120.5 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2287,
            "NAD83 / Wisconsin North",
            "+proj=lcc +lat_1=46.7666667 +lat_2=45.5666667 +lat_0=45.1666667 +lon_0=-90 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2288,
            "NAD83 / Wisconsin Central",
            "+proj=lcc +lat_1=45.5 +lat_2=44.25 +lat_0=43.8333333 +lon_0=-90 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2289,
            "NAD83 / Wisconsin South",
            "+proj=lcc +lat_1=44.0666667 +lat_2=42.7333333 +lat_0=42 +lon_0=-90 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2195,
            "NAD83 / Alaska zone 1",
            "+proj=omerc +lat_0=57 +lonc=-133.6666667 +alpha=323.1301023611111 +k=0.9999 +x_0=5000000 +y_0=-5000000 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2196,
            "NAD83 / Alaska zone 2",
            "+proj=tmerc +lat_0=54 +lon_0=-142 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2197,
            "NAD83 / Alaska zone 3",
            "+proj=tmerc +lat_0=54 +lon_0=-146 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2198,
            "NAD83 / Alaska zone 4",
            "+proj=tmerc +lat_0=54 +lon_0=-150 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2199,
            "NAD83 / Alaska zone 5",
            "+proj=tmerc +lat_0=54 +lon_0=-154 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2200,
            "NAD83 / Alaska zone 6",
            "+proj=tmerc +lat_0=54 +lon_0=-158 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2201,
            "NAD83 / Alaska zone 7",
            "+proj=tmerc +lat_0=54 +lon_0=-162 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2202,
            "NAD83 / Alaska zone 8",
            "+proj=tmerc +lat_0=54 +lon_0=-166 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2203,
            "NAD83 / Alaska zone 9",
            "+proj=tmerc +lat_0=54 +lon_0=-170 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            2204,
            "NAD83 / Alaska zone 10",
            "+proj=lcc +lat_1=53.8333333 +lat_2=51.8333333 +lat_0=51 +lon_0=-176 +x_0=1000000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32164,
            "NAD83 / Hawaii zone 1",
            "+proj=tmerc +lat_0=18.8333333 +lon_0=-155.5 +k=0.999966667 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32165,
            "NAD83 / Hawaii zone 2",
            "+proj=tmerc +lat_0=20.3333333 +lon_0=-156.6666667 +k=0.999966667 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
        (
            32166,
            "NAD83 / Hawaii zone 3",
            "+proj=tmerc +lat_0=21.1666667 +lon_0=-158 +k=0.99999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
        ),
    ];

    for &(code, name, proj_string) in spcs {
        db.add_definition(EpsgDefinition {
            code,
            name: name.to_string(),
            proj_string: proj_string.to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "United States".to_string(),
            unit: "metre".to_string(),
            datum: "NAD83".to_string(),
        });
    }

    // NAD83(2011) State Plane zones — modern realizations (EPSG 6355-6543)
    let nad83_2011_spcs: &[(u32, &str, &str)] = &[
        (
            6355,
            "NAD83(2011) / Alabama East",
            "+proj=tmerc +lat_0=30.5 +lon_0=-85.8333333 +k=0.99996 +x_0=200000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6356,
            "NAD83(2011) / Alabama West",
            "+proj=tmerc +lat_0=30 +lon_0=-87.5 +k=0.999933333 +x_0=600000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6393,
            "NAD83(2011) / Illinois East",
            "+proj=tmerc +lat_0=36.6666667 +lon_0=-88.3333333 +k=0.999975 +x_0=300000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6394,
            "NAD83(2011) / Illinois West",
            "+proj=tmerc +lat_0=36.6666667 +lon_0=-90.1666667 +k=0.999941177 +x_0=700000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6395,
            "NAD83(2011) / Indiana East",
            "+proj=tmerc +lat_0=37.5 +lon_0=-85.6666667 +k=0.999966667 +x_0=100000 +y_0=250000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6396,
            "NAD83(2011) / Indiana West",
            "+proj=tmerc +lat_0=37.5 +lon_0=-87.0833333 +k=0.999966667 +x_0=900000 +y_0=250000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6397,
            "NAD83(2011) / Iowa North",
            "+proj=lcc +lat_1=43.2666667 +lat_2=42.0666667 +lat_0=41.5 +lon_0=-93.5 +x_0=1500000 +y_0=1000000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6398,
            "NAD83(2011) / Iowa South",
            "+proj=lcc +lat_1=41.7833333 +lat_2=40.6166667 +lat_0=40 +lon_0=-93.5 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6399,
            "NAD83(2011) / Kansas North",
            "+proj=lcc +lat_1=39.7833333 +lat_2=38.7166667 +lat_0=38.3333333 +lon_0=-98 +x_0=400000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6400,
            "NAD83(2011) / Kansas South",
            "+proj=lcc +lat_1=38.5666667 +lat_2=37.2666667 +lat_0=36.6666667 +lon_0=-98.5 +x_0=400000 +y_0=400000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6401,
            "NAD83(2011) / Kentucky North",
            "+proj=lcc +lat_1=38.9666667 +lat_2=37.9666667 +lat_0=37.5 +lon_0=-84.25 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6402,
            "NAD83(2011) / Kentucky South",
            "+proj=lcc +lat_1=37.9333333 +lat_2=36.7333333 +lat_0=36.3333333 +lon_0=-85.75 +x_0=500000 +y_0=500000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6403,
            "NAD83(2011) / Louisiana North",
            "+proj=lcc +lat_1=32.6666667 +lat_2=31.1666667 +lat_0=30.5 +lon_0=-92.5 +x_0=1000000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6404,
            "NAD83(2011) / Louisiana South",
            "+proj=lcc +lat_1=30.7 +lat_2=29.3 +lat_0=28.5 +lon_0=-91.3333333 +x_0=1000000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6405,
            "NAD83(2011) / Maine East",
            "+proj=tmerc +lat_0=43.6666667 +lon_0=-68.5 +k=0.9999 +x_0=300000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6406,
            "NAD83(2011) / Maine West",
            "+proj=tmerc +lat_0=42.8333333 +lon_0=-70.1666667 +k=0.999966667 +x_0=900000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6414,
            "NAD83(2011) / Minnesota North",
            "+proj=lcc +lat_1=48.6333333 +lat_2=47.0333333 +lat_0=46.5 +lon_0=-93.1 +x_0=800000 +y_0=100000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6415,
            "NAD83(2011) / Minnesota Central",
            "+proj=lcc +lat_1=47.05 +lat_2=45.6166667 +lat_0=45 +lon_0=-94.25 +x_0=800000 +y_0=100000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6416,
            "NAD83(2011) / Minnesota South",
            "+proj=lcc +lat_1=45.2166667 +lat_2=43.7833333 +lat_0=43 +lon_0=-94 +x_0=800000 +y_0=100000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6417,
            "NAD83(2011) / Missouri East",
            "+proj=tmerc +lat_0=35.8333333 +lon_0=-90.5 +k=0.999933333 +x_0=250000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6418,
            "NAD83(2011) / Missouri Central",
            "+proj=tmerc +lat_0=35.8333333 +lon_0=-92.5 +k=0.999933333 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6419,
            "NAD83(2011) / Missouri West",
            "+proj=tmerc +lat_0=36.1666667 +lon_0=-94.5 +k=0.999941177 +x_0=850000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
    ];

    for &(code, name, proj_string) in nad83_2011_spcs {
        db.add_definition(EpsgDefinition {
            code,
            name: name.to_string(),
            proj_string: proj_string.to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "United States".to_string(),
            unit: "metre".to_string(),
            datum: "NAD83(2011)".to_string(),
        });
    }
}
