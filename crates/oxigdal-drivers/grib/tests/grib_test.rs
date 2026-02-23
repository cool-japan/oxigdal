//! Integration tests for oxigdal-grib

use oxigdal_grib::error::GribError;
use oxigdal_grib::grid::{GridDefinition, LatLonGrid, ScanMode};
use oxigdal_grib::message::GribEdition;
use oxigdal_grib::parameter::{LevelType, lookup_grib1_parameter, lookup_grib2_parameter};
use oxigdal_grib::reader::GribReader;
use std::io::Cursor;

#[test]
fn test_grib_edition() {
    assert_eq!(GribEdition::Grib1.number(), 1);
    assert_eq!(GribEdition::Grib2.number(), 2);

    assert_eq!(
        GribEdition::from_number(1).expect("GRIB edition 1 should be valid"),
        GribEdition::Grib1
    );
    assert_eq!(
        GribEdition::from_number(2).expect("GRIB edition 2 should be valid"),
        GribEdition::Grib2
    );

    assert!(matches!(
        GribEdition::from_number(3),
        Err(GribError::UnsupportedEdition(3))
    ));
}

#[test]
fn test_grib2_parameter_lookup() {
    // Temperature
    let temp = lookup_grib2_parameter(0, 0, 0).expect("Temperature parameter should be found");
    assert_eq!(temp.short_name, "TMP");
    assert_eq!(temp.long_name, "Temperature");
    assert_eq!(temp.units, "K");
    assert_eq!(temp.discipline, Some(0));
    assert_eq!(temp.category, 0);
    assert_eq!(temp.number, 0);

    // U-component of wind
    let u_wind =
        lookup_grib2_parameter(0, 2, 2).expect("U-component wind parameter should be found");
    assert_eq!(u_wind.short_name, "UGRD");
    assert_eq!(u_wind.units, "m/s");

    // V-component of wind
    let v_wind =
        lookup_grib2_parameter(0, 2, 3).expect("V-component wind parameter should be found");
    assert_eq!(v_wind.short_name, "VGRD");

    // Relative humidity
    let rh = lookup_grib2_parameter(0, 1, 1).expect("Relative humidity parameter should be found");
    assert_eq!(rh.short_name, "RH");
    assert_eq!(rh.units, "%");

    // Pressure
    let pres = lookup_grib2_parameter(0, 3, 0).expect("Pressure parameter should be found");
    assert_eq!(pres.short_name, "PRES");
    assert_eq!(pres.units, "Pa");

    // Invalid parameter
    assert!(lookup_grib2_parameter(0, 99, 99).is_err());
    assert!(lookup_grib2_parameter(99, 0, 0).is_err());
}

#[test]
fn test_grib1_parameter_lookup() {
    // Temperature (table version 3, parameter 11)
    let temp = lookup_grib1_parameter(3, 11).expect("GRIB1 temperature parameter should be found");
    assert_eq!(temp.short_name, "TMP");
    assert_eq!(temp.units, "K");

    // U-component of wind
    let u_wind =
        lookup_grib1_parameter(3, 33).expect("GRIB1 U-component wind parameter should be found");
    assert_eq!(u_wind.short_name, "UGRD");

    // V-component of wind
    let v_wind =
        lookup_grib1_parameter(3, 34).expect("GRIB1 V-component wind parameter should be found");
    assert_eq!(v_wind.short_name, "VGRD");

    // Relative humidity
    let rh =
        lookup_grib1_parameter(3, 52).expect("GRIB1 relative humidity parameter should be found");
    assert_eq!(rh.short_name, "RH");

    // Invalid parameter
    assert!(lookup_grib1_parameter(3, 255).is_err());
}

#[test]
fn test_level_types() {
    // GRIB2 level types
    assert_eq!(LevelType::from_grib2_code(1), LevelType::Surface);
    assert_eq!(LevelType::from_grib2_code(100), LevelType::Isobaric);
    assert_eq!(LevelType::from_grib2_code(101), LevelType::MeanSeaLevel);
    assert_eq!(
        LevelType::from_grib2_code(103),
        LevelType::HeightAboveGround
    );
    assert_eq!(LevelType::from_grib2_code(104), LevelType::Sigma);
    assert_eq!(LevelType::from_grib2_code(105), LevelType::Hybrid);

    // GRIB1 level types
    assert_eq!(LevelType::from_grib1_code(1), LevelType::Surface);
    assert_eq!(LevelType::from_grib1_code(100), LevelType::Isobaric);
    assert_eq!(LevelType::from_grib1_code(102), LevelType::MeanSeaLevel);
    assert_eq!(
        LevelType::from_grib1_code(105),
        LevelType::HeightAboveGround
    );

    // Level descriptions
    assert_eq!(LevelType::Surface.description(), "Surface");
    assert_eq!(
        LevelType::Isobaric.description(),
        "Isobaric (pressure level)"
    );
    assert_eq!(LevelType::MeanSeaLevel.description(), "Mean sea level");
}

