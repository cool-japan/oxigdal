//! Integration tests for NetCDF driver.
//!
//! Tests reading, writing, and round-trip operations.

// Allow expect in tests - per project policy, test code can use expect() with descriptive messages
#![allow(clippy::expect_used)]

#[cfg(feature = "netcdf3")]
use oxigdal_netcdf::{
    Attribute, AttributeValue, DataType, Dimension, NetCdfReader, NetCdfVersion, NetCdfWriter,
    Variable,
};
#[cfg(feature = "netcdf3")]
use tempfile::NamedTempFile;

#[test]
#[cfg(feature = "netcdf3")]
fn test_create_simple_file() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let path = temp_file.path();

    let mut writer =
        NetCdfWriter::create(path, NetCdfVersion::Classic).expect("Failed to create NetCDF file");

    // Add dimension
    writer
        .add_dimension(Dimension::new("x", 10).expect("Valid dimension"))
        .expect("Failed to add dimension");

    // Add variable
    writer
        .add_variable(
            Variable::new("data", DataType::F32, vec!["x".to_string()]).expect("Valid variable"),
        )
        .expect("Failed to add variable");

    // Add global attribute
    writer
        .add_global_attribute(
            Attribute::new("title", AttributeValue::text("Test")).expect("Valid attribute"),
        )
        .expect("Failed to add global attribute");

    // End define mode
    writer.end_define_mode().expect("Failed to end define mode");

    // Write data
    let data: Vec<f32> = (0..10).map(|i| i as f32).collect();
    writer
        .write_f32("data", &data)
        .expect("Failed to write data");

    // Close file
    writer.close().expect("Failed to close file");

    // Read back
    let reader = NetCdfReader::open(path).expect("Failed to open NetCDF file");

    assert_eq!(reader.dimensions().len(), 1);
    assert_eq!(reader.variables().len(), 1);
    assert_eq!(reader.global_attributes().len(), 1);

    let dim = reader.dimensions().get("x").expect("Dimension not found");
    assert_eq!(dim.len(), 10);

    let var = reader.variables().get("data").expect("Variable not found");
    assert_eq!(var.data_type(), DataType::F32);

    let read_data = reader.read_f32("data").expect("Failed to read data");
    assert_eq!(read_data.len(), 10);
    for (i, &value) in read_data.iter().enumerate() {
        assert!((value - i as f32).abs() < f32::EPSILON);
    }
}

#[test]
#[cfg(feature = "netcdf3")]
fn test_multidimensional_array() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let path = temp_file.path();

    let mut writer =
        NetCdfWriter::create(path, NetCdfVersion::Classic).expect("Failed to create NetCDF file");

    // Add dimensions
    writer
        .add_dimension(Dimension::new("x", 10).expect("Valid dimension"))
        .expect("Failed to add dimension");
    writer
        .add_dimension(Dimension::new("y", 20).expect("Valid dimension"))
        .expect("Failed to add dimension");
    writer
        .add_dimension(Dimension::new("z", 30).expect("Valid dimension"))
        .expect("Failed to add dimension");

    // Add variable
    writer
        .add_variable(
            Variable::new(
                "data",
                DataType::F64,
                vec!["x".to_string(), "y".to_string(), "z".to_string()],
            )
            .expect("Valid variable"),
        )
        .expect("Failed to add variable");

    writer.end_define_mode().expect("Failed to end define mode");

    // Write data
    let size = 10 * 20 * 30;
    let data: Vec<f64> = (0..size).map(|i| i as f64).collect();
    writer
        .write_f64("data", &data)
        .expect("Failed to write data");

    writer.close().expect("Failed to close file");

    // Read back
    let reader = NetCdfReader::open(path).expect("Failed to open NetCDF file");

    let var = reader.variables().get("data").expect("Variable not found");
    assert_eq!(var.ndims(), 3);

    let shape = var.shape(reader.dimensions()).expect("Failed to get shape");
    assert_eq!(shape, vec![10, 20, 30]);

    let read_data = reader.read_f64("data").expect("Failed to read data");
    assert_eq!(read_data.len(), size);
}

