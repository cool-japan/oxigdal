//! Comprehensive tests for CRS registry, WKT parsing extensions, and PROJ string parsing.

#![allow(clippy::expect_used)]

use oxigdal_proj::crs_registry::{AxisOrder, CrsRegistry, CrsType, CrsUnit};
use oxigdal_proj::proj_string::ProjString;
use oxigdal_proj::wkt::{WktParser, WktVersion};

// =============================================================================
// CrsRegistry tests
// =============================================================================

#[test]
fn test_default_registry_count_at_least_50() {
    let reg = CrsRegistry::default_registry();
    assert!(
        reg.count() >= 50,
        "expected >= 50 CRS definitions, got {}",
        reg.count()
    );
}

#[test]
fn test_registry_count_includes_utm_zones() {
    let reg = CrsRegistry::default_registry();
    // 60 N + 60 S UTM + at least 18 other CRS
    assert!(reg.count() >= 138, "expected >= 138, got {}", reg.count());
}

#[test]
fn test_get_4326_returns_wgs84() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(4326).expect("EPSG:4326 must exist");
    assert!(
        def.name.contains("WGS 84") || def.name.contains("WGS84"),
        "name should mention WGS 84, got: {}",
        def.name
    );
}

#[test]
fn test_get_3857_returns_web_mercator() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(3857).expect("EPSG:3857 must exist");
    let name_lower = def.name.to_lowercase();
    assert!(
        name_lower.contains("mercator") || name_lower.contains("pseudo"),
        "name should mention Mercator, got: {}",
        def.name
    );
}

#[test]
fn test_get_32632_returns_utm_zone_32n() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(32632).expect("EPSG:32632 must exist");
    assert!(
        def.name.contains("32N"),
        "name should contain '32N', got: {}",
        def.name
    );
}

#[test]
fn test_get_32732_returns_utm_zone_32s() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(32732).expect("EPSG:32732 must exist");
    assert!(
        def.name.contains("32S"),
        "name should contain '32S', got: {}",
        def.name
    );
}

#[test]
fn test_get_27700_returns_british_national_grid() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(27700).expect("EPSG:27700 must exist");
    let name_lower = def.name.to_lowercase();
    assert!(
        name_lower.contains("british")
            || name_lower.contains("national")
            || name_lower.contains("osgb"),
        "name should mention British National Grid, got: {}",
        def.name
    );
}

#[test]
fn test_get_2154_returns_lambert_93() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(2154).expect("EPSG:2154 must exist");
    let name_lower = def.name.to_lowercase();
    assert!(
        name_lower.contains("lambert") || name_lower.contains("rgf93"),
        "name should mention Lambert-93, got: {}",
        def.name
    );
}

#[test]
fn test_get_25832_returns_etrs89_utm32n() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(25832).expect("EPSG:25832 must exist");
    assert!(
        def.name.contains("32N") || def.name.contains("32n"),
        "name should contain UTM 32N info, got: {}",
        def.name
    );
}

#[test]
fn test_get_nonexistent_returns_none() {
    let reg = CrsRegistry::default_registry();
    assert!(reg.get(99999).is_none());
    assert!(reg.get(0).is_none());
    assert!(reg.get(-1).is_none());
}

#[test]
fn test_find_by_name_wgs84_finds_4326() {
    let reg = CrsRegistry::default_registry();
    let results = reg.find_by_name("WGS 84");
    let found = results.iter().any(|d| d.epsg_code == Some(4326));
    assert!(found, "find_by_name('WGS 84') should include EPSG:4326");
}

#[test]
fn test_find_by_name_case_insensitive() {
    let reg = CrsRegistry::default_registry();
    let lower = reg.find_by_name("wgs 84");
    let upper = reg.find_by_name("WGS 84");
    assert!(
        !lower.is_empty(),
        "case-insensitive search should find results"
    );
    assert_eq!(
        lower.len(),
        upper.len(),
        "case should not affect result count"
    );
}

#[test]
fn test_find_by_name_empty_results() {
    let reg = CrsRegistry::default_registry();
    let results = reg.find_by_name("XYZNONEXISTENT12345QRST");
    assert!(results.is_empty());
}

