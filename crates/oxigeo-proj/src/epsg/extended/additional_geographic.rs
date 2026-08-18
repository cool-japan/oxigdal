//! Additional geographic CRS registrations (modern realizations such as ITRF, JGD2011, GDA2020).

use super::super::types::{CrsType, EpsgDatabase, EpsgDefinition};
use alloc::string::ToString;

pub(super) fn register_additional_geographic(db: &mut EpsgDatabase) {
    let geographic_crs: &[(u32, &str, &str, &str, &str)] = &[
        // Modern datums
        (
            4167,
            "NZGD2000",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "New Zealand",
            "NZGD2000",
        ),
        (
            4171,
            "RGF93 v1",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "France",
            "RGF93",
        ),
        (
            4178,
            "Pulkovo 1942(83)",
            "+proj=longlat +ellps=krass +no_defs",
            "Eastern Europe",
            "Pulkovo 1942(83)",
        ),
        (
            4258,
            "ETRS89",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "Europe",
            "ETRS89",
        ),
        (
            4269,
            "NAD83",
            "+proj=longlat +datum=NAD83 +no_defs",
            "North America",
            "NAD83",
        ),
        (
            4272,
            "NZGD49",
            "+proj=longlat +datum=nzgd49 +no_defs",
            "New Zealand (historic)",
            "NZGD49",
        ),
        (
            4275,
            "NTF",
            "+proj=longlat +a=6378249.2 +b=6356515 +no_defs",
            "France (historic)",
            "NTF",
        ),
        (
            4284,
            "Pulkovo 1942",
            "+proj=longlat +ellps=krass +no_defs",
            "Russia / FSU",
            "Pulkovo 1942",
        ),
        (
            4314,
            "DHDN",
            "+proj=longlat +ellps=bessel +no_defs",
            "Germany (historic)",
            "DHDN",
        ),
        (
            4326,
            "WGS 84",
            "+proj=longlat +datum=WGS84 +no_defs",
            "World",
            "WGS84",
        ),
        (
            4490,
            "CGCS2000",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "China",
            "CGCS2000",
        ),
        (
            4555,
            "New Beijing",
            "+proj=longlat +ellps=krass +no_defs",
            "China (historic)",
            "New Beijing",
        ),
        (
            4612,
            "JGD2000",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "Japan",
            "JGD2000",
        ),
        (
            4618,
            "SAD69",
            "+proj=longlat +ellps=aust_SA +no_defs",
            "South America",
            "SAD69",
        ),
        (
            4674,
            "SIRGAS 2000",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "South America",
            "SIRGAS 2000",
        ),
        (
            4737,
            "Korea 2000",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "South Korea",
            "Korea 2000",
        ),
        (
            4745,
            "RD/83",
            "+proj=longlat +ellps=bessel +no_defs",
            "Germany Saxony (historic)",
            "RD/83",
        ),
        (
            4818,
            "S-JTSK (Ferro)",
            "+proj=longlat +ellps=bessel +pm=ferro +no_defs",
            "Czech Republic (historic)",
            "S-JTSK",
        ),
        (
            4979,
            "WGS 84",
            "+proj=longlat +datum=WGS84 +no_defs",
            "World (3D geographic)",
            "WGS84",
        ),
        (
            5013,
            "PTRA08",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "Azores / Madeira",
            "PTRA08",
        ),
        (
            5323,
            "ISN2004",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "Iceland",
            "ISN2004",
        ),
        (
            5324,
            "ISN2004",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "Iceland",
            "ISN2004",
        ),
        (
            6318,
            "NAD83(2011)",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "North America",
            "NAD83(2011)",
        ),
        (
            6668,
            "JGD2011",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "Japan",
            "JGD2011",
        ),
        // ITRF realizations occupy EPSG:7900–7912 in chronological order.
        // The historical registry placed ITRF97/2000/2005 on 7900/7901/7902,
        // which are actually ITRF88/89/90 — every frame off by four
        // realizations — and claimed EPSG:7930 as ITRF2008 when it is in fact
        // ETRF2000, a European rather than global frame. Verified against
        // PROJ 9.5.1; the real ITRF2008 is EPSG:7911.
        (
            7900,
            "ITRF88",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 1988)",
            "ITRF88",
        ),
        (
            7901,
            "ITRF89",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 1989)",
            "ITRF89",
        ),
        (
            7902,
            "ITRF90",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 1990)",
            "ITRF90",
        ),
        (
            7903,
            "ITRF91",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 1991)",
            "ITRF91",
        ),
        (
            7904,
            "ITRF92",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 1992)",
            "ITRF92",
        ),
        (
            7905,
            "ITRF93",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 1993)",
            "ITRF93",
        ),
        (
            7906,
            "ITRF94",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 1994)",
            "ITRF94",
        ),
        (
            7907,
            "ITRF96",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 1996)",
            "ITRF96",
        ),
        (
            7908,
            "ITRF97",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 1997)",
            "ITRF97",
        ),
        (
            7909,
            "ITRF2000",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 2000)",
            "ITRF2000",
        ),
        (
            7910,
            "ITRF2005",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 2005)",
            "ITRF2005",
        ),
        (
            7911,
            "ITRF2008",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 2008)",
            "ITRF2008",
        ),
        (
            7912,
            "ITRF2014",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 2014)",
            "ITRF2014",
        ),
        (
            7930,
            "ETRF2000",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "Europe (kinematic 2000)",
            "ETRF2000",
        ),
        (
            7789,
            "ITRF2014",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic)",
            "ITRF2014",
        ),
        (
            7844,
            "GDA2020",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "Australia",
            "GDA2020",
        ),
        (
            8232,
            "NAD83(CSRS96)",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "Canada",
            "NAD83(CSRS96)",
        ),
        (
            9000,
            "ITRF2014",
            "+proj=longlat +ellps=GRS80 +no_defs",
            "World (kinematic 2014, IGS realization)",
            "ITRF2014",
        ),
        (
            9755,
            "WGS 84 (G2139)",
            "+proj=longlat +datum=WGS84 +no_defs",
            "World (latest realization)",
            "WGS 84 (G2139)",
        ),
    ];

    for &(code, name, proj_string, area, datum) in geographic_crs {
        // Only add if not already in the database (avoid duplicate WGS84 etc.)
        if !db.contains(code) {
            db.add_definition(EpsgDefinition {
                code,
                name: name.to_string(),
                proj_string: proj_string.to_string(),
                wkt: None,
                crs_type: CrsType::Geographic,
                area_of_use: area.to_string(),
                unit: "degree".to_string(),
                datum: datum.to_string(),
            });
        }
    }
}
