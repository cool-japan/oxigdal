//! Vertical CRS registrations (NAVD88, EGM96, EGM2008, etc.).

use super::super::types::{CrsType, EpsgDatabase, EpsgDefinition};

pub(super) fn register_vertical_crs(db: &mut EpsgDatabase) {
    let vertical_crs: &[(u32, &str, &str, &str)] = &[
        (
            5701,
            "ODN height",
            "United Kingdom",
            "Ordnance Datum Newlyn",
        ),
        (5703, "NAVD88 height", "North America", "NAVD88"),
        (5714, "MSL height", "World (mean sea level)", "MSL"),
        (5773, "EGM96 height", "World (EGM96 geoid)", "EGM96"),
        (3855, "EGM2008 height", "World (EGM2008 geoid)", "EGM2008"),
        (5709, "NAP height", "Netherlands", "NAP"),
        (5710, "Ostend height", "Belgium", "Ostend"),
        (5711, "AHD height", "Australia", "AHD"),
        (5712, "AHD (Tasmania) height", "Tasmania", "AHD (Tasmania)"),
        (5717, "CGVD28 height", "Canada (historic)", "CGVD28"),
        (5613, "CGVD2013a height", "Canada", "CGVD2013a"),
        (5720, "Belfast height", "Northern Ireland", "Belfast"),
        (5721, "Dublin height", "Ireland", "Dublin"),
        (5782, "DVR90 height", "Denmark", "DVR90"),
        (5783, "DHHN92 height", "Germany", "DHHN92"),
        (7837, "DHHN2016 height", "Germany (modern)", "DHHN2016"),
        (5776, "NN2000 height", "Norway", "NN2000"),
        (5799, "RH2000 height", "Sweden", "RH2000"),
        (
            5861,
            "LAT height",
            "World (lowest astronomical tide)",
            "LAT",
        ),
        (
            5862,
            "LLWLT height",
            "Tidal (lower low water large tide)",
            "LLWLT",
        ),
        (
            6360,
            "NAVD88 (metric) height",
            "USA (metric variant)",
            "NAVD88",
        ),
    ];

    for &(code, name, area, datum) in vertical_crs {
        db.add_definition(EpsgDefinition {
            code,
            name: name.to_string(),
            proj_string: String::new(),
            wkt: None,
            crs_type: CrsType::Vertical,
            area_of_use: area.to_string(),
            unit: "metre".to_string(),
            datum: datum.to_string(),
        });
    }
}
