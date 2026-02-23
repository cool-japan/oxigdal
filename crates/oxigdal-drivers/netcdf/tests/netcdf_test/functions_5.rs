//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use oxigdal_netcdf::{
    Attribute, AttributeValue, Attributes, CfMetadata, DataType, Dimension,
    DimensionSize, Dimensions, NetCdfError, NetCdfMetadata, NetCdfReader, NetCdfVersion,
    NetCdfWriter, Variable, Variables,
};

mod edge_cases_tests {
    use super::*;
    #[test]
    fn test_empty_dimensions_collection() {
        let dims = Dimensions::new();
        assert!(dims.is_empty());
        assert_eq!(dims.len(), 0);
        assert!(dims.unlimited().is_none());
        assert_eq!(dims.total_size(), Some(0));
        assert!(dims.shape().is_empty());
        assert!(dims.names().is_empty());
    }
    #[test]
    fn test_empty_variables_collection() {
        let vars = Variables::new();
        assert!(vars.is_empty());
        assert_eq!(vars.len(), 0);
        assert!(vars.names().is_empty());
        assert_eq!(vars.coordinates().count(), 0);
        assert_eq!(vars.data_variables().count(), 0);
    }
    #[test]
    fn test_empty_attributes_collection() {
        let attrs = Attributes::new();
        assert!(attrs.is_empty());
        assert_eq!(attrs.len(), 0);
        assert!(attrs.names().is_empty());
    }
    #[test]
    fn test_dimension_very_large_size() {
        let dim = Dimension::new("large", 1_000_000).expect("Should handle large size");
        assert_eq!(dim.len(), 1_000_000);
    }
    #[test]
    fn test_variable_many_dimensions() {
        let mut dims = Dimensions::new();
        let dim_names: Vec<String> = (0..10).map(|i| format!("dim_{}", i)).collect();
        for name in &dim_names {
            dims.add(Dimension::new(name, 2).expect("Valid")).expect("Failed");
        }
        let var = Variable::new("high_dim", DataType::F32, dim_names.clone())
            .expect("Valid");
        assert_eq!(var.ndims(), 10);
        let shape = var.shape(&dims).expect("Should work");
        assert_eq!(shape.len(), 10);
        let size = var.size(&dims).expect("Should work");
        assert_eq!(size, 2usize.pow(10));
    }
    #[test]
    fn test_attribute_empty_string() {
        let attr = Attribute::new("empty_text", AttributeValue::text(""))
            .expect("Valid");
        assert!(attr.value().is_empty());
        assert_eq!(attr.value().len(), 0);
    }
    #[test]
    fn test_attribute_long_string() {
        let long_text = "A".repeat(10000);
        let attr = Attribute::new("long_text", AttributeValue::text(long_text.clone()))
            .expect("Valid");
        assert_eq!(attr.value().as_text().expect("Should be text"), long_text);
    }
    #[test]
    fn test_attribute_unicode_string() {
        let unicode_text = "Hello, \u{4e16}\u{754c}! \u{1F600}";
        let attr = Attribute::new("unicode", AttributeValue::text(unicode_text))
            .expect("Valid");
        assert_eq!(attr.value().as_text().expect("Should be text"), unicode_text);
    }
    #[test]
    fn test_dimension_name_special_chars() {
        let dim = Dimension::new("dim_with_underscore", 10).expect("Valid");
        assert_eq!(dim.name(), "dim_with_underscore");
    }
    #[test]
    fn test_variable_name_numbers() {
        let var = Variable::new("var123", DataType::F32, vec![]).expect("Valid");
        assert_eq!(var.name(), "var123");
    }
    #[test]
    fn test_cf_metadata_empty_fields() {
        let cf = CfMetadata::new();
        let attrs = cf.to_attributes();
        assert!(attrs.is_empty());
    }
    #[test]
    fn test_cf_metadata_partial_fields() {
        let mut cf = CfMetadata::new();
        cf.conventions = Some("CF-1.8".to_string());
        let attrs = cf.to_attributes();
        assert_eq!(attrs.len(), 1);
        assert!(attrs.contains("Conventions"));
        assert!(! attrs.contains("title"));
    }
    #[test]
    fn test_metadata_validate_empty() {
        let metadata = NetCdfMetadata::new_classic();
        assert!(metadata.validate().is_ok());
    }
    #[test]
    fn test_attribute_value_empty_arrays() {
        let val = AttributeValue::f64_array(vec![]);
        assert!(val.is_empty());
        assert_eq!(val.len(), 0);
    }
}
mod lib_tests {
    use oxigdal_netcdf::{
        NAME, VERSION, has_netcdf3, has_netcdf4, info, is_pure_rust, supported_versions,
    };
    #[test]
    fn test_version_constants() {
        assert!(! VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-netcdf");
    }
    #[test]
    fn test_pure_rust_status() {
        #[cfg(not(feature = "netcdf4"))]
        assert!(is_pure_rust());
        #[cfg(feature = "netcdf4")]
        assert!(! is_pure_rust());
    }
    #[test]
    fn test_feature_detection() {
        #[cfg(feature = "netcdf3")]
        assert!(has_netcdf3());
        #[cfg(not(feature = "netcdf3"))]
        assert!(! has_netcdf3());
        #[cfg(feature = "netcdf4")]
        assert!(has_netcdf4());
        #[cfg(not(feature = "netcdf4"))]
        assert!(! has_netcdf4());
    }
    #[test]
    fn test_supported_versions() {
        let versions = supported_versions();
        #[cfg(feature = "netcdf3")]
        {
            assert!(versions.contains(& super::NetCdfVersion::Classic));
            assert!(versions.contains(& super::NetCdfVersion::Offset64Bit));
        }
        #[cfg(feature = "netcdf4")]
        {
            assert!(versions.contains(& super::NetCdfVersion::NetCdf4));
            assert!(versions.contains(& super::NetCdfVersion::NetCdf4Classic));
        }
    }
    #[test]
    fn test_info_string() {
        let info_str = info();
        assert!(info_str.contains(NAME));
        assert!(info_str.contains(VERSION));
        #[cfg(not(feature = "netcdf4"))]
        assert!(info_str.contains("Pure Rust"));
    }
}
