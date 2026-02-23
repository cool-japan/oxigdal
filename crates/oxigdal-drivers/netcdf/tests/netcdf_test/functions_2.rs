//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use oxigdal_netcdf::{
    Attribute, AttributeValue, Attributes, CfMetadata, DataType, Dimension,
    DimensionSize, Dimensions, NetCdfError, NetCdfMetadata, NetCdfReader, NetCdfVersion,
    NetCdfWriter, Variable, Variables,
};

mod attribute_tests {
    use super::*;
    #[test]
    fn test_attribute_value_text() {
        let val = AttributeValue::text("Test string");
        assert_eq!(val.as_text().expect("Should be text"), "Test string");
        assert_eq!(val.type_name(), "text");
        assert_eq!(val.len(), 11);
    }
    #[test]
    fn test_attribute_value_i8() {
        let val = AttributeValue::i8(42);
        assert_eq!(val.type_name(), "i8");
        assert_eq!(val.len(), 1);
    }
    #[test]
    fn test_attribute_value_i8_array() {
        let val = AttributeValue::i8_array(vec![1, 2, 3, 4, 5]);
        assert_eq!(val.type_name(), "i8");
        assert_eq!(val.len(), 5);
    }
    #[test]
    fn test_attribute_value_i32() {
        let val = AttributeValue::i32(42);
        assert_eq!(val.as_i32().expect("Should be i32"), 42);
        assert_eq!(val.type_name(), "i32");
    }
    #[test]
    fn test_attribute_value_i32_array() {
        let val = AttributeValue::i32_array(vec![1, 2, 3]);
        assert_eq!(val.type_name(), "i32");
        assert_eq!(val.len(), 3);
        assert!(val.as_i32().is_err());
    }
    #[test]
    fn test_attribute_value_f32() {
        let val = AttributeValue::f32(3.14);
        assert_eq!(val.type_name(), "f32");
        assert_eq!(val.len(), 1);
    }
    #[test]
    fn test_attribute_value_f64() {
        let val = AttributeValue::f64(std::f64::consts::PI);
        assert!(
            (val.as_f64().expect("Should be f64") - std::f64::consts::PI).abs() < 1e-10
        );
        assert_eq!(val.type_name(), "f64");
    }
    #[test]
    fn test_attribute_value_f64_array() {
        let val = AttributeValue::f64_array(vec![1.0, 2.0, 3.0]);
        assert_eq!(val.type_name(), "f64");
        assert_eq!(val.len(), 3);
        assert!(val.as_f64().is_err());
    }
    #[test]
    fn test_attribute_value_type_mismatch() {
        let val = AttributeValue::i32(42);
        assert!(val.as_text().is_err());
        assert!(val.as_f64().is_err());
    }
    #[test]
    fn test_attribute_new() {
        let attr = Attribute::new("title", AttributeValue::text("Test"))
            .expect("Failed to create attribute");
        assert_eq!(attr.name(), "title");
        assert_eq!(attr.value().as_text().expect("Should be text"), "Test");
    }
    #[test]
    fn test_attribute_empty_name_fails() {
        let result = Attribute::new("", AttributeValue::text("value"));
        assert!(result.is_err());
        match result {
            Err(NetCdfError::AttributeError(msg)) => {
                assert!(msg.contains("empty"), "Error should mention empty: {}", msg);
            }
            _ => panic!("Expected AttributeError"),
        }
    }
    #[test]
    fn test_attributes_collection() {
        let mut attrs = Attributes::new();
        assert!(attrs.is_empty());
        attrs
            .add(Attribute::new("title", AttributeValue::text("Test")).expect("Valid"))
            .expect("Failed");
        attrs
            .add(Attribute::new("version", AttributeValue::i32(1)).expect("Valid"))
            .expect("Failed");
        assert_eq!(attrs.len(), 2);
        assert!(attrs.contains("title"));
        assert!(attrs.contains("version"));
    }
    #[test]
    fn test_attributes_get_value() {
        let mut attrs = Attributes::new();
        attrs
            .add(Attribute::new("count", AttributeValue::i32(42)).expect("Valid"))
            .expect("Failed");
        let value = attrs.get_value("count").expect("Attribute should exist");
        assert_eq!(value.as_i32().expect("Should be i32"), 42);
        assert!(attrs.get_value("nonexistent").is_none());
    }
    #[test]
    fn test_attributes_set_replace() {
        let mut attrs = Attributes::new();
        attrs.set(Attribute::new("test", AttributeValue::i32(1)).expect("Valid"));
        assert_eq!(
            attrs.get_value("test").expect("Should exist").as_i32()
            .expect("Should be i32"), 1
        );
        attrs.set(Attribute::new("test", AttributeValue::i32(2)).expect("Valid"));
        assert_eq!(
            attrs.get_value("test").expect("Should exist").as_i32()
            .expect("Should be i32"), 2
        );
        assert_eq!(attrs.len(), 1);
    }
    #[test]
    fn test_attributes_remove() {
        let mut attrs = Attributes::new();
        attrs.set(Attribute::new("test", AttributeValue::i32(1)).expect("Valid"));
        assert!(attrs.contains("test"));
        let removed = attrs.remove("test");
        assert!(removed.is_some());
        assert!(! attrs.contains("test"));
        assert!(attrs.remove("nonexistent").is_none());
    }
    #[test]
    fn test_attributes_names() {
        let mut attrs = Attributes::new();
        attrs
            .add(Attribute::new("first", AttributeValue::text("a")).expect("Valid"))
            .expect("Failed");
        attrs
            .add(Attribute::new("second", AttributeValue::text("b")).expect("Valid"))
            .expect("Failed");
        let names = attrs.names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(& "first"));
        assert!(names.contains(& "second"));
    }
    #[test]
    fn test_attributes_duplicate_fails() {
        let mut attrs = Attributes::new();
        attrs
            .add(Attribute::new("test", AttributeValue::i32(1)).expect("Valid"))
            .expect("First add should succeed");
        let result = attrs
            .add(Attribute::new("test", AttributeValue::i32(2)).expect("Valid"));
        assert!(result.is_err());
    }
}
mod metadata_tests {
    use super::*;
    #[test]
    fn test_netcdf_version_classic() {
        let version = NetCdfVersion::Classic;
        assert!(version.is_netcdf3());
        assert!(! version.is_netcdf4());
        assert_eq!(version.version_number(), 3);
        assert_eq!(version.format_name(), "NetCDF-3 Classic");
    }
    #[test]
    fn test_netcdf_version_offset64bit() {
        let version = NetCdfVersion::Offset64Bit;
        assert!(version.is_netcdf3());
        assert!(! version.is_netcdf4());
        assert_eq!(version.version_number(), 3);
        assert_eq!(version.format_name(), "NetCDF-3 64-bit Offset");
    }
    #[test]
    fn test_netcdf_version_netcdf4() {
        let version = NetCdfVersion::NetCdf4;
        assert!(! version.is_netcdf3());
        assert!(version.is_netcdf4());
        assert_eq!(version.version_number(), 4);
        assert_eq!(version.format_name(), "NetCDF-4");
    }
    #[test]
    fn test_netcdf_version_netcdf4_classic() {
        let version = NetCdfVersion::NetCdf4Classic;
        assert!(! version.is_netcdf3());
        assert!(version.is_netcdf4());
        assert_eq!(version.version_number(), 4);
        assert_eq!(version.format_name(), "NetCDF-4 Classic");
    }
    #[test]
    fn test_netcdf_version_default() {
        let version = NetCdfVersion::default();
        assert_eq!(version, NetCdfVersion::Classic);
    }
    #[test]
    fn test_cf_metadata_new() {
        let cf = CfMetadata::new();
        assert!(cf.conventions.is_none());
        assert!(cf.title.is_none());
        assert!(cf.institution.is_none());
        assert!(! cf.has_conventions());
        assert!(! cf.is_cf_compliant());
    }
    #[test]
    fn test_cf_metadata_cf_compliant() {
        let mut cf = CfMetadata::new();
        cf.conventions = Some("CF-1.8".to_string());
        assert!(cf.has_conventions());
        assert!(cf.is_cf_compliant());
    }
    #[test]
    fn test_cf_metadata_non_cf_conventions() {
        let mut cf = CfMetadata::new();
        cf.conventions = Some("Other-1.0".to_string());
        assert!(cf.has_conventions());
        assert!(! cf.is_cf_compliant());
    }
    #[test]
    fn test_cf_metadata_from_attributes() {
        let mut attrs = Attributes::new();
        attrs
            .add(
                Attribute::new("Conventions", AttributeValue::text("CF-1.8"))
                    .expect("Valid"),
            )
            .expect("Failed");
        attrs
            .add(
                Attribute::new("title", AttributeValue::text("Test Data"))
                    .expect("Valid"),
            )
            .expect("Failed");
        attrs
            .add(
                Attribute::new("institution", AttributeValue::text("Test Lab"))
                    .expect("Valid"),
            )
            .expect("Failed");
        attrs
            .add(Attribute::new("source", AttributeValue::text("Model")).expect("Valid"))
            .expect("Failed");
        let cf = CfMetadata::from_attributes(&attrs);
        assert_eq!(cf.conventions.as_deref(), Some("CF-1.8"));
        assert_eq!(cf.title.as_deref(), Some("Test Data"));
        assert_eq!(cf.institution.as_deref(), Some("Test Lab"));
        assert_eq!(cf.source.as_deref(), Some("Model"));
    }
    #[test]
    fn test_cf_metadata_to_attributes() {
        let mut cf = CfMetadata::new();
        cf.conventions = Some("CF-1.8".to_string());
        cf.title = Some("Test".to_string());
        cf.institution = Some("Lab".to_string());
        let attrs = cf.to_attributes();
        assert!(attrs.contains("Conventions"));
        assert!(attrs.contains("title"));
        assert!(attrs.contains("institution"));
        assert_eq!(attrs.len(), 3);
    }
    #[test]
    fn test_netcdf_metadata_new() {
        let metadata = NetCdfMetadata::new(NetCdfVersion::Classic);
        assert_eq!(metadata.version(), NetCdfVersion::Classic);
        assert!(metadata.dimensions().is_empty());
        assert!(metadata.variables().is_empty());
        assert!(metadata.global_attributes().is_empty());
        assert!(metadata.cf_metadata().is_none());
    }
    #[test]
    fn test_netcdf_metadata_new_classic() {
        let metadata = NetCdfMetadata::new_classic();
        assert_eq!(metadata.version(), NetCdfVersion::Classic);
    }
    #[test]
    fn test_netcdf_metadata_new_netcdf4() {
        let metadata = NetCdfMetadata::new_netcdf4();
        assert_eq!(metadata.version(), NetCdfVersion::NetCdf4);
    }
    #[test]
    fn test_netcdf_metadata_default() {
        let metadata = NetCdfMetadata::default();
        assert_eq!(metadata.version(), NetCdfVersion::Classic);
    }
    #[test]
    fn test_netcdf_metadata_add_dimension() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new("x", 10).expect("Valid"))
            .expect("Failed");
        assert_eq!(metadata.dimensions().len(), 1);
        assert!(metadata.dimensions().contains("x"));
    }
    #[test]
    fn test_netcdf_metadata_add_variable() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new("x", 10).expect("Valid"))
            .expect("Failed");
        metadata
            .variables_mut()
            .add(
                Variable::new("data", DataType::F32, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        assert_eq!(metadata.variables().len(), 1);
        assert!(metadata.variables().contains("data"));
    }
    #[test]
    fn test_netcdf_metadata_add_global_attribute() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .global_attributes_mut()
            .add(Attribute::new("title", AttributeValue::text("Test")).expect("Valid"))
            .expect("Failed");
        assert!(metadata.global_attributes().contains("title"));
    }
    #[test]
    fn test_netcdf_metadata_validate_success() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new("x", 10).expect("Valid"))
            .expect("Failed");
        metadata
            .variables_mut()
            .add(Variable::new_coordinate("x", DataType::F32).expect("Valid"))
            .expect("Failed");
        assert!(metadata.validate().is_ok());
    }
    #[test]
    fn test_netcdf_metadata_validate_missing_dimension() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .variables_mut()
            .add(
                Variable::new("data", DataType::F32, vec!["missing".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        let result = metadata.validate();
        assert!(result.is_err());
        match result {
            Err(NetCdfError::DimensionNotFound { name }) => {
                assert_eq!(name, "missing");
            }
            _ => panic!("Expected DimensionNotFound error"),
        }
    }
    #[test]
    fn test_netcdf_metadata_validate_incompatible_type() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new("x", 10).expect("Valid"))
            .expect("Failed");
        metadata
            .variables_mut()
            .add(
                Variable::new("data", DataType::U16, vec!["x".to_string()])
                    .expect("Valid"),
            )
            .expect("Failed");
        let result = metadata.validate();
        assert!(result.is_err());
    }
    #[test]
    fn test_netcdf_metadata_validate_multiple_unlimited_dims() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new_unlimited("time", 10).expect("Valid"))
            .expect("Failed");
        metadata
            .dimensions_mut()
            .add(Dimension::new_unlimited("level", 5).expect("Valid"))
            .expect("Failed");
        let result = metadata.validate();
        assert!(result.is_err());
        match result {
            Err(NetCdfError::UnlimitedDimensionError(msg)) => {
                assert!(
                    msg.contains("one unlimited"),
                    "Error should mention one unlimited: {}", msg
                );
            }
            _ => panic!("Expected UnlimitedDimensionError"),
        }
    }
    #[test]
    fn test_netcdf_metadata_summary() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .dimensions_mut()
            .add(Dimension::new("x", 10).expect("Valid"))
            .expect("Failed");
        metadata
            .dimensions_mut()
            .add(Dimension::new("y", 20).expect("Valid"))
            .expect("Failed");
        metadata
            .variables_mut()
            .add(
                Variable::new(
                        "data",
                        DataType::F32,
                        vec!["x".to_string(), "y".to_string()],
                    )
                    .expect("Valid"),
            )
            .expect("Failed");
        metadata
            .global_attributes_mut()
            .add(Attribute::new("title", AttributeValue::text("Test")).expect("Valid"))
            .expect("Failed");
        let summary = metadata.summary();
        assert!(summary.contains("NetCDF-3"));
        assert!(summary.contains("2 dimensions"));
        assert!(summary.contains("1 variables"));
        assert!(summary.contains("1 global attributes"));
    }
    #[test]
    fn test_netcdf_metadata_parse_cf_metadata() {
        let mut metadata = NetCdfMetadata::new_classic();
        metadata
            .global_attributes_mut()
            .add(
                Attribute::new("Conventions", AttributeValue::text("CF-1.8"))
                    .expect("Valid"),
            )
            .expect("Failed");
        metadata
            .global_attributes_mut()
            .add(
                Attribute::new("title", AttributeValue::text("Test Data"))
                    .expect("Valid"),
            )
            .expect("Failed");
        metadata.parse_cf_metadata();
        let cf = metadata.cf_metadata().expect("CF metadata should exist");
        assert!(cf.is_cf_compliant());
        assert_eq!(cf.conventions.as_deref(), Some("CF-1.8"));
    }
}
