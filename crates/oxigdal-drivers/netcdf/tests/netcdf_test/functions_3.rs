//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use oxigdal_netcdf::{
    Attribute, AttributeValue, Attributes, CfMetadata, DataType, Dimension,
    DimensionSize, Dimensions, NetCdfError, NetCdfMetadata, NetCdfReader, NetCdfVersion,
    NetCdfWriter, Variable, Variables,
};
use std::path::PathBuf;

#[cfg(feature = "netcdf3")]
mod writer_tests {
    use super::*;
    #[test]
    fn test_writer_create() {
        let path = temp_file_path("writer_create");
        let writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        assert!(writer.metadata().dimensions().is_empty());
        assert!(writer.metadata().variables().is_empty());
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_add_dimension() {
        let path = temp_file_path("writer_add_dim");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer
            .add_dimension(Dimension::new("x", 10).expect("Valid dimension"))
            .expect("Failed to add dimension");
        assert_eq!(writer.metadata().dimensions().len(), 1);
        assert!(writer.metadata().dimensions().contains("x"));
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_add_variable() {
        let path = temp_file_path("writer_add_var");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer
            .add_dimension(Dimension::new("x", 10).expect("Valid"))
            .expect("Failed to add dimension");
        writer
            .add_variable(
                Variable::new("data", DataType::F32, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed to add variable");
        assert_eq!(writer.metadata().variables().len(), 1);
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_add_variable_missing_dimension() {
        let path = temp_file_path("writer_missing_dim");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        let result = writer
            .add_variable(
                Variable::new("data", DataType::F32, vec!["missing".to_string()])
                    .expect("Valid"),
            );
        assert!(result.is_err());
        match result {
            Err(NetCdfError::DimensionNotFound { name }) => {
                assert_eq!(name, "missing");
            }
            _ => panic!("Expected DimensionNotFound error"),
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_add_global_attribute() {
        let path = temp_file_path("writer_global_attr");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer
            .add_global_attribute(
                Attribute::new("title", AttributeValue::text("Test")).expect("Valid"),
            )
            .expect("Failed to add global attribute");
        assert!(writer.metadata().global_attributes().contains("title"));
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_add_variable_attribute() {
        let path = temp_file_path("writer_var_attr");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer.add_dimension(Dimension::new("x", 10).expect("Valid")).expect("Failed");
        writer
            .add_variable(
                Variable::new("temp", DataType::F32, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        writer
            .add_variable_attribute(
                "temp",
                Attribute::new("units", AttributeValue::text("celsius")).expect("Valid"),
            )
            .expect("Failed to add variable attribute");
        let var = writer
            .metadata()
            .variables()
            .get("temp")
            .expect("Variable should exist");
        assert!(var.attributes().contains("units"));
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_add_variable_attribute_missing_var() {
        let path = temp_file_path("writer_missing_var");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        let result = writer
            .add_variable_attribute(
                "nonexistent",
                Attribute::new("units", AttributeValue::text("test")).expect("Valid"),
            );
        assert!(result.is_err());
        match result {
            Err(NetCdfError::VariableNotFound { name }) => {
                assert_eq!(name, "nonexistent");
            }
            _ => panic!("Expected VariableNotFound error"),
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_end_define_mode() {
        let path = temp_file_path("writer_end_define");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer.add_dimension(Dimension::new("x", 10).expect("Valid")).expect("Failed");
        writer
            .add_variable(
                Variable::new("data", DataType::F32, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        writer.end_define_mode().expect("Failed to end define mode");
        let result = writer.add_dimension(Dimension::new("y", 20).expect("Valid"));
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_write_before_end_define_fails() {
        let path = temp_file_path("writer_write_before_define");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer.add_dimension(Dimension::new("x", 10).expect("Valid")).expect("Failed");
        writer
            .add_variable(
                Variable::new("data", DataType::F32, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        let data = vec![0.0f32; 10];
        let result = writer.write_f32("data", &data);
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_write_f32() {
        let path = temp_file_path("writer_write_f32");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer.add_dimension(Dimension::new("x", 5).expect("Valid")).expect("Failed");
        writer
            .add_variable(
                Variable::new("data", DataType::F32, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        writer.end_define_mode().expect("Failed to end define mode");
        let data: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        writer.write_f32("data", &data).expect("Failed to write data");
        writer.close().expect("Failed to close");
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_write_f64() {
        let path = temp_file_path("writer_write_f64");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer.add_dimension(Dimension::new("x", 5).expect("Valid")).expect("Failed");
        writer
            .add_variable(
                Variable::new("data", DataType::F64, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        writer.end_define_mode().expect("Failed to end define mode");
        let data: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        writer.write_f64("data", &data).expect("Failed to write data");
        writer.close().expect("Failed to close");
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_write_i32() {
        let path = temp_file_path("writer_write_i32");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer.add_dimension(Dimension::new("x", 5).expect("Valid")).expect("Failed");
        writer
            .add_variable(
                Variable::new("data", DataType::I32, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        writer.end_define_mode().expect("Failed to end define mode");
        let data: Vec<i32> = vec![1, 2, 3, 4, 5];
        writer.write_i32("data", &data).expect("Failed to write data");
        writer.close().expect("Failed to close");
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_write_wrong_size() {
        let path = temp_file_path("writer_wrong_size");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer.add_dimension(Dimension::new("x", 10).expect("Valid")).expect("Failed");
        writer
            .add_variable(
                Variable::new("data", DataType::F32, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        writer.end_define_mode().expect("Failed to end define mode");
        let data: Vec<f32> = vec![1.0, 2.0, 3.0];
        let result = writer.write_f32("data", &data);
        assert!(result.is_err());
        match result {
            Err(NetCdfError::InvalidShape { .. }) => {}
            _ => panic!("Expected InvalidShape error"),
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_write_missing_variable() {
        let path = temp_file_path("writer_write_missing");
        let mut writer = NetCdfWriter::create(&path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer.add_dimension(Dimension::new("x", 10).expect("Valid")).expect("Failed");
        writer.end_define_mode().expect("Failed to end define mode");
        let data = vec![0.0f32; 10];
        let result = writer.write_f32("nonexistent", &data);
        assert!(result.is_err());
        match result {
            Err(NetCdfError::VariableNotFound { name }) => {
                assert_eq!(name, "nonexistent");
            }
            _ => panic!("Expected VariableNotFound error"),
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_writer_create_netcdf4_fails_without_feature() {
        let path = temp_file_path("writer_netcdf4");
        let result = NetCdfWriter::create(&path, NetCdfVersion::NetCdf4);
        #[cfg(not(feature = "netcdf4"))]
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }
}
#[cfg(feature = "netcdf3")]
mod reader_tests {
    use super::*;
    fn create_test_file(path: &PathBuf) {
        let mut writer = NetCdfWriter::create(path, NetCdfVersion::Classic)
            .expect("Failed to create writer");
        writer.add_dimension(Dimension::new("x", 10).expect("Valid")).expect("Failed");
        writer.add_dimension(Dimension::new("y", 20).expect("Valid")).expect("Failed");
        writer
            .add_variable(Variable::new_coordinate("x", DataType::F32).expect("Valid"))
            .expect("Failed");
        writer
            .add_variable(Variable::new_coordinate("y", DataType::F32).expect("Valid"))
            .expect("Failed");
        writer
            .add_variable(
                Variable::new(
                        "data",
                        DataType::F64,
                        vec!["x".to_string(), "y".to_string()],
                    )
                    .expect("Valid"),
            )
            .expect("Failed");
        writer
            .add_global_attribute(
                Attribute::new("title", AttributeValue::text("Test File"))
                    .expect("Valid"),
            )
            .expect("Failed");
        writer
            .add_variable_attribute(
                "data",
                Attribute::new("units", AttributeValue::text("meters")).expect("Valid"),
            )
            .expect("Failed");
        writer.end_define_mode().expect("Failed to end define mode");
        let x_data: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let y_data: Vec<f32> = (0..20).map(|i| i as f32 * 0.5).collect();
        let data: Vec<f64> = (0..200).map(|i| i as f64 * 0.1).collect();
        writer.write_f32("x", &x_data).expect("Failed to write x");
        writer.write_f32("y", &y_data).expect("Failed to write y");
        writer.write_f64("data", &data).expect("Failed to write data");
        writer.close().expect("Failed to close");
    }
    #[test]
    fn test_reader_open() {
        let path = temp_file_path("reader_open");
        create_test_file(&path);
        let reader = NetCdfReader::open(&path).expect("Failed to open file");
        assert_eq!(reader.version(), NetCdfVersion::Classic);
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_reader_dimensions() {
        let path = temp_file_path("reader_dims");
        create_test_file(&path);
        let reader = NetCdfReader::open(&path).expect("Failed to open file");
        let dims = reader.dimensions();
        assert_eq!(dims.len(), 2);
        assert!(dims.contains("x"));
        assert!(dims.contains("y"));
        let x_dim = dims.get("x").expect("x dimension should exist");
        assert_eq!(x_dim.len(), 10);
        let y_dim = dims.get("y").expect("y dimension should exist");
        assert_eq!(y_dim.len(), 20);
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_reader_variables() {
        let path = temp_file_path("reader_vars");
        create_test_file(&path);
        let reader = NetCdfReader::open(&path).expect("Failed to open file");
        let vars = reader.variables();
        assert_eq!(vars.len(), 3);
        assert!(vars.contains("x"));
        assert!(vars.contains("y"));
        assert!(vars.contains("data"));
        let data_var = vars.get("data").expect("data variable should exist");
        assert_eq!(data_var.data_type(), DataType::F64);
        assert_eq!(data_var.ndims(), 2);
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_reader_global_attributes() {
        let path = temp_file_path("reader_global_attrs");
        create_test_file(&path);
        let reader = NetCdfReader::open(&path).expect("Failed to open file");
        let attrs = reader.global_attributes();
        assert!(attrs.contains("title"));
        let title = attrs.get("title").expect("title should exist");
        assert_eq!(title.value().as_text().expect("Should be text"), "Test File");
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_reader_variable_attributes() {
        let path = temp_file_path("reader_var_attrs");
        create_test_file(&path);
        let reader = NetCdfReader::open(&path).expect("Failed to open file");
        let var = reader.variables().get("data").expect("data should exist");
        let attrs = var.attributes();
        assert!(attrs.contains("units"));
        let units = attrs.get("units").expect("units should exist");
        assert_eq!(units.value().as_text().expect("Should be text"), "meters");
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_reader_read_f32() {
        let path = temp_file_path("reader_read_f32");
        create_test_file(&path);
        let reader = NetCdfReader::open(&path).expect("Failed to open file");
        let data = reader.read_f32("x").expect("Failed to read x");
        assert_eq!(data.len(), 10);
        for (i, &val) in data.iter().enumerate() {
            assert!((val - i as f32).abs() < f32::EPSILON, "Mismatch at index {}", i);
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_reader_read_f64() {
        let path = temp_file_path("reader_read_f64");
        create_test_file(&path);
        let reader = NetCdfReader::open(&path).expect("Failed to open file");
        let data = reader.read_f64("data").expect("Failed to read data");
        assert_eq!(data.len(), 200);
        for (i, &val) in data.iter().enumerate() {
            let expected = i as f64 * 0.1;
            assert!((val - expected).abs() < 1e-10, "Mismatch at index {}", i);
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_reader_read_nonexistent_variable() {
        let path = temp_file_path("reader_nonexistent");
        create_test_file(&path);
        let reader = NetCdfReader::open(&path).expect("Failed to open file");
        let result = reader.read_f32("nonexistent");
        assert!(result.is_err());
        match result {
            Err(NetCdfError::VariableNotFound { name }) => {
                assert_eq!(name, "nonexistent");
            }
            _ => panic!("Expected VariableNotFound error"),
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_reader_metadata_summary() {
        let path = temp_file_path("reader_summary");
        create_test_file(&path);
        let reader = NetCdfReader::open(&path).expect("Failed to open file");
        let summary = reader.metadata().summary();
        assert!(summary.contains("NetCDF-3"));
        assert!(summary.contains("2 dimensions"));
        assert!(summary.contains("3 variables"));
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn test_reader_open_nonexistent_file() {
        let path = temp_file_path("nonexistent_file_that_does_not_exist");
        let result = NetCdfReader::open(&path);
        assert!(result.is_err());
    }
}
