//! NAD83 State Plane Coordinate Systems (SPCS) registrations.
//!
//! Includes the legacy NAD83 zones (EPSG 2222-2289, 2195-2204, 32164-32166)
//! plus modern NAD83(2011) zones (EPSG 6355-6419).

use super::super::types::{CrsType, EpsgDatabase, EpsgDefinition, epsg_unit_for};
use alloc::string::ToString;

pub(super) fn register_nad83_state_planes(db: &mut EpsgDatabase) {
    // NAD83 SPCS — major US state plane zones (Lambert / Transverse Mercator)
    let spcs: &[(u32, &str, &str)] = &[
        (
            2222,
            "NAD83 / Arizona East (ft)",
            "+proj=tmerc +lat_0=31 +lon_0=-110.166666666667 +k=0.9999 +x_0=213360 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2223,
            "NAD83 / Arizona Central (ft)",
            "+proj=tmerc +lat_0=31 +lon_0=-111.916666666667 +k=0.9999 +x_0=213360 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2224,
            "NAD83 / Arizona West (ft)",
            "+proj=tmerc +lat_0=31 +lon_0=-113.75 +k=0.999933333 +x_0=213360 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2225,
            "NAD83 / California zone 1 (ftUS)",
            "+proj=lcc +lat_0=39.3333333333333 +lat_1=41.6666666666667 +lat_2=40 +lon_0=-122 +x_0=2000000.0001016 +y_0=500000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2226,
            "NAD83 / California zone 2 (ftUS)",
            "+proj=lcc +lat_0=37.6666666666667 +lat_1=39.8333333333333 +lat_2=38.3333333333333 +lon_0=-122 +x_0=2000000.0001016 +y_0=500000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2227,
            "NAD83 / California zone 3 (ftUS)",
            "+proj=lcc +lat_0=36.5 +lat_1=38.4333333333333 +lat_2=37.0666666666667 +lon_0=-120.5 +x_0=2000000.0001016 +y_0=500000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2228,
            "NAD83 / California zone 4 (ftUS)",
            "+proj=lcc +lat_0=35.3333333333333 +lat_1=37.25 +lat_2=36 +lon_0=-119 +x_0=2000000.0001016 +y_0=500000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2229,
            "NAD83 / California zone 5 (ftUS)",
            "+proj=lcc +lat_0=33.5 +lat_1=35.4666666666667 +lat_2=34.0333333333333 +lon_0=-118 +x_0=2000000.0001016 +y_0=500000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2230,
            "NAD83 / California zone 6 (ftUS)",
            "+proj=lcc +lat_0=32.1666666666667 +lat_1=33.8833333333333 +lat_2=32.7833333333333 +lon_0=-116.25 +x_0=2000000.0001016 +y_0=500000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2231,
            "NAD83 / Colorado North (ftUS)",
            "+proj=lcc +lat_0=39.3333333333333 +lat_1=40.7833333333333 +lat_2=39.7166666666667 +lon_0=-105.5 +x_0=914401.828803658 +y_0=304800.609601219 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2232,
            "NAD83 / Colorado Central (ftUS)",
            "+proj=lcc +lat_0=37.8333333333333 +lat_1=39.75 +lat_2=38.45 +lon_0=-105.5 +x_0=914401.828803658 +y_0=304800.609601219 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2233,
            "NAD83 / Colorado South (ftUS)",
            "+proj=lcc +lat_0=36.6666666666667 +lat_1=38.4333333333333 +lat_2=37.2333333333333 +lon_0=-105.5 +x_0=914401.828803658 +y_0=304800.609601219 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2234,
            "NAD83 / Connecticut (ftUS)",
            "+proj=lcc +lat_0=40.8333333333333 +lat_1=41.8666666666667 +lat_2=41.2 +lon_0=-72.75 +x_0=304800.609601219 +y_0=152400.30480061 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2235,
            "NAD83 / Delaware (ftUS)",
            "+proj=tmerc +lat_0=38 +lon_0=-75.4166666666667 +k=0.999995 +x_0=200000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2236,
            "NAD83 / Florida East (ftUS)",
            "+proj=tmerc +lat_0=24.3333333333333 +lon_0=-81 +k=0.999941177 +x_0=200000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2237,
            "NAD83 / Florida West (ftUS)",
            "+proj=tmerc +lat_0=24.3333333333333 +lon_0=-82 +k=0.999941177 +x_0=200000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2238,
            "NAD83 / Florida North (ftUS)",
            "+proj=lcc +lat_0=29 +lat_1=30.75 +lat_2=29.5833333333333 +lon_0=-84.5 +x_0=600000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2239,
            "NAD83 / Georgia East (ftUS)",
            "+proj=tmerc +lat_0=30 +lon_0=-82.1666666666667 +k=0.9999 +x_0=200000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2240,
            "NAD83 / Georgia West (ftUS)",
            "+proj=tmerc +lat_0=30 +lon_0=-84.1666666666667 +k=0.9999 +x_0=699999.9998984 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2241,
            "NAD83 / Idaho East (ftUS)",
            "+proj=tmerc +lat_0=41.6666666666667 +lon_0=-112.166666666667 +k=0.999947368 +x_0=200000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2242,
            "NAD83 / Idaho Central (ftUS)",
            "+proj=tmerc +lat_0=41.6666666666667 +lon_0=-114 +k=0.999947368 +x_0=500000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2243,
            "NAD83 / Idaho West (ftUS)",
            "+proj=tmerc +lat_0=41.6666666666667 +lon_0=-115.75 +k=0.999933333 +x_0=800000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2244,
            "NAD83 / Indiana East (ftUS)",
            "+proj=tmerc +lat_0=37.5 +lon_0=-85.6666666666667 +k=0.999966667 +x_0=99999.9998983998 +y_0=249364.998729997 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2245,
            "NAD83 / Indiana West (ftUS)",
            "+proj=tmerc +lat_0=37.5 +lon_0=-87.0833333333333 +k=0.999966667 +x_0=900000 +y_0=249364.998729997 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2246,
            "NAD83 / Kentucky North (ftUS)",
            "+proj=lcc +lat_0=37.5 +lat_1=37.9666666666667 +lat_2=38.9666666666667 +lon_0=-84.25 +x_0=500000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2247,
            "NAD83 / Kentucky South (ftUS)",
            "+proj=lcc +lat_0=36.3333333333333 +lat_1=37.9333333333333 +lat_2=36.7333333333333 +lon_0=-85.75 +x_0=500000.0001016 +y_0=500000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2248,
            "NAD83 / Maryland (ftUS)",
            "+proj=lcc +lat_0=37.6666666666667 +lat_1=39.45 +lat_2=38.3 +lon_0=-77 +x_0=399999.9998984 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2249,
            "NAD83 / Massachusetts Mainland (ftUS)",
            "+proj=lcc +lat_0=41 +lat_1=42.6833333333333 +lat_2=41.7166666666667 +lon_0=-71.5 +x_0=200000.0001016 +y_0=750000 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2250,
            "NAD83 / Massachusetts Island (ftUS)",
            "+proj=lcc +lat_0=41 +lat_1=41.4833333333333 +lat_2=41.2833333333333 +lon_0=-70.5 +x_0=500000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2251,
            "NAD83 / Michigan North (ft)",
            "+proj=lcc +lat_0=44.7833333333333 +lat_1=47.0833333333333 +lat_2=45.4833333333333 +lon_0=-87 +x_0=7999999.999968 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2252,
            "NAD83 / Michigan Central (ft)",
            "+proj=lcc +lat_0=43.3166666666667 +lat_1=45.7 +lat_2=44.1833333333333 +lon_0=-84.3666666666667 +x_0=5999999.999976 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2253,
            "NAD83 / Michigan South (ft)",
            "+proj=lcc +lat_0=41.5 +lat_1=43.6666666666667 +lat_2=42.1 +lon_0=-84.3666666666667 +x_0=3999999.999984 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2254,
            "NAD83 / Mississippi East (ftUS)",
            "+proj=tmerc +lat_0=29.5 +lon_0=-88.8333333333333 +k=0.99995 +x_0=300000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2255,
            "NAD83 / Mississippi West (ftUS)",
            "+proj=tmerc +lat_0=29.5 +lon_0=-90.3333333333333 +k=0.99995 +x_0=699999.9998984 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2256,
            "NAD83 / Montana (ft)",
            "+proj=lcc +lat_0=44.25 +lat_1=49 +lat_2=45 +lon_0=-109.5 +x_0=599999.9999976 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2257,
            "NAD83 / New Mexico East (ftUS)",
            "+proj=tmerc +lat_0=31 +lon_0=-104.333333333333 +k=0.999909091 +x_0=165000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2258,
            "NAD83 / New Mexico Central (ftUS)",
            "+proj=tmerc +lat_0=31 +lon_0=-106.25 +k=0.9999 +x_0=500000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2259,
            "NAD83 / New Mexico West (ftUS)",
            "+proj=tmerc +lat_0=31 +lon_0=-107.833333333333 +k=0.999916667 +x_0=830000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2260,
            "NAD83 / New York East (ftUS)",
            "+proj=tmerc +lat_0=38.8333333333333 +lon_0=-74.5 +k=0.9999 +x_0=150000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2261,
            "NAD83 / New York Central (ftUS)",
            "+proj=tmerc +lat_0=40 +lon_0=-76.5833333333333 +k=0.9999375 +x_0=249999.9998984 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2262,
            "NAD83 / New York West (ftUS)",
            "+proj=tmerc +lat_0=40 +lon_0=-78.5833333333333 +k=0.9999375 +x_0=350000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2263,
            "NAD83 / New York Long Island (ftUS)",
            "+proj=lcc +lat_0=40.1666666666667 +lat_1=41.0333333333333 +lat_2=40.6666666666667 +lon_0=-74 +x_0=300000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2264,
            "NAD83 / North Carolina (ftUS)",
            "+proj=lcc +lat_0=33.75 +lat_1=36.1666666666667 +lat_2=34.3333333333333 +lon_0=-79 +x_0=609601.219202438 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2265,
            "NAD83 / North Dakota North (ft)",
            "+proj=lcc +lat_0=47 +lat_1=48.7333333333333 +lat_2=47.4333333333333 +lon_0=-100.5 +x_0=599999.9999976 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2266,
            "NAD83 / North Dakota South (ft)",
            "+proj=lcc +lat_0=45.6666666666667 +lat_1=47.4833333333333 +lat_2=46.1833333333333 +lon_0=-100.5 +x_0=599999.9999976 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2267,
            "NAD83 / Oklahoma North (ftUS)",
            "+proj=lcc +lat_0=35 +lat_1=36.7666666666667 +lat_2=35.5666666666667 +lon_0=-98 +x_0=600000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2268,
            "NAD83 / Oklahoma South (ftUS)",
            "+proj=lcc +lat_0=33.3333333333333 +lat_1=35.2333333333333 +lat_2=33.9333333333333 +lon_0=-98 +x_0=600000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2269,
            "NAD83 / Oregon North (ft)",
            "+proj=lcc +lat_0=43.6666666666667 +lat_1=46 +lat_2=44.3333333333333 +lon_0=-120.5 +x_0=2500000.0001424 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2270,
            "NAD83 / Oregon South (ft)",
            "+proj=lcc +lat_0=41.6666666666667 +lat_1=44 +lat_2=42.3333333333333 +lon_0=-120.5 +x_0=1500000.0001464 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2271,
            "NAD83 / Pennsylvania North (ftUS)",
            "+proj=lcc +lat_0=40.1666666666667 +lat_1=41.95 +lat_2=40.8833333333333 +lon_0=-77.75 +x_0=600000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2272,
            "NAD83 / Pennsylvania South (ftUS)",
            "+proj=lcc +lat_0=39.3333333333333 +lat_1=40.9666666666667 +lat_2=39.9333333333333 +lon_0=-77.75 +x_0=600000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2273,
            "NAD83 / South Carolina (ft)",
            "+proj=lcc +lat_0=31.8333333333333 +lat_1=34.8333333333333 +lat_2=32.5 +lon_0=-81 +x_0=609600 +y_0=0 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2274,
            "NAD83 / Tennessee (ftUS)",
            "+proj=lcc +lat_0=34.3333333333333 +lat_1=36.4166666666667 +lat_2=35.25 +lon_0=-86 +x_0=600000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2275,
            "NAD83 / Texas North (ftUS)",
            "+proj=lcc +lat_0=34 +lat_1=36.1833333333333 +lat_2=34.65 +lon_0=-101.5 +x_0=200000.0001016 +y_0=999999.9998984 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2276,
            "NAD83 / Texas North Central (ftUS)",
            "+proj=lcc +lat_0=31.6666666666667 +lat_1=33.9666666666667 +lat_2=32.1333333333333 +lon_0=-98.5 +x_0=600000 +y_0=2000000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2277,
            "NAD83 / Texas Central (ftUS)",
            "+proj=lcc +lat_0=29.6666666666667 +lat_1=31.8833333333333 +lat_2=30.1166666666667 +lon_0=-100.333333333333 +x_0=699999.9998984 +y_0=3000000 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2278,
            "NAD83 / Texas South Central (ftUS)",
            "+proj=lcc +lat_0=27.8333333333333 +lat_1=30.2833333333333 +lat_2=28.3833333333333 +lon_0=-99 +x_0=600000 +y_0=3999999.9998984 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2279,
            "NAD83 / Texas South (ftUS)",
            "+proj=lcc +lat_0=25.6666666666667 +lat_1=27.8333333333333 +lat_2=26.1666666666667 +lon_0=-98.5 +x_0=300000 +y_0=5000000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2280,
            "NAD83 / Utah North (ft)",
            "+proj=lcc +lat_0=40.3333333333333 +lat_1=41.7833333333333 +lat_2=40.7166666666667 +lon_0=-111.5 +x_0=500000.0001504 +y_0=999999.999996 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2281,
            "NAD83 / Utah Central (ft)",
            "+proj=lcc +lat_0=38.3333333333333 +lat_1=40.65 +lat_2=39.0166666666667 +lon_0=-111.5 +x_0=500000.0001504 +y_0=1999999.999992 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2282,
            "NAD83 / Utah South (ft)",
            "+proj=lcc +lat_0=36.6666666666667 +lat_1=38.35 +lat_2=37.2166666666667 +lon_0=-111.5 +x_0=500000.0001504 +y_0=2999999.999988 +datum=NAD83 +units=ft +no_defs",
        ),
        (
            2283,
            "NAD83 / Virginia North (ftUS)",
            "+proj=lcc +lat_0=37.6666666666667 +lat_1=39.2 +lat_2=38.0333333333333 +lon_0=-78.5 +x_0=3500000.0001016 +y_0=2000000.0001016 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2284,
            "NAD83 / Virginia South (ftUS)",
            "+proj=lcc +lat_0=36.3333333333333 +lat_1=37.9666666666667 +lat_2=36.7666666666667 +lon_0=-78.5 +x_0=3500000.0001016 +y_0=999999.9998984 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2285,
            "NAD83 / Washington North (ftUS)",
            "+proj=lcc +lat_0=47 +lat_1=48.7333333333333 +lat_2=47.5 +lon_0=-120.833333333333 +x_0=500000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2286,
            "NAD83 / Washington South (ftUS)",
            "+proj=lcc +lat_0=45.3333333333333 +lat_1=47.3333333333333 +lat_2=45.8333333333333 +lon_0=-120.5 +x_0=500000.0001016 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2287,
            "NAD83 / Wisconsin North (ftUS)",
            "+proj=lcc +lat_0=45.1666666666667 +lat_1=46.7666666666667 +lat_2=45.5666666666667 +lon_0=-90 +x_0=600000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2288,
            "NAD83 / Wisconsin Central (ftUS)",
            "+proj=lcc +lat_0=43.8333333333333 +lat_1=45.5 +lat_2=44.25 +lon_0=-90 +x_0=600000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2289,
            "NAD83 / Wisconsin South (ftUS)",
            "+proj=lcc +lat_0=42 +lat_1=44.0666666666667 +lat_2=42.7333333333333 +lon_0=-90 +x_0=600000 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            2195,
            "NAD83(HARN) / UTM zone 2S",
            "+proj=utm +zone=2 +south +ellps=GRS80 +units=m +no_defs",
        ),
        (
            2196,
            "ETRS89 / Kp2000 Jutland",
            "+proj=tmerc +lat_0=0 +lon_0=9.5 +k=0.99995 +x_0=200000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            2197,
            "ETRS89 / Kp2000 Zealand",
            "+proj=tmerc +lat_0=0 +lon_0=12 +k=0.99995 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            2198,
            "ETRS89 / Kp2000 Bornholm",
            "+proj=tmerc +lat_0=0 +lon_0=15 +k=1 +x_0=900000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            2199,
            "Albanian 1987 / Gauss Kruger zone 4",
            "+proj=tmerc +lat_0=0 +lon_0=21 +k=1 +x_0=4500000 +y_0=0 +ellps=krass +units=m +no_defs",
        ),
        (
            2200,
            "ATS77 / New Brunswick Stereographic (ATS77)",
            "+proj=sterea +lat_0=46.5 +lon_0=-66.5 +k=0.999912 +x_0=300000 +y_0=800000 +a=6378135 +rf=298.257 +units=m +no_defs",
        ),
        (
            2201,
            "REGVEN / UTM zone 18N",
            "+proj=utm +zone=18 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            2202,
            "REGVEN / UTM zone 19N",
            "+proj=utm +zone=19 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            2203,
            "REGVEN / UTM zone 20N",
            "+proj=utm +zone=20 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            2204,
            "NAD27 / Tennessee",
            "+proj=lcc +lat_0=34.6666666666667 +lat_1=35.25 +lat_2=36.4166666666667 +lon_0=-86 +x_0=609601.219202438 +y_0=30480.0609601219 +datum=NAD27 +units=us-ft +no_defs",
        ),
        (
            32164,
            "NAD83 / BLM 14N (ftUS)",
            "+proj=tmerc +lat_0=0 +lon_0=-99 +k=0.9996 +x_0=500000.001016002 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            32165,
            "NAD83 / BLM 15N (ftUS)",
            "+proj=tmerc +lat_0=0 +lon_0=-93 +k=0.9996 +x_0=500000.001016002 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
        ),
        (
            32166,
            "NAD83 / BLM 16N (ftUS)",
            "+proj=tmerc +lat_0=0 +lon_0=-87 +k=0.9996 +x_0=500000.001016002 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
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
            unit: epsg_unit_for(proj_string).to_string(),
            datum: "NAD83".to_string(),
        });
    }

    // NAD83(2011) State Plane zones — modern realizations (EPSG 6355-6543)
    let nad83_2011_spcs: &[(u32, &str, &str)] = &[
        (
            6355,
            "NAD83(2011) / Alabama East",
            "+proj=tmerc +lat_0=30.5 +lon_0=-85.8333333333333 +k=0.99996 +x_0=200000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6356,
            "NAD83(2011) / Alabama West",
            "+proj=tmerc +lat_0=30 +lon_0=-87.5 +k=0.999933333 +x_0=600000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6393,
            "NAD83(2011) / Alaska Albers",
            "+proj=aea +lat_0=50 +lat_1=55 +lat_2=65 +lon_0=-154 +x_0=0 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6394,
            "NAD83(2011) / Alaska zone 1",
            "+proj=omerc +lat_0=57 +lonc=-133.666666666667 +alpha=323.130102361111 +gamma=323.130102361111 +k=0.9999 +x_0=5000000 +y_0=-5000000 +ellps=GRS80 +units=m +no_defs +no_uoff",
        ),
        (
            6395,
            "NAD83(2011) / Alaska zone 2",
            "+proj=tmerc +lat_0=54 +lon_0=-142 +k=0.9999 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6396,
            "NAD83(2011) / Alaska zone 3",
            "+proj=tmerc +lat_0=54 +lon_0=-146 +k=0.9999 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6397,
            "NAD83(2011) / Alaska zone 4",
            "+proj=tmerc +lat_0=54 +lon_0=-150 +k=0.9999 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6398,
            "NAD83(2011) / Alaska zone 5",
            "+proj=tmerc +lat_0=54 +lon_0=-154 +k=0.9999 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6399,
            "NAD83(2011) / Alaska zone 6",
            "+proj=tmerc +lat_0=54 +lon_0=-158 +k=0.9999 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6400,
            "NAD83(2011) / Alaska zone 7",
            "+proj=tmerc +lat_0=54 +lon_0=-162 +k=0.9999 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6401,
            "NAD83(2011) / Alaska zone 8",
            "+proj=tmerc +lat_0=54 +lon_0=-166 +k=0.9999 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6402,
            "NAD83(2011) / Alaska zone 9",
            "+proj=tmerc +lat_0=54 +lon_0=-170 +k=0.9999 +x_0=500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6403,
            "NAD83(2011) / Alaska zone 10",
            "+proj=lcc +lat_0=51 +lat_1=53.8333333333333 +lat_2=51.8333333333333 +lon_0=-176 +x_0=1000000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6404,
            "NAD83(2011) / Arizona Central",
            "+proj=tmerc +lat_0=31 +lon_0=-111.916666666667 +k=0.9999 +x_0=213360 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6405,
            "NAD83(2011) / Arizona Central (ft)",
            "+proj=tmerc +lat_0=31 +lon_0=-111.916666666667 +k=0.9999 +x_0=213360 +y_0=0 +ellps=GRS80 +units=ft +no_defs",
        ),
        (
            6406,
            "NAD83(2011) / Arizona East",
            "+proj=tmerc +lat_0=31 +lon_0=-110.166666666667 +k=0.9999 +x_0=213360 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6414,
            "NAD83(2011) / California Albers",
            "+proj=aea +lat_0=0 +lat_1=34 +lat_2=40.5 +lon_0=-120 +x_0=0 +y_0=-4000000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6415,
            "NAD83(2011) / California zone 1",
            "+proj=lcc +lat_0=39.3333333333333 +lat_1=41.6666666666667 +lat_2=40 +lon_0=-122 +x_0=2000000 +y_0=500000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6416,
            "NAD83(2011) / California zone 1 (ftUS)",
            "+proj=lcc +lat_0=39.3333333333333 +lat_1=41.6666666666667 +lat_2=40 +lon_0=-122 +x_0=2000000.0001016 +y_0=500000.0001016 +ellps=GRS80 +units=us-ft +no_defs",
        ),
        (
            6417,
            "NAD83(2011) / California zone 2",
            "+proj=lcc +lat_0=37.6666666666667 +lat_1=39.8333333333333 +lat_2=38.3333333333333 +lon_0=-122 +x_0=2000000 +y_0=500000 +ellps=GRS80 +units=m +no_defs",
        ),
        (
            6418,
            "NAD83(2011) / California zone 2 (ftUS)",
            "+proj=lcc +lat_0=37.6666666666667 +lat_1=39.8333333333333 +lat_2=38.3333333333333 +lon_0=-122 +x_0=2000000.0001016 +y_0=500000.0001016 +ellps=GRS80 +units=us-ft +no_defs",
        ),
        (
            6419,
            "NAD83(2011) / California zone 3",
            "+proj=lcc +lat_0=36.5 +lat_1=38.4333333333333 +lat_2=37.0666666666667 +lon_0=-120.5 +x_0=2000000 +y_0=500000 +ellps=GRS80 +units=m +no_defs",
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
            unit: epsg_unit_for(proj_string).to_string(),
            datum: "NAD83(2011)".to_string(),
        });
    }
}