#[test]
fn test_by_type_geographic2d_all_geographic() {
    let reg = CrsRegistry::default_registry();
    let results = reg.by_type(&CrsType::Geographic2D);
    assert!(!results.is_empty());
    for def in &results {
        assert_eq!(
            def.crs_type,
            CrsType::Geographic2D,
            "{} should be Geographic2D",
            def.name
        );
    }
}

#[test]
fn test_by_type_projected_all_projected() {
    let reg = CrsRegistry::default_registry();
    let results = reg.by_type(&CrsType::Projected);
    assert!(!results.is_empty());
    for def in &results {
        assert_eq!(
            def.crs_type,
            CrsType::Projected,
            "{} should be Projected",
            def.name
        );
    }
}

#[test]
fn test_by_type_geographic2d_includes_4326() {
    let reg = CrsRegistry::default_registry();
    let results = reg.by_type(&CrsType::Geographic2D);
    let found = results.iter().any(|d| d.epsg_code == Some(4326));
    assert!(found, "Geographic2D results should include EPSG:4326");
}

#[test]
fn test_by_type_projected_includes_3857() {
    let reg = CrsRegistry::default_registry();
    let results = reg.by_type(&CrsType::Projected);
    let found = results.iter().any(|d| d.epsg_code == Some(3857));
    assert!(found, "Projected results should include EPSG:3857");
}

#[test]
fn test_covering_point_london_includes_27700() {
    let reg = CrsRegistry::default_registry();
    // London: lat=51.5, lon=-0.1
    let results = reg.covering_point(51.5, -0.1);
    let found = results.iter().any(|d| d.epsg_code == Some(27700));
    assert!(
        found,
        "covering_point(51.5, -0.1) should include EPSG:27700 (British National Grid)"
    );
}

#[test]
fn test_covering_point_london_includes_4326() {
    let reg = CrsRegistry::default_registry();
    let results = reg.covering_point(51.5, -0.1);
    let found = results.iter().any(|d| d.epsg_code == Some(4326));
    assert!(
        found,
        "covering_point(51.5, -0.1) should include EPSG:4326 (world CRS)"
    );
}

#[test]
fn test_covering_point_france_includes_2154() {
    let reg = CrsRegistry::default_registry();
    // Paris: lat=48.8, lon=2.3
    let results = reg.covering_point(48.8, 2.3);
    let found = results.iter().any(|d| d.epsg_code == Some(2154));
    assert!(
        found,
        "covering_point(48.8, 2.3) should include EPSG:2154 (Lambert-93)"
    );
}

#[test]
fn test_covering_point_germany_includes_25832() {
    let reg = CrsRegistry::default_registry();
    // Frankfurt: lat=50.1, lon=8.7
    let results = reg.covering_point(50.1, 8.7);
    let found = results.iter().any(|d| d.epsg_code == Some(25832));
    assert!(
        found,
        "covering_point(50.1, 8.7) should include EPSG:25832 (ETRS89/UTM 32N)"
    );
}

#[test]
fn test_covering_point_south_pole_not_includes_27700() {
    let reg = CrsRegistry::default_registry();
    let results = reg.covering_point(-90.0, 0.0);
    let found = results.iter().any(|d| d.epsg_code == Some(27700));
    assert!(!found, "South Pole should not be covered by EPSG:27700");
}

#[test]
fn test_register_custom_crs() {
    use oxigdal_proj::crs_registry::CrsDefinition;
    let mut reg = CrsRegistry::default_registry();
    let before = reg.count();
    reg.register(CrsDefinition {
        epsg_code: Some(99001),
        name: "My Custom CRS".to_string(),
        crs_type: CrsType::Projected,
        datum: "WGS84".to_string(),
        unit: CrsUnit::Metre,
        proj_string: None,
        wkt_name: None,
        area_of_use: None,
        deprecated: false,
    });
    assert_eq!(reg.count(), before + 1);
    let def = reg.get(99001).expect("custom CRS should be retrievable");
    assert_eq!(def.name, "My Custom CRS");
}

