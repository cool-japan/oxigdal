//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use oxigdal_netcdf::{
    Attribute, AttributeValue, Attributes, CfMetadata, DataType, Dimension,
    DimensionSize, Dimensions, NetCdfError, NetCdfMetadata, NetCdfReader, NetCdfVersion,
    NetCdfWriter, Variable, Variables,
};

#[cfg(feature = "netcdf3")]
mod roundtrip_tests {
    use super::*;
    #[test]
    fn test_roundtrip_simple() {
        let path = temp_file_path("roundtrip_simple");
        {
            let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
                .expect("Failed to create");
            writer
                .add_dimension(Dimension::new("x", 5).expect("Valid"))
                .expect("Failed");
            writer
                .add_variable(
                    Variable::new("values", DataType::F32, vec!["x".to_string()])
                        .expect("Valid"),
                )
                .expect("Failed");
            writer.end_define_mode().expect("Failed");
            let data: Vec<f32> = vec![1.1, 2.2, 3.3, 4.4, 5.5];
            writer.write_f32("values", &data).expect("Failed to write");
            writer.close().expect("Failed to close");
        }
        {
            let reader = NetCdfReader::open(&path).expect("Failed to open");
            let data = reader.read_f32("values").expect("Failed to read");
            assert_eq!(data.len(), 5);
            let expected: Vec<f32> = vec![1.1, 2.2, 3.3, 4.4, 5.5];
            for (i, (&got, &exp)) in data.iter().zip(expected.iter()).enumerate() {
                assert!(
                    (got - exp).abs() < 1e-6,
                    "Mismatch at index {}: got {}, expected {}", i, got, exp
                );
            }
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_roundtrip_multidimensional() {
        let path = temp_file_path("roundtrip_multi");
        {
            let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
                .expect("Failed to create");
            writer
                .add_dimension(Dimension::new("x", 3).expect("Valid"))
                .expect("Failed");
            writer
                .add_dimension(Dimension::new("y", 4).expect("Valid"))
                .expect("Failed");
            writer
                .add_variable(
                    Variable::new(
                            "matrix",
                            DataType::F64,
                            vec!["x".to_string(), "y".to_string()],
                        )
                        .expect("Valid"),
                )
                .expect("Failed");
            writer.end_define_mode().expect("Failed");
            let data: Vec<f64> = (0..12).map(|i| i as f64 * 1.5).collect();
            writer.write_f64("matrix", &data).expect("Failed to write");
            writer.close().expect("Failed to close");
        }
        {
            let reader = NetCdfReader::open(&path).expect("Failed to open");
            let var = reader.variables().get("matrix").expect("Variable should exist");
            let shape = var.shape(reader.dimensions()).expect("Failed to get shape");
            assert_eq!(shape, vec![3, 4]);
            let data = reader.read_f64("matrix").expect("Failed to read");
            assert_eq!(data.len(), 12);
            for i in 0..12 {
                let expected = i as f64 * 1.5;
                assert!((data[i] - expected).abs() < 1e-10, "Mismatch at index {}", i);
            }
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_roundtrip_with_attributes() {
        let path = temp_file_path("roundtrip_attrs");
        {
            let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
                .expect("Failed to create");
            writer
                .add_global_attribute(
                    Attribute::new("Conventions", AttributeValue::text("CF-1.8"))
                        .expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_global_attribute(
                    Attribute::new("title", AttributeValue::text("Test Dataset"))
                        .expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_global_attribute(
                    Attribute::new("version", AttributeValue::i32(1)).expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_dimension(Dimension::new("x", 5).expect("Valid"))
                .expect("Failed");
            writer
                .add_variable(
                    Variable::new("data", DataType::F32, vec!["x".to_string()])
                        .expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_variable_attribute(
                    "data",
                    Attribute::new("units", AttributeValue::text("meters"))
                        .expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_variable_attribute(
                    "data",
                    Attribute::new("long_name", AttributeValue::text("Distance"))
                        .expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_variable_attribute(
                    "data",
                    Attribute::new("scale_factor", AttributeValue::f64(1.5))
                        .expect("Valid"),
                )
                .expect("Failed");
            writer.end_define_mode().expect("Failed");
            let data = vec![1.0f32; 5];
            writer.write_f32("data", &data).expect("Failed to write");
            writer.close().expect("Failed to close");
        }
        {
            let reader = NetCdfReader::open(&path).expect("Failed to open");
            let global_attrs = reader.global_attributes();
            assert!(global_attrs.contains("Conventions"));
            assert!(global_attrs.contains("title"));
            assert!(global_attrs.contains("version"));
            let conventions = global_attrs.get("Conventions").expect("Should exist");
            assert_eq!(conventions.value().as_text().expect("Should be text"), "CF-1.8");
            let cf = reader.cf_metadata().expect("CF metadata should exist");
            assert!(cf.is_cf_compliant());
            assert_eq!(cf.title.as_deref(), Some("Test Dataset"));
            let var = reader.variables().get("data").expect("Should exist");
            let var_attrs = var.attributes();
            assert!(var_attrs.contains("units"));
            assert!(var_attrs.contains("long_name"));
            assert!(var_attrs.contains("scale_factor"));
            let units = var_attrs.get("units").expect("Should exist");
            assert_eq!(units.value().as_text().expect("Should be text"), "meters");
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_roundtrip_coordinate_variables() {
        let path = temp_file_path("roundtrip_coords");
        {
            let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
                .expect("Failed to create");
            writer
                .add_dimension(Dimension::new("lat", 3).expect("Valid"))
                .expect("Failed");
            writer
                .add_dimension(Dimension::new("lon", 4).expect("Valid"))
                .expect("Failed");
            writer
                .add_variable(
                    Variable::new_coordinate("lat", DataType::F64).expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_variable(
                    Variable::new_coordinate("lon", DataType::F64).expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_variable(
                    Variable::new(
                            "temp",
                            DataType::F32,
                            vec!["lat".to_string(), "lon".to_string()],
                        )
                        .expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_variable_attribute(
                    "lat",
                    Attribute::new("units", AttributeValue::text("degrees_north"))
                        .expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_variable_attribute(
                    "lon",
                    Attribute::new("units", AttributeValue::text("degrees_east"))
                        .expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_variable_attribute(
                    "temp",
                    Attribute::new("units", AttributeValue::text("K")).expect("Valid"),
                )
                .expect("Failed");
            writer.end_define_mode().expect("Failed");
            let lat_data = vec![30.0, 40.0, 50.0];
            let lon_data = vec![- 120.0, - 110.0, - 100.0, - 90.0];
            let temp_data = vec![280.0f32; 12];
            writer.write_f64("lat", &lat_data).expect("Failed to write lat");
            writer.write_f64("lon", &lon_data).expect("Failed to write lon");
            writer.write_f32("temp", &temp_data).expect("Failed to write temp");
            writer.close().expect("Failed to close");
        }
        {
            let reader = NetCdfReader::open(&path).expect("Failed to open");
            let lat_var = reader.variables().get("lat").expect("Should exist");
            assert!(lat_var.is_coordinate());
            let lon_var = reader.variables().get("lon").expect("Should exist");
            assert!(lon_var.is_coordinate());
            let temp_var = reader.variables().get("temp").expect("Should exist");
            assert!(! temp_var.is_coordinate());
            let lat_data = reader.read_f64("lat").expect("Failed to read lat");
            assert_eq!(lat_data, vec![30.0, 40.0, 50.0]);
            let lon_data = reader.read_f64("lon").expect("Failed to read lon");
            assert_eq!(lon_data, vec![- 120.0, - 110.0, - 100.0, - 90.0]);
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_roundtrip_integer_data() {
        let path = temp_file_path("roundtrip_int");
        {
            let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
                .expect("Failed to create");
            writer
                .add_dimension(Dimension::new("x", 10).expect("Valid"))
                .expect("Failed");
            writer
                .add_variable(
                    Variable::new("counts", DataType::I32, vec!["x".to_string()])
                        .expect("Valid"),
                )
                .expect("Failed");
            writer.end_define_mode().expect("Failed");
            let data: Vec<i32> = (0..10).map(|i| i * 100 - 500).collect();
            writer.write_i32("counts", &data).expect("Failed to write");
            writer.close().expect("Failed to close");
        }
        {
            let reader = NetCdfReader::open(&path).expect("Failed to open");
            let data = reader.read_i32("counts").expect("Failed to read");
            assert_eq!(data.len(), 10);
            let expected: Vec<i32> = (0..10).map(|i| i * 100 - 500).collect();
            assert_eq!(data, expected);
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_roundtrip_unlimited_dimension() {
        let path = temp_file_path("roundtrip_unlimited");
        {
            let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
                .expect("Failed to create");
            writer
                .add_dimension(Dimension::new_unlimited("time", 0).expect("Valid"))
                .expect("Failed");
            writer
                .add_dimension(Dimension::new("x", 5).expect("Valid"))
                .expect("Failed");
            writer
                .add_variable(
                    Variable::new_coordinate("time", DataType::F64).expect("Valid"),
                )
                .expect("Failed");
            writer
                .add_variable(
                    Variable::new(
                            "data",
                            DataType::F32,
                            vec!["time".to_string(), "x".to_string()],
                        )
                        .expect("Valid"),
                )
                .expect("Failed");
            writer.end_define_mode().expect("Failed");
            writer.close().expect("Failed to close");
        }
        {
            let reader = NetCdfReader::open(&path).expect("Failed to open");
            let time_dim = reader.dimensions().get("time").expect("Should exist");
            assert!(time_dim.is_unlimited());
        }
        let _ = std::fs::remove_file(&path);
    }
}
mod error_tests {
    use super::*;
    #[test]
    fn test_error_display_dimension_not_found() {
        let err = NetCdfError::DimensionNotFound {
            name: "missing_dim".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("missing_dim"), "Error message: {}", msg);
        assert!(msg.to_lowercase().contains("not found"), "Error message: {}", msg);
    }
    #[test]
    fn test_error_display_variable_not_found() {
        let err = NetCdfError::VariableNotFound {
            name: "missing_var".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("missing_var"), "Error message: {}", msg);
    }
    #[test]
    fn test_error_display_attribute_not_found() {
        let err = NetCdfError::AttributeNotFound {
            name: "missing_attr".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("missing_attr"), "Error message: {}", msg);
    }
    #[test]
    fn test_error_display_data_type_mismatch() {
        let err = NetCdfError::DataTypeMismatch {
            expected: "f32".to_string(),
            found: "i32".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("f32"), "Error message: {}", msg);
        assert!(msg.contains("i32"), "Error message: {}", msg);
    }
    #[test]
    fn test_error_display_invalid_shape() {
        let err = NetCdfError::InvalidShape {
            message: "Data size mismatch".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("shape"), "Error message: {}", msg);
    }
    #[test]
    fn test_error_display_index_out_of_bounds() {
        let err = NetCdfError::IndexOutOfBounds {
            index: 100,
            length: 50,
            dimension: "x".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("100"), "Error message: {}", msg);
        assert!(msg.contains("50"), "Error message: {}", msg);
        assert!(msg.contains("x"), "Error message: {}", msg);
    }
    #[test]
    fn test_error_display_netcdf4_not_available() {
        let err = NetCdfError::NetCdf4NotAvailable;
        let msg = err.to_string();
        assert!(msg.contains("NetCDF-4"), "Error message: {}", msg);
        assert!(msg.contains("netcdf4"), "Error message: {}", msg);
    }
    #[test]
    fn test_error_display_feature_not_enabled() {
        let err = NetCdfError::FeatureNotEnabled {
            feature: "compression".to_string(),
            message: "Enable feature to use".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("compression"), "Error message: {}", msg);
    }
    #[test]
    fn test_error_display_unlimited_dimension() {
        let err = NetCdfError::UnlimitedDimensionError(
            "Cannot change fixed dimension".to_string(),
        );
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("unlimited") || msg.contains("fixed"),
            "Error message: {}", msg
        );
    }
    #[test]
    fn test_error_display_cf_conventions() {
        let err = NetCdfError::CfConventionsError("Invalid CF metadata".to_string());
        let msg = err.to_string();
        assert!(msg.contains("CF"), "Error message: {}", msg);
    }
    #[test]
    fn test_error_display_io() {
        let err = NetCdfError::Io("File not found".to_string());
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("i/o") || msg.contains("I/O"),
            "Error message: {}", msg
        );
    }
}
