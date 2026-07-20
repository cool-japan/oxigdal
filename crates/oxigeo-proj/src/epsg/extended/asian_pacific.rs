//! Asian and Pacific national grid CRS registrations.

use super::super::types::{CrsType, EpsgDatabase, EpsgDefinition};

pub(super) fn register_asian_pacific_grids(db: &mut EpsgDatabase) {
    let asian_grids: &[(u32, &str, &str, &str, &str)] = &[
        // Chinese grids (CGCS2000 Gauss-Kruger zones)
        (
            4502,
            "CGCS2000 / Gauss-Kruger zone 13",
            "+proj=tmerc +lat_0=0 +lon_0=75 +k=1 +x_0=13500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 13",
            "CGCS2000",
        ),
        (
            4503,
            "CGCS2000 / Gauss-Kruger zone 14",
            "+proj=tmerc +lat_0=0 +lon_0=81 +k=1 +x_0=14500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 14",
            "CGCS2000",
        ),
        (
            4504,
            "CGCS2000 / Gauss-Kruger zone 15",
            "+proj=tmerc +lat_0=0 +lon_0=87 +k=1 +x_0=15500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 15",
            "CGCS2000",
        ),
        (
            4505,
            "CGCS2000 / Gauss-Kruger zone 16",
            "+proj=tmerc +lat_0=0 +lon_0=93 +k=1 +x_0=16500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 16",
            "CGCS2000",
        ),
        (
            4506,
            "CGCS2000 / Gauss-Kruger zone 17",
            "+proj=tmerc +lat_0=0 +lon_0=99 +k=1 +x_0=17500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 17",
            "CGCS2000",
        ),
        (
            4507,
            "CGCS2000 / Gauss-Kruger zone 18",
            "+proj=tmerc +lat_0=0 +lon_0=105 +k=1 +x_0=18500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 18",
            "CGCS2000",
        ),
        (
            4508,
            "CGCS2000 / Gauss-Kruger zone 19",
            "+proj=tmerc +lat_0=0 +lon_0=111 +k=1 +x_0=19500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 19",
            "CGCS2000",
        ),
        (
            4509,
            "CGCS2000 / Gauss-Kruger zone 20",
            "+proj=tmerc +lat_0=0 +lon_0=117 +k=1 +x_0=20500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 20",
            "CGCS2000",
        ),
        (
            4510,
            "CGCS2000 / Gauss-Kruger zone 21",
            "+proj=tmerc +lat_0=0 +lon_0=123 +k=1 +x_0=21500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 21",
            "CGCS2000",
        ),
        (
            4511,
            "CGCS2000 / Gauss-Kruger zone 22",
            "+proj=tmerc +lat_0=0 +lon_0=129 +k=1 +x_0=22500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 22",
            "CGCS2000",
        ),
        (
            4512,
            "CGCS2000 / Gauss-Kruger zone 23",
            "+proj=tmerc +lat_0=0 +lon_0=135 +k=1 +x_0=23500000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "China zone 23",
            "CGCS2000",
        ),
        // Korean grids
        (
            5174,
            "Korean 1985 / Modified Central",
            "+proj=tmerc +lat_0=38 +lon_0=127 +k=1 +x_0=200000 +y_0=500000 +ellps=bessel +units=m +no_defs",
            "South Korea",
            "Korean 1985",
        ),
        (
            5178,
            "Korean 1985 / Unified CS",
            "+proj=tmerc +lat_0=38 +lon_0=127.5 +k=0.9996 +x_0=1000000 +y_0=2000000 +ellps=bessel +units=m +no_defs",
            "South Korea",
            "Korean 1985",
        ),
        (
            5179,
            "Korea 2000 / Unified CS",
            "+proj=tmerc +lat_0=38 +lon_0=127.5 +k=0.9996 +x_0=1000000 +y_0=2000000 +ellps=GRS80 +units=m +no_defs",
            "South Korea",
            "Korea 2000",
        ),
        (
            5186,
            "Korea 2000 / Central Belt 2010",
            "+proj=tmerc +lat_0=38 +lon_0=127 +k=1 +x_0=200000 +y_0=600000 +ellps=GRS80 +units=m +no_defs",
            "South Korea Central",
            "Korea 2000",
        ),
        // Indian grids
        (
            24378,
            "Kalianpur 1975 / India zone I",
            "+proj=lcc +lat_1=32.5 +lat_0=32.5 +lon_0=68 +k_0=0.99878641 +x_0=2743195.5 +y_0=914398.5 +a=6377299.151 +b=6356098.145120132 +units=m +no_defs",
            "India zone I",
            "Kalianpur 1975",
        ),
        (
            24379,
            "Kalianpur 1975 / India zone IIa",
            "+proj=lcc +lat_1=26 +lat_0=26 +lon_0=74 +k_0=0.99878641 +x_0=2743195.5 +y_0=914398.5 +a=6377299.151 +b=6356098.145120132 +units=m +no_defs",
            "India zone IIa",
            "Kalianpur 1975",
        ),
        (
            24380,
            "Kalianpur 1975 / India zone IIb",
            "+proj=lcc +lat_1=26 +lat_0=26 +lon_0=90 +k_0=0.99878641 +x_0=2743195.5 +y_0=914398.5 +a=6377299.151 +b=6356098.145120132 +units=m +no_defs",
            "India zone IIb",
            "Kalianpur 1975",
        ),
        (
            24381,
            "Kalianpur 1975 / India zone III",
            "+proj=lcc +lat_1=19 +lat_0=19 +lon_0=80 +k_0=0.99878641 +x_0=2743195.5 +y_0=914398.5 +a=6377299.151 +b=6356098.145120132 +units=m +no_defs",
            "India zone III",
            "Kalianpur 1975",
        ),
        (
            24382,
            "Kalianpur 1975 / India zone IV",
            "+proj=lcc +lat_1=12 +lat_0=12 +lon_0=80 +k_0=0.99878641 +x_0=2743195.5 +y_0=914398.5 +a=6377299.151 +b=6356098.145120132 +units=m +no_defs",
            "India zone IV",
            "Kalianpur 1975",
        ),
        // Philippine grid
        (
            3121,
            "PRS92 / Philippines zone 1",
            "+proj=tmerc +lat_0=0 +lon_0=117 +k=0.99995 +x_0=500000 +y_0=0 +ellps=clrk66 +units=m +no_defs",
            "Philippines zone 1",
            "PRS92",
        ),
        (
            3122,
            "PRS92 / Philippines zone 2",
            "+proj=tmerc +lat_0=0 +lon_0=119 +k=0.99995 +x_0=500000 +y_0=0 +ellps=clrk66 +units=m +no_defs",
            "Philippines zone 2",
            "PRS92",
        ),
        (
            3123,
            "PRS92 / Philippines zone 3",
            "+proj=tmerc +lat_0=0 +lon_0=121 +k=0.99995 +x_0=500000 +y_0=0 +ellps=clrk66 +units=m +no_defs",
            "Philippines zone 3",
            "PRS92",
        ),
        (
            3124,
            "PRS92 / Philippines zone 4",
            "+proj=tmerc +lat_0=0 +lon_0=123 +k=0.99995 +x_0=500000 +y_0=0 +ellps=clrk66 +units=m +no_defs",
            "Philippines zone 4",
            "PRS92",
        ),
        (
            3125,
            "PRS92 / Philippines zone 5",
            "+proj=tmerc +lat_0=0 +lon_0=125 +k=0.99995 +x_0=500000 +y_0=0 +ellps=clrk66 +units=m +no_defs",
            "Philippines zone 5",
            "PRS92",
        ),
        // Thai grid
        (
            24047,
            "Indian 1975 / UTM zone 47N",
            "+proj=utm +zone=47 +a=6377276.345 +b=6356075.413 +units=m +no_defs",
            "Thailand (96°E to 102°E)",
            "Indian 1975",
        ),
        (
            24048,
            "Indian 1975 / UTM zone 48N",
            "+proj=utm +zone=48 +a=6377276.345 +b=6356075.413 +units=m +no_defs",
            "Thailand (102°E to 108°E)",
            "Indian 1975",
        ),
        // New Zealand grids
        (
            2193,
            "NZGD2000 / New Zealand Transverse Mercator 2000",
            "+proj=tmerc +lat_0=0 +lon_0=173 +k=0.9996 +x_0=1600000 +y_0=10000000 +ellps=GRS80 +units=m +no_defs",
            "New Zealand",
            "NZGD2000",
        ),
        (
            27200,
            "NZGD49 / New Zealand Map Grid",
            "+proj=nzmg +lat_0=-41 +lon_0=173 +x_0=2510000 +y_0=6023150 +ellps=intl +datum=nzgd49 +units=m +no_defs",
            "New Zealand",
            "NZGD49",
        ),
        // Australian grids (non-UTM)
        (
            3112,
            "GDA94 / Geoscience Australia Lambert",
            "+proj=lcc +lat_1=-18 +lat_2=-36 +lat_0=0 +lon_0=134 +x_0=0 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Australia",
            "GDA94",
        ),
        (
            7845,
            "GDA2020 / GA LCC",
            "+proj=lcc +lat_1=-18 +lat_2=-36 +lat_0=0 +lon_0=134 +x_0=0 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Australia",
            "GDA2020",
        ),
        // Taiwan grid
        (
            3826,
            "TWD97 / TM2 zone 121",
            "+proj=tmerc +lat_0=0 +lon_0=121 +k=0.9999 +x_0=250000 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "Taiwan",
            "TWD97",
        ),
        (
            3828,
            "TWD67 / TM2 zone 121",
            "+proj=tmerc +lat_0=0 +lon_0=121 +k=0.9999 +x_0=250000 +y_0=0 +ellps=aust_SA +units=m +no_defs",
            "Taiwan",
            "TWD67",
        ),
        // Singapore grid
        (
            3414,
            "SVY21 / Singapore TM",
            "+proj=tmerc +lat_0=1.3666667 +lon_0=103.8333333 +k=1 +x_0=28001.642 +y_0=38744.572 +ellps=WGS84 +units=m +no_defs",
            "Singapore",
            "SVY21",
        ),
        // Hong Kong grid
        (
            2326,
            "Hong Kong 1980 Grid System",
            "+proj=tmerc +lat_0=22.3121333 +lon_0=114.1785556 +k=1 +x_0=836694.05 +y_0=819069.8 +ellps=intl +units=m +no_defs",
            "Hong Kong",
            "Hong Kong 1980",
        ),
        // Macau grid
        (
            8428,
            "Macau 2008 / Macau TM",
            "+proj=tmerc +lat_0=22.2123972 +lon_0=113.5364694 +k=1 +x_0=20000 +y_0=20000 +ellps=GRS80 +units=m +no_defs",
            "Macau",
            "Macau 2008",
        ),
    ];

    for &(code, name, proj_string, area, datum) in asian_grids {
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