#[test]
fn test_count_increases_after_register() {
    use oxigdal_proj::crs_registry::CrsDefinition;
    let mut reg = CrsRegistry::new();
    assert_eq!(reg.count(), 0);
    reg.register(CrsDefinition {
        epsg_code: Some(1),
        name: "Test".to_string(),
        crs_type: CrsType::Geographic2D,
        datum: "Test".to_string(),
        unit: CrsUnit::Degree,
        proj_string: None,
        wkt_name: None,
        area_of_use: None,
        deprecated: false,
    });
    assert_eq!(reg.count(), 1);
}

#[test]
fn test_utm_zones_count_120() {
    let reg = CrsRegistry::default_registry();
    // Check 60 north + 60 south zones are all present
    for zone in 1u8..=60u8 {
        let n_code = 32600 + i32::from(zone);
        let s_code = 32700 + i32::from(zone);
        assert!(
            reg.get(n_code).is_some(),
            "EPSG:{n_code} (UTM zone {zone}N) should be present"
        );
        assert!(
            reg.get(s_code).is_some(),
            "EPSG:{s_code} (UTM zone {zone}S) should be present"
        );
    }
}

#[test]
fn test_get_32601_zone_1n() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(32601).expect("EPSG:32601 must exist");
    assert!(def.name.contains("1N"), "got: {}", def.name);
}

#[test]
fn test_get_32660_zone_60n() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(32660).expect("EPSG:32660 must exist");
    assert!(def.name.contains("60N"), "got: {}", def.name);
}

#[test]
fn test_get_32701_zone_1s() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(32701).expect("EPSG:32701 must exist");
    assert!(def.name.contains("1S"), "got: {}", def.name);
}

#[test]
fn test_get_32760_zone_60s() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(32760).expect("EPSG:32760 must exist");
    assert!(def.name.contains("60S"), "got: {}", def.name);
}

#[test]
fn test_crs_4326_is_geographic() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(4326).expect("EPSG:4326 must exist");
    assert!(def.is_geographic());
}

#[test]
fn test_crs_3857_is_projected() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(3857).expect("EPSG:3857 must exist");
    assert!(def.is_projected());
}

#[test]
fn test_crs_4326_not_projected() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(4326).expect("EPSG:4326 must exist");
    assert!(!def.is_projected());
}

#[test]
fn test_crs_3857_not_geographic() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(3857).expect("EPSG:3857 must exist");
    assert!(!def.is_geographic());
}

#[test]
fn test_crs_4326_axis_order_north_east() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(4326).expect("EPSG:4326 must exist");
    assert_eq!(def.axis_order(), AxisOrder::NorthEast);
}

#[test]
fn test_crs_3857_axis_order_east_north() {
    let reg = CrsRegistry::default_registry();
    let def = reg.get(3857).expect("EPSG:3857 must exist");
    assert_eq!(def.axis_order(), AxisOrder::EastNorth);
}

// =============================================================================
// CrsUnit tests
// =============================================================================

#[test]
fn test_metre_to_metres_is_1() {
    let diff = (CrsUnit::Metre.to_metres() - 1.0).abs();
    assert!(
        diff < f64::EPSILON,
        "Metre.to_metres() should be exactly 1.0"
    );
}

#[test]
fn test_foot_to_metres_approx_0_3048() {
    let v = CrsUnit::Foot.to_metres();
    assert!(
        (v - 0.3048).abs() < 0.001,
        "Foot.to_metres() should be ~0.3048, got {v}"
    );
}

#[test]
fn test_foot_intl_to_metres_exact_0_3048() {
    let v = CrsUnit::FootIntl.to_metres();
    assert!(
        (v - 0.3048).abs() < f64::EPSILON,
        "FootIntl.to_metres() should be exactly 0.3048, got {v}"
    );
}

#[test]
fn test_degree_to_metres_positive() {
    let v = CrsUnit::Degree.to_metres();
    assert!(v > 0.0, "Degree.to_metres() should be positive");
    // ~111319 m/degree at equator
    assert!(v > 100_000.0 && v < 200_000.0, "expected ~111319, got {v}");
}

#[test]
fn test_kilometre_to_metres_is_1000() {
    let v = CrsUnit::Kilometre.to_metres();
    assert!((v - 1000.0).abs() < f64::EPSILON);
}