#[test]
#[cfg(feature = "netcdf3")]
fn test_unlimited_dimension() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let path = temp_file.path();

    let mut writer =
        NetCdfWriter::create(path, NetCdfVersion::Classic).expect("Failed to create NetCDF file");

    // Add unlimited dimension
    writer
        .add_dimension(Dimension::new_unlimited("time", 0).expect("Valid dimension"))
        .expect("Failed to add dimension");
    writer
        .add_dimension(Dimension::new("station", 5).expect("Valid dimension"))
        .expect("Failed to add dimension");

    // Add coordinate variable
    writer
        .add_variable(Variable::new_coordinate("time", DataType::F64).expect("Valid variable"))
        .expect("Failed to add variable");

    // Add data variable
    writer
        .add_variable(
            Variable::new(
                "temperature",
                DataType::F32,
                vec!["time".to_string(), "station".to_string()],
            )
            .expect("Valid variable"),
        )
        .expect("Failed to add variable");

    writer.end_define_mode().expect("Failed to end define mode");

    // Note: NetCDF-3 unlimited dimension size is determined when data is written
    // For now, we just verify the structure was created correctly

    writer.close().expect("Failed to close file");

    // Read back
    let reader = NetCdfReader::open(path).expect("Failed to open NetCDF file");

    let time_dim = reader
        .dimensions()
        .get("time")
        .expect("Time dimension not found");
    assert!(time_dim.is_unlimited());

    let var = reader
        .variables()
        .get("temperature")
        .expect("Variable not found");
    assert_eq!(var.dimension_names(), &["time", "station"]);
}

#[test]
#[cfg(feature = "netcdf3")]
fn test_coordinate_variables() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let path = temp_file.path();

    let mut writer =
        NetCdfWriter::create(path, NetCdfVersion::Classic).expect("Failed to create NetCDF file");

    // Add dimensions
    writer
        .add_dimension(Dimension::new("lat", 180).expect("Valid dimension"))
        .expect("Failed to add dimension");
    writer
        .add_dimension(Dimension::new("lon", 360).expect("Valid dimension"))
        .expect("Failed to add dimension");

    // Add coordinate variables
    writer
        .add_variable(Variable::new_coordinate("lat", DataType::F32).expect("Valid variable"))
        .expect("Failed to add variable");
    writer
        .add_variable(Variable::new_coordinate("lon", DataType::F32).expect("Valid variable"))
        .expect("Failed to add variable");

    // Add attributes to coordinate variables
    writer
        .add_variable_attribute(
            "lat",
            Attribute::new("units", AttributeValue::text("degrees_north"))
                .expect("Valid attribute"),
        )
        .expect("Failed to add attribute");
    writer
        .add_variable_attribute(
            "lon",
            Attribute::new("units", AttributeValue::text("degrees_east")).expect("Valid attribute"),
        )
        .expect("Failed to add attribute");

    writer.end_define_mode().expect("Failed to end define mode");

    // Write coordinate data
    let lat_data: Vec<f32> = (0..180).map(|i| -90.0 + i as f32).collect();
    let lon_data: Vec<f32> = (0..360).map(|i| -180.0 + i as f32).collect();

    writer
        .write_f32("lat", &lat_data)
        .expect("Failed to write lat");
    writer
        .write_f32("lon", &lon_data)
        .expect("Failed to write lon");

    writer.close().expect("Failed to close file");

    // Read back
    let reader = NetCdfReader::open(path).expect("Failed to open NetCDF file");

    let lat_var = reader
        .variables()
        .get("lat")
        .expect("Lat variable not found");
    assert!(lat_var.is_coordinate());

    let lon_var = reader
        .variables()
        .get("lon")
        .expect("Lon variable not found");
    assert!(lon_var.is_coordinate());

    let lat_units = lat_var
        .attributes()
        .get("units")
        .expect("Units attribute not found");
    assert_eq!(
        lat_units.value().as_text().expect("Not text"),
        "degrees_north"
    );
}

#[test]
#[cfg(feature = "netcdf3")]
fn test_cf_conventions() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let path = temp_file.path();

    let mut writer =
        NetCdfWriter::create(path, NetCdfVersion::Classic).expect("Failed to create NetCDF file");

    // Add CF conventions global attributes
    writer
        .add_global_attribute(
            Attribute::new("Conventions", AttributeValue::text("CF-1.8")).expect("Valid attribute"),
        )
        .expect("Failed to add attribute");
    writer
        .add_global_attribute(
            Attribute::new("title", AttributeValue::text("Temperature Data"))
                .expect("Valid attribute"),
        )
        .expect("Failed to add attribute");
    writer
        .add_global_attribute(
            Attribute::new("institution", AttributeValue::text("Test Lab"))
                .expect("Valid attribute"),
        )
        .expect("Failed to add attribute");

    // Add dimension
    writer
        .add_dimension(Dimension::new("x", 10).expect("Valid dimension"))
        .expect("Failed to add dimension");

    // Add variable
    writer
        .add_variable(
            Variable::new("data", DataType::F32, vec!["x".to_string()]).expect("Valid variable"),
        )
        .expect("Failed to add variable");

    writer.end_define_mode().expect("Failed to end define mode");

    let data = vec![0.0f32; 10];
    writer
        .write_f32("data", &data)
        .expect("Failed to write data");

    writer.close().expect("Failed to close file");

    // Read back
    let reader = NetCdfReader::open(path).expect("Failed to open NetCDF file");

    let cf = reader.cf_metadata().expect("CF metadata not found");
    assert!(cf.is_cf_compliant());
    assert_eq!(cf.conventions.as_deref(), Some("CF-1.8"));
    assert_eq!(cf.title.as_deref(), Some("Temperature Data"));
    assert_eq!(cf.institution.as_deref(), Some("Test Lab"));
}

