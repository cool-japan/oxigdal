//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use oxigdal_netcdf::{
    Attribute, AttributeValue, Attributes, CfMetadata, DataType, Dimension,
    DimensionSize, Dimensions, NetCdfError, NetCdfMetadata, NetCdfReader, NetCdfVersion,
    NetCdfWriter, Variable, Variables,
};
use std::path::PathBuf;

/// Create a temporary file path in the system temp directory.
fn temp_file_path(name: &str) -> PathBuf {
    let mut path = env::temp_dir();
    path.push(format!("oxigdal_netcdf_test_{}_{}.nc", name, std::process::id()));
    path
}
mod dimension_tests {
    use super::*;
    #[test]
    fn test_dimension_new_fixed() {
        let dim = Dimension::new("test_dim", 100).expect("Failed to create dimension");
        assert_eq!(dim.name(), "test_dim");
        assert_eq!(dim.len(), 100);
        assert!(! dim.is_unlimited());
        assert!(! dim.is_empty());
    }
    #[test]
    fn test_dimension_new_unlimited() {
        let dim = Dimension::new_unlimited("time", 50)
            .expect("Failed to create unlimited dimension");
        assert_eq!(dim.name(), "time");
        assert_eq!(dim.len(), 50);
        assert!(dim.is_unlimited());
    }
    #[test]
    fn test_dimension_empty_name_fails() {
        let result = Dimension::new("", 100);
        assert!(result.is_err());
        match result {
            Err(NetCdfError::DimensionError(msg)) => {
                assert!(
                    msg.contains("empty"), "Error message should mention empty: {}", msg
                );
            }
            _ => panic!("Expected DimensionError"),
        }
    }
    #[test]
    fn test_dimension_zero_size() {
        let dim = Dimension::new("empty_dim", 0).expect("Should allow zero size");
        assert!(dim.is_empty());
        assert_eq!(dim.len(), 0);
    }
    #[test]
    fn test_dimension_set_len_unlimited() {
        let mut dim = Dimension::new_unlimited("time", 10)
            .expect("Failed to create dimension");
        dim.set_len(100).expect("Should allow changing unlimited dimension size");
        assert_eq!(dim.len(), 100);
    }
    #[test]
    fn test_dimension_set_len_fixed_fails() {
        let mut dim = Dimension::new("fixed", 10).expect("Failed to create dimension");
        let result = dim.set_len(100);
        assert!(result.is_err());
        match result {
            Err(NetCdfError::UnlimitedDimensionError(msg)) => {
                assert!(
                    msg.contains("fixed"), "Error should mention fixed dimension: {}",
                    msg
                );
            }
            _ => panic!("Expected UnlimitedDimensionError"),
        }
    }
    #[test]
    fn test_dimension_size_enum_fixed() {
        let size = DimensionSize::Fixed(100);
        assert_eq!(size.len(), 100);
        assert!(! size.is_unlimited());
        assert!(! size.is_empty());
    }
    #[test]
    fn test_dimension_size_enum_unlimited() {
        let size = DimensionSize::Unlimited(50);
        assert_eq!(size.len(), 50);
        assert!(size.is_unlimited());
    }
    #[test]
    fn test_dimensions_collection_basic() {
        let mut dims = Dimensions::new();
        assert!(dims.is_empty());
        assert_eq!(dims.len(), 0);
        dims.add(Dimension::new("x", 10).expect("Valid dimension"))
            .expect("Failed to add dimension");
        dims.add(Dimension::new("y", 20).expect("Valid dimension"))
            .expect("Failed to add dimension");
        dims.add(Dimension::new("z", 30).expect("Valid dimension"))
            .expect("Failed to add dimension");
        assert_eq!(dims.len(), 3);
        assert!(! dims.is_empty());
        assert!(dims.contains("x"));
        assert!(dims.contains("y"));
        assert!(dims.contains("z"));
        assert!(! dims.contains("w"));
    }
    #[test]
    fn test_dimensions_get_by_name() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("test", 100).expect("Valid dimension"))
            .expect("Failed to add dimension");
        let dim = dims.get("test").expect("Dimension should exist");
        assert_eq!(dim.len(), 100);
        assert!(dims.get("nonexistent").is_none());
    }
    #[test]
    fn test_dimensions_get_by_index() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("first", 10).expect("Valid dimension"))
            .expect("Failed to add dimension");
        dims.add(Dimension::new("second", 20).expect("Valid dimension"))
            .expect("Failed to add dimension");
        let first = dims.get_by_index(0).expect("Index 0 should exist");
        assert_eq!(first.name(), "first");
        let second = dims.get_by_index(1).expect("Index 1 should exist");
        assert_eq!(second.name(), "second");
        assert!(dims.get_by_index(2).is_none());
    }
    #[test]
    fn test_dimensions_duplicate_fails() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("test", 10).expect("Valid dimension"))
            .expect("First add should succeed");
        let result = dims.add(Dimension::new("test", 20).expect("Valid dimension"));
        assert!(result.is_err());
    }
    #[test]
    fn test_dimensions_total_size() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("x", 10).expect("Valid dimension"))
            .expect("Failed to add");
        dims.add(Dimension::new("y", 20).expect("Valid dimension"))
            .expect("Failed to add");
        dims.add(Dimension::new("z", 5).expect("Valid dimension"))
            .expect("Failed to add");
        assert_eq!(dims.total_size(), Some(10 * 20 * 5));
    }
    #[test]
    fn test_dimensions_shape() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("x", 10).expect("Valid dimension"))
            .expect("Failed to add");
        dims.add(Dimension::new("y", 20).expect("Valid dimension"))
            .expect("Failed to add");
        dims.add(Dimension::new("z", 30).expect("Valid dimension"))
            .expect("Failed to add");
        assert_eq!(dims.shape(), vec![10, 20, 30]);
    }
    #[test]
    fn test_dimensions_names() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("lat", 180).expect("Valid dimension"))
            .expect("Failed to add");
        dims.add(Dimension::new("lon", 360).expect("Valid dimension"))
            .expect("Failed to add");
        let names = dims.names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(& "lat"));
        assert!(names.contains(& "lon"));
    }
    #[test]
    fn test_dimensions_unlimited() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("x", 10).expect("Valid dimension"))
            .expect("Failed to add");
        dims.add(Dimension::new_unlimited("time", 0).expect("Valid dimension"))
            .expect("Failed to add");
        dims.add(Dimension::new("y", 20).expect("Valid dimension"))
            .expect("Failed to add");
        let unlimited = dims.unlimited().expect("Should have unlimited dimension");
        assert_eq!(unlimited.name(), "time");
        assert!(unlimited.is_unlimited());
    }
    #[test]
    fn test_dimensions_iterator() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("a", 1).expect("Valid")).expect("Failed");
        dims.add(Dimension::new("b", 2).expect("Valid")).expect("Failed");
        dims.add(Dimension::new("c", 3).expect("Valid")).expect("Failed");
        let names: Vec<&str> = dims.iter().map(|d| d.name()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }
    #[test]
    fn test_dimensions_from_iterator() {
        let dim_vec = vec![
            Dimension::new("x", 10).expect("Valid"), Dimension::new("y", 20)
            .expect("Valid"),
        ];
        let dims: Dimensions = dim_vec.into_iter().collect();
        assert_eq!(dims.len(), 2);
    }
}
mod variable_tests {
    use super::*;
    #[test]
    fn test_data_type_size() {
        assert_eq!(DataType::I8.size(), 1);
        assert_eq!(DataType::U8.size(), 1);
        assert_eq!(DataType::I16.size(), 2);
        assert_eq!(DataType::U16.size(), 2);
        assert_eq!(DataType::I32.size(), 4);
        assert_eq!(DataType::U32.size(), 4);
        assert_eq!(DataType::I64.size(), 8);
        assert_eq!(DataType::U64.size(), 8);
        assert_eq!(DataType::F32.size(), 4);
        assert_eq!(DataType::F64.size(), 8);
        assert_eq!(DataType::Char.size(), 1);
        assert_eq!(DataType::String.size(), 0);
    }
    #[test]
    fn test_data_type_name() {
        assert_eq!(DataType::I8.name(), "i8");
        assert_eq!(DataType::F32.name(), "f32");
        assert_eq!(DataType::F64.name(), "f64");
        assert_eq!(DataType::Char.name(), "char");
        assert_eq!(DataType::String.name(), "string");
    }
    #[test]
    fn test_data_type_is_float() {
        assert!(DataType::F32.is_float());
        assert!(DataType::F64.is_float());
        assert!(! DataType::I32.is_float());
        assert!(! DataType::Char.is_float());
    }
    #[test]
    fn test_data_type_is_integer() {
        assert!(DataType::I8.is_integer());
        assert!(DataType::I16.is_integer());
        assert!(DataType::I32.is_integer());
        assert!(DataType::I64.is_integer());
        assert!(DataType::U8.is_integer());
        assert!(DataType::U16.is_integer());
        assert!(DataType::U32.is_integer());
        assert!(DataType::U64.is_integer());
        assert!(! DataType::F32.is_integer());
        assert!(! DataType::F64.is_integer());
    }
    #[test]
    fn test_data_type_is_signed() {
        assert!(DataType::I8.is_signed());
        assert!(DataType::I16.is_signed());
        assert!(DataType::I32.is_signed());
        assert!(DataType::I64.is_signed());
        assert!(DataType::F32.is_signed());
        assert!(DataType::F64.is_signed());
        assert!(! DataType::U8.is_signed());
        assert!(! DataType::U16.is_signed());
        assert!(! DataType::U32.is_signed());
        assert!(! DataType::U64.is_signed());
    }
    #[test]
    fn test_data_type_netcdf3_compatible() {
        assert!(DataType::I8.is_netcdf3_compatible());
        assert!(DataType::I16.is_netcdf3_compatible());
        assert!(DataType::I32.is_netcdf3_compatible());
        assert!(DataType::F32.is_netcdf3_compatible());
        assert!(DataType::F64.is_netcdf3_compatible());
        assert!(DataType::Char.is_netcdf3_compatible());
        assert!(! DataType::U16.is_netcdf3_compatible());
        assert!(! DataType::U32.is_netcdf3_compatible());
        assert!(! DataType::I64.is_netcdf3_compatible());
        assert!(! DataType::U64.is_netcdf3_compatible());
        assert!(! DataType::String.is_netcdf3_compatible());
    }
    #[test]
    fn test_variable_new() {
        let var = Variable::new(
                "temperature",
                DataType::F32,
                vec!["time".to_string(), "lat".to_string(), "lon".to_string()],
            )
            .expect("Failed to create variable");
        assert_eq!(var.name(), "temperature");
        assert_eq!(var.data_type(), DataType::F32);
        assert_eq!(var.ndims(), 3);
        assert_eq!(
            var.dimension_names(), & ["time".to_string(), "lat".to_string(), "lon"
            .to_string()]
        );
        assert!(! var.is_scalar());
        assert!(! var.is_coordinate());
    }
    #[test]
    fn test_variable_new_coordinate() {
        let var = Variable::new_coordinate("time", DataType::F64)
            .expect("Failed to create coordinate variable");
        assert_eq!(var.name(), "time");
        assert_eq!(var.data_type(), DataType::F64);
        assert_eq!(var.ndims(), 1);
        assert_eq!(var.dimension_names(), & ["time".to_string()]);
        assert!(var.is_coordinate());
        assert!(! var.is_scalar());
    }
    #[test]
    fn test_variable_scalar() {
        let var = Variable::new("global_mean", DataType::F64, vec![])
            .expect("Failed to create scalar");
        assert!(var.is_scalar());
        assert_eq!(var.ndims(), 0);
        assert!(var.dimension_names().is_empty());
    }
    #[test]
    fn test_variable_empty_name_fails() {
        let result = Variable::new("", DataType::F32, vec!["x".to_string()]);
        assert!(result.is_err());
        match result {
            Err(NetCdfError::VariableError(msg)) => {
                assert!(msg.contains("empty"), "Error should mention empty: {}", msg);
            }
            _ => panic!("Expected VariableError"),
        }
    }
    #[test]
    fn test_variable_shape() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("time", 10).expect("Valid")).expect("Failed");
        dims.add(Dimension::new("lat", 180).expect("Valid")).expect("Failed");
        dims.add(Dimension::new("lon", 360).expect("Valid")).expect("Failed");
        let var = Variable::new(
                "temp",
                DataType::F32,
                vec!["time".to_string(), "lat".to_string(), "lon".to_string()],
            )
            .expect("Failed to create variable");
        let shape = var.shape(&dims).expect("Failed to get shape");
        assert_eq!(shape, vec![10, 180, 360]);
    }
    #[test]
    fn test_variable_size() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("x", 10).expect("Valid")).expect("Failed");
        dims.add(Dimension::new("y", 20).expect("Valid")).expect("Failed");
        let var = Variable::new(
                "data",
                DataType::F32,
                vec!["x".to_string(), "y".to_string()],
            )
            .expect("Failed to create variable");
        let size = var.size(&dims).expect("Failed to get size");
        assert_eq!(size, 10 * 20);
    }
    #[test]
    fn test_variable_size_bytes() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("x", 100).expect("Valid")).expect("Failed");
        let var = Variable::new("data", DataType::F64, vec!["x".to_string()])
            .expect("Failed to create variable");
        let size_bytes = var.size_bytes(&dims).expect("Failed to get size bytes");
        assert_eq!(size_bytes, 100 * 8);
    }
    #[test]
    fn test_variable_scalar_size() {
        let dims = Dimensions::new();
        let var = Variable::new("scalar", DataType::F32, vec![])
            .expect("Failed to create scalar");
        let size = var.size(&dims).expect("Failed to get size");
        assert_eq!(size, 1);
    }
    #[test]
    fn test_variable_missing_dimension() {
        let dims = Dimensions::new();
        let var = Variable::new("data", DataType::F32, vec!["missing".to_string()])
            .expect("Failed to create variable");
        let result = var.shape(&dims);
        assert!(result.is_err());
        match result {
            Err(NetCdfError::DimensionNotFound { name }) => {
                assert_eq!(name, "missing");
            }
            _ => panic!("Expected DimensionNotFound"),
        }
    }
    #[test]
    fn test_variable_set_coordinate() {
        let mut var = Variable::new("data", DataType::F32, vec!["data".to_string()])
            .expect("Failed to create variable");
        assert!(! var.is_coordinate());
        var.set_coordinate(true);
        assert!(var.is_coordinate());
        var.set_coordinate(false);
        assert!(! var.is_coordinate());
    }
    #[test]
    fn test_variables_collection() {
        let mut vars = Variables::new();
        assert!(vars.is_empty());
        vars.add(Variable::new_coordinate("time", DataType::F64).expect("Valid"))
            .expect("Failed");
        vars.add(
                Variable::new("temp", DataType::F32, vec!["time".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        assert_eq!(vars.len(), 2);
        assert!(vars.contains("time"));
        assert!(vars.contains("temp"));
        assert!(! vars.contains("other"));
    }
    #[test]
    fn test_variables_coordinates_filter() {
        let mut vars = Variables::new();
        vars.add(Variable::new_coordinate("time", DataType::F64).expect("Valid"))
            .expect("Failed");
        vars.add(Variable::new_coordinate("lat", DataType::F32).expect("Valid"))
            .expect("Failed");
        vars.add(
                Variable::new("temp", DataType::F32, vec!["time".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        let coords: Vec<_> = vars.coordinates().collect();
        assert_eq!(coords.len(), 2);
        let data_vars: Vec<_> = vars.data_variables().collect();
        assert_eq!(data_vars.len(), 1);
        assert_eq!(data_vars[0].name(), "temp");
    }
    #[test]
    fn test_variables_duplicate_fails() {
        let mut vars = Variables::new();
        vars.add(Variable::new("test", DataType::F32, vec![]).expect("Valid"))
            .expect("First add should succeed");
        let result = vars
            .add(Variable::new("test", DataType::F64, vec![]).expect("Valid"));
        assert!(result.is_err());
    }
}