#[test]
fn test_radian_to_metres_positive() {
    let v = CrsUnit::Radian.to_metres();
    assert!(v > 0.0);
    // Should be approximately Earth radius = 6371000 m
    assert!(
        v > 6_000_000.0 && v < 7_000_000.0,
        "expected ~6371000, got {v}"
    );
}

#[test]
fn test_metre_name() {
    assert_eq!(CrsUnit::Metre.name(), "metre");
}

#[test]
fn test_foot_name() {
    let name = CrsUnit::Foot.name();
    assert!(
        name.contains("foot") || name.contains("survey"),
        "got: {name}"
    );
}

#[test]
fn test_degree_name() {
    let name = CrsUnit::Degree.name();
    assert!(name.contains("degree"), "got: {name}");
}

// =============================================================================
// WktParser static method tests
// =============================================================================

#[test]
fn test_extract_name_from_projcs() {
    let wkt = r#"PROJCS["WGS 84 / UTM zone 32N",GEOGCS["WGS 84"]]"#;
    let name = WktParser::extract_name(wkt);
    assert_eq!(name, Some("WGS 84 / UTM zone 32N".to_string()));
}

#[test]
fn test_extract_name_from_geogcs() {
    let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984"]]"#;
    let name = WktParser::extract_name(wkt);
    assert_eq!(name, Some("WGS 84".to_string()));
}

#[test]
fn test_extract_name_from_projcrs_wkt2() {
    let wkt = r#"PROJCRS["WGS 84 / UTM zone 32N",BASEGEOGCRS["WGS 84"]]"#;
    let name = WktParser::extract_name(wkt);
    assert_eq!(name, Some("WGS 84 / UTM zone 32N".to_string()));
}

#[test]
fn test_extract_name_none_for_empty() {
    let name = WktParser::extract_name("");
    assert!(name.is_none());
}

#[test]
fn test_extract_name_none_without_bracket() {
    // No '[' in the string
    let name = WktParser::extract_name("GEOGCS");
    assert!(name.is_none());
}

#[test]
fn test_extract_epsg_from_authority() {
    let wkt = r#"GEOGCS["WGS 84",AUTHORITY["EPSG","4326"]]"#;
    let epsg = WktParser::extract_epsg(wkt);
    assert_eq!(epsg, Some(4326));
}

#[test]
fn test_extract_epsg_from_id_form() {
    let wkt = r#"PROJCRS["WGS 84 / UTM zone 32N",ID["EPSG",32632]]"#;
    let epsg = WktParser::extract_epsg(wkt);
    assert_eq!(epsg, Some(32632));
}

#[test]
fn test_extract_epsg_none_for_no_authority() {
    let wkt = r#"GEOGCS["My Custom CRS",DATUM["My Datum"]]"#;
    let epsg = WktParser::extract_epsg(wkt);
    assert!(epsg.is_none());
}

#[test]
fn test_extract_epsg_different_authority() {
    // Non-EPSG authority should return None
    let wkt = r#"GEOGCS["NAD83",AUTHORITY["ESRI","104019"]]"#;
    let epsg = WktParser::extract_epsg(wkt);
    assert!(epsg.is_none());
}

#[test]
fn test_detect_version_wkt1_projcs() {
    let wkt = r#"PROJCS["WGS 84 / UTM zone 32N",GEOGCS["WGS 84"]]"#;
    assert_eq!(WktParser::detect_version(wkt), WktVersion::Wkt1);
}

#[test]
fn test_detect_version_wkt1_geogcs() {
    let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984"]]"#;
    assert_eq!(WktParser::detect_version(wkt), WktVersion::Wkt1);
}

#[test]
fn test_detect_version_wkt2_projcrs() {
    let wkt = r#"PROJCRS["WGS 84 / UTM zone 32N",BASEGEOGCRS["WGS 84"]]"#;
    assert_eq!(WktParser::detect_version(wkt), WktVersion::Wkt2);
}