#[test]
#[cfg(feature = "netcdf3")]
fn test_variable_attributes() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let path = temp_file.path();

    let mut writer =
        NetCdfWriter::create(path, NetCdfVersion::Classic).expect("Failed to create NetCDF file");

    writer
        .add_dimension(Dimension::new("x", 10).expect("Valid dimension"))
        .expect("Failed to add dimension");

    writer
        .add_variable(
            Variable::new("temperature", DataType::F32, vec!["x".to_string()])
                .expect("Valid variable"),
        )
        .expect("Failed to add variable");

    // Add various attribute types
    writer
        .add_variable_attribute(
            "temperature",
            Attribute::new("units", AttributeValue::text("celsius")).expect("Valid attribute"),
        )
        .expect("Failed to add attribute");
    writer
        .add_variable_attribute(
            "temperature",
            Attribute::new("scale_factor", AttributeValue::f32(1.5)).expect("Valid attribute"),
        )
        .expect("Failed to add attribute");
    writer
        .add_variable_attribute(
            "temperature",
            Attribute::new("valid_range", AttributeValue::f32_array(vec![-50.0, 50.0]))
                .expect("Valid attribute"),
        )
        .expect("Failed to add attribute");

    writer.end_define_mode().expect("Failed to end define mode");

    let data = vec![20.0f32; 10];
    writer
        .write_f32("temperature", &data)
        .expect("Failed to write data");

    writer.close().expect("Failed to close file");

    // Read back
    let reader = NetCdfReader::open(path).expect("Failed to open NetCDF file");

    let var = reader
        .variables()
        .get("temperature")
        .expect("Variable not found");
    let attrs = var.attributes();

    assert_eq!(attrs.len(), 3);

    let units = attrs.get("units").expect("Units not found");
    assert_eq!(units.value().as_text().expect("Not text"), "celsius");

    let scale = attrs.get("scale_factor").expect("Scale factor not found");
    assert!((scale.value().as_f32().expect("Not f32") - 1.5).abs() < f32::EPSILON);
}

#[test]
#[cfg(feature = "netcdf3")]
fn test_integer_data_types() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let path = temp_file.path();

    let mut writer =
        NetCdfWriter::create(path, NetCdfVersion::Classic).expect("Failed to create NetCDF file");

    writer
        .add_dimension(Dimension::new("x", 10).expect("Valid dimension"))
        .expect("Failed to add dimension");

    writer
        .add_variable(
            Variable::new("int_data", DataType::I32, vec!["x".to_string()])
                .expect("Valid variable"),
        )
        .expect("Failed to add variable");

    writer.end_define_mode().expect("Failed to end define mode");

    let data: Vec<i32> = (0..10).collect();
    writer
        .write_i32("int_data", &data)
        .expect("Failed to write data");

    writer.close().expect("Failed to close file");

    // Read back
    let reader = NetCdfReader::open(path).expect("Failed to open NetCDF file");

    let read_data = reader.read_i32("int_data").expect("Failed to read data");
    assert_eq!(read_data, data);
}

#[test]
#[cfg(feature = "netcdf3")]
fn test_metadata_summary() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let path = temp_file.path();

    let mut writer =
        NetCdfWriter::create(path, NetCdfVersion::Classic).expect("Failed to create NetCDF file");

    writer
        .add_dimension(Dimension::new("x", 10).expect("Valid dimension"))
        .expect("Failed to add dimension");
    writer
        .add_variable(
            Variable::new("data", DataType::F32, vec!["x".to_string()]).expect("Valid variable"),
        )
        .expect("Failed to add variable");
    writer
        .add_global_attribute(
            Attribute::new("title", AttributeValue::text("Test")).expect("Valid attribute"),
        )
        .expect("Failed to add attribute");

    writer.end_define_mode().expect("Failed to end define mode");

    let data = vec![0.0f32; 10];
    writer
        .write_f32("data", &data)
        .expect("Failed to write data");

    writer.close().expect("Failed to close file");

    // Read back
    let reader = NetCdfReader::open(path).expect("Failed to open NetCDF file");

    let summary = reader.metadata().summary();
    assert!(summary.contains("NetCDF"));
    assert!(summary.contains("1 dimensions"));
    assert!(summary.contains("1 variables"));
    assert!(summary.contains("1 global attributes"));
}
