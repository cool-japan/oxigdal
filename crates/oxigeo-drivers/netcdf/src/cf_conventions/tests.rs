//! Tests for CF Conventions Support

#[cfg(test)]
mod cf_conventions_tests {
    use super::super::*;
    use crate::attribute::{Attribute, AttributeValue};
    use crate::dimension::Dimension;

    #[test]
    fn test_standard_name_table() {
        let table = StandardNameTable::cf_1_8();

        assert!(table.contains("air_temperature"));
        assert!(table.contains("sea_surface_temperature"));
        assert!(!table.contains("invalid_name"));

        let entry = table.get("air_temperature");
        assert!(entry.is_some());
        let entry = entry.expect("Entry should exist");
        assert_eq!(entry.canonical_units, "K");
    }

    #[test]
    fn test_units_validator() {
        let validator = UnitsValidator::new();

        assert!(validator.is_valid("K"));
        assert!(validator.is_valid("m"));
        assert!(validator.is_valid("kg m-2 s-1"));
        assert!(validator.is_valid("degrees_north"));
        assert!(validator.is_valid("1"));

        assert!(validator.are_compatible("K", "K"));
        assert!(validator.are_compatible("celsius", "K"));
        assert!(validator.are_compatible("degrees_north", "degrees_north"));
    }

    #[test]
    fn test_grid_mapping_type() {
        assert_eq!(
            GridMappingType::from_cf_name("lambert_conformal_conic"),
            GridMappingType::LambertConformalConic
        );
        assert_eq!(
            GridMappingType::from_cf_name("polar_stereographic"),
            GridMappingType::PolarStereographic
        );
        assert_eq!(
            GridMappingType::from_cf_name("unknown_projection"),
            GridMappingType::Unknown
        );
    }

    #[test]
    fn test_cell_method_parsing() {
        let methods = CellMethod::parse_cell_methods("time: mean area: mean");
        assert!(methods.is_ok());
        let methods = methods.expect("Should parse");
        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].operation, CellMethodOperation::Mean);
        assert_eq!(methods[1].operation, CellMethodOperation::Mean);
    }

    #[test]
    fn test_cell_measures_parsing() {
        let measures = CellMeasure::parse_cell_measures("area: cell_area volume: cell_volume");
        assert!(measures.is_ok());
        let measures = measures.expect("Should parse");
        assert_eq!(measures.len(), 2);
        assert_eq!(measures[0].measure_type, CellMeasureType::Area);
        assert_eq!(measures[1].measure_type, CellMeasureType::Volume);
    }

    #[test]
    fn test_coordinate_detector() {
        use crate::dimension::Dimensions;
        use crate::variable::{DataType, Variable, Variables};

        let detector = CoordinateDetector::new();
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("time", 10).expect("Valid dim"))
            .expect("Add dim");

        let mut var =
            Variable::new("time", DataType::F64, vec!["time".to_string()]).expect("Valid var");
        var.attributes_mut().set(
            Attribute::new("units", AttributeValue::text("days since 2000-01-01"))
                .expect("Valid attr"),
        );

        assert!(detector.is_coordinate_variable(&var, &dims));
        assert_eq!(detector.detect_axis(&var), Some(AxisType::T));
    }

    #[test]
    fn test_cf_validator() {
        use crate::attribute::Attributes;
        use crate::dimension::Dimensions;
        use crate::variable::Variables;

        let validator = CfValidator::new();

        let mut attrs = Attributes::new();
        attrs.set(
            Attribute::new("Conventions", AttributeValue::text("CF-1.8")).expect("Valid attr"),
        );
        attrs.set(Attribute::new("title", AttributeValue::text("Test Data")).expect("Valid attr"));

        let mut dims = Dimensions::new();
        dims.add(Dimension::new("time", 10).expect("Valid dim"))
            .expect("Add dim");

        let vars = Variables::new();

        let report = validator.validate(&attrs, &dims, &vars);

        // Should have recommended issue for missing coordinate variable
        assert!(
            report
                .issues()
                .iter()
                .any(|i| i.issue_type == CfIssueType::MissingCoordinateVariable)
        );
    }

    #[test]
    fn test_validation_report() {
        let mut report = CfValidationReport::new("1.8");

        report.add_issue(CfValidationIssue::new(
            CfComplianceLevel::Required,
            CfIssueType::MissingAttribute,
            "Test issue",
        ));

        assert!(!report.is_compliant(CfComplianceLevel::Required));
        assert_eq!(report.count_at_level(CfComplianceLevel::Required), 1);
    }

    #[test]
    fn test_bounds_variable() {
        let bounds = BoundsVariable::new("time_bnds", "time", 2);
        assert_eq!(bounds.name, "time_bnds");
        assert_eq!(bounds.coordinate_variable, "time");
        assert_eq!(bounds.num_vertices, 2);
    }

    #[test]
    fn test_axis_type() {
        assert_eq!(AxisType::from_cf_value("X"), Some(AxisType::X));
        assert_eq!(AxisType::from_cf_value("y"), Some(AxisType::Y));
        assert_eq!(AxisType::from_cf_value("Z"), Some(AxisType::Z));
        assert_eq!(AxisType::from_cf_value("T"), Some(AxisType::T));
        assert_eq!(AxisType::from_cf_value("invalid"), None);
    }
}