#[test]
fn test_detect_version_wkt2_geogcrs() {
    let wkt = r#"GEOGCRS["WGS 84",DATUM["WGS 1984"]]"#;
    assert_eq!(WktParser::detect_version(wkt), WktVersion::Wkt2);
}

#[test]
fn test_detect_version_unknown() {
    let wkt = "not a wkt string";
    assert_eq!(WktParser::detect_version(wkt), WktVersion::Unknown);
}

#[test]
fn test_extract_unit_metre() {
    let wkt = r#"PROJCS["test",UNIT["metre",1]]"#;
    let unit = WktParser::extract_unit(wkt);
    assert!(unit.is_some(), "should find UNIT");
    let (name, factor) = unit.expect("unit present");
    assert_eq!(name, "metre");
    assert!((factor - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_extract_unit_degree() {
    let wkt = r#"GEOGCS["WGS 84",UNIT["degree",0.0174532925199433]]"#;
    let unit = WktParser::extract_unit(wkt);
    assert!(unit.is_some());
    let (name, factor) = unit.expect("unit present");
    assert_eq!(name, "degree");
    assert!((factor - 0.0174532925199433).abs() < 1e-15);
}

#[test]
fn test_extract_unit_none_for_no_unit() {
    let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984"]]"#;
    let unit = WktParser::extract_unit(wkt);
    assert!(unit.is_none());
}

#[test]
fn test_parse_crs_from_projcs_wkt() {
    let wkt = r#"PROJCS["WGS 84 / UTM zone 32N",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]],UNIT["metre",1],AUTHORITY["EPSG","32632"]]"#;
    let result = WktParser::parse_crs(wkt);
    assert!(result.is_ok(), "parse_crs failed: {:?}", result.err());
    let def = result.expect("valid");
    assert!(def.name.contains("UTM zone 32N"));
    assert_eq!(def.epsg_code, Some(32632));
    assert!(def.is_projected());
}

#[test]
fn test_parse_crs_from_geogcs_wkt() {
    let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],AUTHORITY["EPSG","4326"]]"#;
    let result = WktParser::parse_crs(wkt);
    assert!(result.is_ok(), "parse_crs failed: {:?}", result.err());
    let def = result.expect("valid");
    assert_eq!(def.name, "WGS 84");
    assert_eq!(def.epsg_code, Some(4326));
    assert!(def.is_geographic());
}

#[test]
fn test_parse_crs_error_on_empty() {
    let result = WktParser::parse_crs("");
    assert!(result.is_err());
}

#[test]
fn test_parse_crs_error_on_no_name() {
    // No quoted name after bracket
    let result = WktParser::parse_crs("GEOGCS[DATUM]");
    assert!(result.is_err());
}

// =============================================================================
// ProjString tests
// =============================================================================

#[test]
fn test_parse_utm_proj() {
    let ps = ProjString::parse("+proj=utm +zone=32 +datum=WGS84 +units=m +no_defs")
        .expect("valid PROJ string");
    assert_eq!(ps.proj(), Some("utm"));
}

#[test]
fn test_parse_utm_zone_value() {
    let ps = ProjString::parse("+proj=utm +zone=32 +datum=WGS84").expect("valid");
    assert_eq!(ps.zone(), Some(32));
}

#[test]
fn test_parse_utm_datum_value() {
    let ps = ProjString::parse("+proj=utm +zone=32 +datum=WGS84").expect("valid");
    assert_eq!(ps.datum(), Some("WGS84"));
}

#[test]
fn test_parse_utm_has_no_defs() {
    let ps = ProjString::parse("+proj=utm +zone=32 +datum=WGS84 +no_defs").expect("valid");
    assert!(ps.has("no_defs"));
}

#[test]
fn test_parse_utm_units() {
    let ps = ProjString::parse("+proj=utm +zone=32 +datum=WGS84 +units=m").expect("valid");
    assert_eq!(ps.units(), Some("m"));
}

#[test]
fn test_parse_wgs84_static() {
    let ps = ProjString::wgs84();
    assert_eq!(ps.proj(), Some("longlat"));
    assert_eq!(ps.datum(), Some("WGS84"));
    assert!(ps.has("no_defs"));
}

#[test]
fn test_parse_utm_static_zone() {
    let ps = ProjString::utm(32, false);
    assert_eq!(ps.proj(), Some("utm"));
    assert_eq!(ps.zone(), Some(32));
}

#[test]
fn test_parse_utm_south_has_south_flag() {
    let ps = ProjString::utm(15, true);
    assert!(
        ps.has("south"),
        "southern hemisphere UTM should have +south flag"
    );
    assert_eq!(ps.zone(), Some(15));
}

#[test]
fn test_parse_utm_north_no_south_flag() {
    let ps = ProjString::utm(32, false);
    assert!(
        !ps.has("south"),
        "northern hemisphere UTM should NOT have +south flag"
    );
}

#[test]
fn test_parse_web_mercator_proj() {
    let ps = ProjString::web_mercator();
    assert_eq!(ps.proj(), Some("merc"));
    assert_eq!(ps.units(), Some("m"));
    assert!(ps.has("no_defs"));
}

#[test]
fn test_towgs84_parses_7_values() {
    let ps =
        ProjString::parse("+proj=tmerc +towgs84=598.1,73.7,418.2,0.202,0.045,-2.455,6.7 +no_defs")
            .expect("valid");
    let params = ps.towgs84().expect("towgs84 should parse");
    assert!((params[0] - 598.1).abs() < 1e-9);
    assert!((params[1] - 73.7).abs() < 1e-9);
    assert!((params[2] - 418.2).abs() < 1e-9);
    assert!((params[3] - 0.202).abs() < 1e-9);
    assert!((params[4] - 0.045).abs() < 1e-9);
    assert!((params[5] - -2.455).abs() < 1e-9);
    assert!((params[6] - 6.7).abs() < 1e-9);
}

#[test]
fn test_to_string_contains_proj() {
    let ps = ProjString::parse("+proj=longlat +datum=WGS84 +no_defs").expect("valid");
    let s = ps.to_proj_string();
    assert!(s.contains("+proj=longlat"), "got: {s}");
}

#[test]
fn test_to_string_contains_zone() {
    let ps = ProjString::parse("+proj=utm +zone=32 +datum=WGS84 +no_defs").expect("valid");
    let s = ps.to_proj_string();
    assert!(s.contains("+zone=32"), "got: {s}");
}

#[test]
fn test_parse_error_on_empty() {
    let err = ProjString::parse("");
    assert!(err.is_err(), "empty string should fail");
}

#[test]
fn test_parse_error_on_no_plus_tokens() {
    // No '+' tokens
    let err = ProjString::parse("proj=utm zone=32");
    assert!(err.is_err(), "string without '+' tokens should fail");
}

#[test]
fn test_has_returns_true_for_existing() {
    let ps = ProjString::parse("+proj=utm +zone=32 +no_defs").expect("valid");
    assert!(ps.has("proj"));
    assert!(ps.has("zone"));
    assert!(ps.has("no_defs"));
}

#[test]
fn test_has_returns_false_for_missing() {
    let ps = ProjString::parse("+proj=utm +zone=32").expect("valid");
    assert!(!ps.has("south"));
    assert!(!ps.has("nonexistent"));
}

#[test]
fn test_get_returns_empty_str_for_flag() {
    let ps = ProjString::parse("+proj=utm +no_defs +zone=32").expect("valid");
    // Boolean flag should return Some("") not None
    assert_eq!(ps.get("no_defs"), Some(""));
}

#[test]
fn test_ellps_returns_value() {
    let ps = ProjString::parse("+proj=tmerc +ellps=bessel +no_defs").expect("valid");
    assert_eq!(ps.ellps(), Some("bessel"));
}

#[test]
fn test_parse_zone_various() {
    for zone in [1u8, 30, 60] {
        let ps = ProjString::utm(zone, false);
        assert_eq!(ps.zone(), Some(i32::from(zone)));
    }
}

#[test]
fn test_towgs84_wrong_count_returns_none() {
    // Only 3 values instead of 7
    let ps = ProjString::parse("+proj=tmerc +towgs84=1.0,2.0,3.0 +no_defs").expect("valid");
    assert!(ps.towgs84().is_none(), "3-param towgs84 should return None");
}