#[test]
fn test_latlon_grid() {
    let grid = LatLonGrid {
        ni: 360,
        nj: 181,
        la1: 90.0,
        lo1: 0.0,
        la2: -90.0,
        lo2: 359.0,
        di: 1.0,
        dj: 1.0,
        scan_mode: ScanMode {
            i_positive: true,
            j_positive: false,
            consecutive_i: true,
        },
    };

    assert_eq!(grid.num_points(), 360 * 181);

    // Test latitude calculation
    let lat = grid
        .latitude(0)
        .expect("Latitude at index 0 should be valid");
    assert!((lat - 90.0).abs() < 1e-6);

    let lat = grid
        .latitude(90)
        .expect("Latitude at index 90 should be valid");
    assert!((lat - 0.0).abs() < 1.0);

    // Test longitude calculation
    let lon = grid
        .longitude(0)
        .expect("Longitude at index 0 should be valid");
    assert!((lon - 0.0).abs() < 1e-6);

    let lon = grid
        .longitude(180)
        .expect("Longitude at index 180 should be valid");
    assert!((lon - 180.0).abs() < 1.0);

    // Test coordinates
    let (lat, lon) = grid
        .coordinates(0, 0)
        .expect("Coordinates at (0,0) should be valid");
    assert!((lat - 90.0).abs() < 1e-6);
    assert!((lon - 0.0).abs() < 1e-6);

    // Test out of range
    assert!(grid.latitude(200).is_err());
    assert!(grid.longitude(400).is_err());
}

#[test]
fn test_grid_definition_enum() {
    let grid = GridDefinition::LatLon(LatLonGrid {
        ni: 720,
        nj: 361,
        la1: 90.0,
        lo1: 0.0,
        la2: -90.0,
        lo2: 359.5,
        di: 0.5,
        dj: 0.5,
        scan_mode: ScanMode::default(),
    });

    assert_eq!(grid.num_points(), 720 * 361);
    assert_eq!(grid.dimensions(), (720, 361));
    assert_eq!(grid.type_name(), "Regular Lat/Lon");
}

#[test]
fn test_scan_mode() {
    // Default scan mode: +i, -j, consecutive i
    let mode = ScanMode::default();
    assert!(mode.i_positive);
    assert!(!mode.j_positive);
    assert!(mode.consecutive_i);

    let flags = mode.to_flags();
    assert_eq!(flags, 0b0000_0000);

    // Test round-trip conversion
    let mode2 = ScanMode::from_flags(flags);
    assert_eq!(mode, mode2);

    // Test different combinations
    let flags = 0b0100_0000; // +i, +j, consecutive i
    let mode = ScanMode::from_flags(flags);
    assert!(mode.i_positive);
    assert!(mode.j_positive);
    assert!(mode.consecutive_i);

    let flags = 0b1000_0000; // -i, -j, consecutive i
    let mode = ScanMode::from_flags(flags);
    assert!(!mode.i_positive);
    assert!(!mode.j_positive);
    assert!(mode.consecutive_i);
}

#[test]
fn test_grib_reader_empty_file() {
    let data: &[u8] = &[];
    let mut reader = GribReader::new(Cursor::new(data));

    let result = reader.next_message();
    assert!(result.is_ok());
    assert!(
        result
            .expect("Reading from empty file should succeed")
            .is_none()
    );

    assert_eq!(reader.message_count(), 0);
}

#[test]
fn test_grib_reader_invalid_header() {
    let data = b"NOT A GRIB FILE";
    let mut reader = GribReader::new(Cursor::new(data));

    let result = reader.next_message();
    let err = result.expect_err("Expected an error");

    assert!(
        matches!(err, GribError::InvalidHeader(_)),
        "Expected InvalidHeader error, got: {:?}",
        err
    );

    if let GribError::InvalidHeader(header) = err {
        assert_eq!(&header, b"NOT ");
    }
}

#[test]
fn test_error_types() {
    // Test error display
    let err = GribError::InvalidHeader(vec![0x47, 0x52, 0x49, 0x41]);
    assert!(err.to_string().contains("GRIB"));

    let err = GribError::UnsupportedEdition(3);
    assert!(err.to_string().contains("GRIB1 and GRIB2"));

    let err = GribError::InvalidParameter {
        discipline: 0,
        category: 1,
        number: 255,
    };
    assert!(err.to_string().contains("discipline=0"));
    assert!(err.to_string().contains("category=1"));

    // Test error constructors
    let err = GribError::parse("test parse error");
    assert!(matches!(err, GribError::ParseError(_)));

    let err = GribError::decode("test decode error");
    assert!(matches!(err, GribError::DecodingError(_)));

    let err = GribError::not_impl("complex packing");
    assert!(matches!(err, GribError::NotImplemented(_)));
}

#[test]
fn test_crate_constants() {
    use oxigdal_grib::{VERSION, has_grib1_support, has_grib2_support};

    // Version should not be empty
    assert!(!VERSION.is_empty());

    // Both GRIB1 and GRIB2 should be enabled by default
    assert!(has_grib1_support());
    assert!(has_grib2_support());
}

// Test with temporary GRIB file
#[test]
fn test_grib_file_operations() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Create a temporary file
    let mut file = NamedTempFile::new().expect("Failed to create temp file");

    // Write some test data (not a valid GRIB file)
    file.write_all(b"TEST DATA")
        .expect("Failed to write to temp file");
    file.flush().expect("Failed to flush temp file");

    // Try to open as GRIB file
    let path = file.path();
    let result = GribReader::open(path);

    // Should succeed in opening, but fail on reading first message
    assert!(result.is_ok());

    let mut reader = result.expect("GRIB reader should be created from file");
    let result = reader.next_message();
    assert!(result.is_err());
}
