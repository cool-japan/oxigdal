//! CF Conventions 1.8 Compliance Module
//!
//! This module provides comprehensive support for CF (Climate and Forecast) Conventions
//! version 1.8 as defined at <http://cfconventions.org/Data/cf-conventions/cf-conventions-1.8/cf-conventions.html>.
//!
//! # Features
//!
//! - Attribute validation
//! - Standard name tables
//! - Units validation
//! - Coordinate variable detection
//! - Grid mapping support
//! - Cell methods parsing
//! - Bounds variables
//! - Cell measures
//!
//! # Example
//!
//! ```ignore
//! use oxigdal_netcdf::cf_conventions::{CfValidator, CfComplianceLevel};
//!
//! let validator = CfValidator::new();
//! let report = validator.validate(&metadata)?;
//!
//! if report.is_compliant(CfComplianceLevel::Required) {
//!     println!("File is CF compliant!");
//! }
//! ```

use serde::{Deserialize, Serialize};

use crate::attribute::Attributes;
use crate::dimension::Dimensions;
use crate::variable::{DataType, Variable, Variables};

// Module declarations
mod coordinates;
mod metadata;
#[cfg(test)]
mod tests;
mod time;

// Re-exports
pub use coordinates::{AxisType, CoordinateDetector, GridMapping, GridMappingType};
pub use metadata::{
    CellMeasure, CellMeasureType, CellMethod, CellMethodOperation, StandardNameEntry,
    StandardNameTable, UnitsValidator,
};
pub use time::BoundsVariable;

// ============================================================================
// CF Compliance Level and Validation Issue Types
// ============================================================================

/// Level of CF compliance requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CfComplianceLevel {
    /// Required for CF compliance
    Required,
    /// Recommended for better interoperability
    Recommended,
    /// Optional/informational
    Optional,
}

impl CfComplianceLevel {
    /// Get the level name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Required => "Required",
            Self::Recommended => "Recommended",
            Self::Optional => "Optional",
        }
    }
}

/// Type of validation issue.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CfIssueType {
    /// Missing required attribute
    MissingAttribute,
    /// Invalid attribute value
    InvalidAttributeValue,
    /// Invalid standard name
    InvalidStandardName,
    /// Invalid units
    InvalidUnits,
    /// Units mismatch with standard name
    UnitsMismatch,
    /// Invalid cell methods
    InvalidCellMethods,
    /// Missing bounds variable
    MissingBounds,
    /// Invalid bounds variable
    InvalidBounds,
    /// Invalid grid mapping
    InvalidGridMapping,
    /// Missing coordinate variable
    MissingCoordinateVariable,
    /// Invalid coordinate variable
    InvalidCoordinateVariable,
    /// Invalid cell measures
    InvalidCellMeasures,
    /// General CF convention issue
    ConventionIssue,
}

impl CfIssueType {
    /// Get the issue type name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::MissingAttribute => "Missing Attribute",
            Self::InvalidAttributeValue => "Invalid Attribute Value",
            Self::InvalidStandardName => "Invalid Standard Name",
            Self::InvalidUnits => "Invalid Units",
            Self::UnitsMismatch => "Units Mismatch",
            Self::InvalidCellMethods => "Invalid Cell Methods",
            Self::MissingBounds => "Missing Bounds",
            Self::InvalidBounds => "Invalid Bounds",
            Self::InvalidGridMapping => "Invalid Grid Mapping",
            Self::MissingCoordinateVariable => "Missing Coordinate Variable",
            Self::InvalidCoordinateVariable => "Invalid Coordinate Variable",
            Self::InvalidCellMeasures => "Invalid Cell Measures",
            Self::ConventionIssue => "Convention Issue",
        }
    }
}

/// A single validation issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfValidationIssue {
    /// Level of the issue
    pub level: CfComplianceLevel,
    /// Type of issue
    pub issue_type: CfIssueType,
    /// Variable name (if applicable)
    pub variable: Option<String>,
    /// Attribute name (if applicable)
    pub attribute: Option<String>,
    /// Description of the issue
    pub message: String,
    /// CF convention section reference
    pub section: Option<String>,
}

impl CfValidationIssue {
    /// Create a new validation issue.
    #[must_use]
    pub fn new(
        level: CfComplianceLevel,
        issue_type: CfIssueType,
        message: impl Into<String>,
    ) -> Self {
        Self {
            level,
            issue_type,
            variable: None,
            attribute: None,
            message: message.into(),
            section: None,
        }
    }

    /// Set the variable name.
    #[must_use]
    pub fn with_variable(mut self, variable: impl Into<String>) -> Self {
        self.variable = Some(variable.into());
        self
    }

    /// Set the attribute name.
    #[must_use]
    pub fn with_attribute(mut self, attribute: impl Into<String>) -> Self {
        self.attribute = Some(attribute.into());
        self
    }

    /// Set the CF section reference.
    #[must_use]
    pub fn with_section(mut self, section: impl Into<String>) -> Self {
        self.section = Some(section.into());
        self
    }
}

/// Validation report containing all issues found.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfValidationReport {
    /// All validation issues
    issues: Vec<CfValidationIssue>,
    /// CF version validated against
    cf_version: String,
}

impl CfValidationReport {
    /// Create a new validation report.
    #[must_use]
    pub fn new(cf_version: impl Into<String>) -> Self {
        Self {
            issues: Vec::new(),
            cf_version: cf_version.into(),
        }
    }

    /// Add an issue to the report.
    pub fn add_issue(&mut self, issue: CfValidationIssue) {
        self.issues.push(issue);
    }

    /// Get all issues.
    #[must_use]
    pub fn issues(&self) -> &[CfValidationIssue] {
        &self.issues
    }

    /// Get issues at a specific level.
    pub fn issues_at_level(
        &self,
        level: CfComplianceLevel,
    ) -> impl Iterator<Item = &CfValidationIssue> {
        self.issues.iter().filter(move |i| i.level == level)
    }

    /// Check if compliant at a given level (no issues at that level or higher).
    #[must_use]
    pub fn is_compliant(&self, level: CfComplianceLevel) -> bool {
        !self.issues.iter().any(|i| i.level <= level)
    }

    /// Count issues at a specific level.
    #[must_use]
    pub fn count_at_level(&self, level: CfComplianceLevel) -> usize {
        self.issues.iter().filter(|i| i.level == level).count()
    }

    /// Get the CF version.
    #[must_use]
    pub fn cf_version(&self) -> &str {
        &self.cf_version
    }

    /// Get a summary of the report.
    #[must_use]
    pub fn summary(&self) -> String {
        let required = self.count_at_level(CfComplianceLevel::Required);
        let recommended = self.count_at_level(CfComplianceLevel::Recommended);
        let optional = self.count_at_level(CfComplianceLevel::Optional);

        format!(
            "CF-{} Validation: {} required, {} recommended, {} optional issues",
            self.cf_version, required, recommended, optional
        )
    }
}

// ============================================================================
// CF Validator
// ============================================================================

/// CF Conventions validator.
#[derive(Debug)]
pub struct CfValidator {
    /// Standard name table
    standard_names: StandardNameTable,
    /// Units validator
    units_validator: UnitsValidator,
    /// Coordinate detector
    coordinate_detector: CoordinateDetector,
    /// CF version to validate against
    cf_version: String,
}

impl Default for CfValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl CfValidator {
    /// Create a new CF validator for version 1.8.
    #[must_use]
    pub fn new() -> Self {
        Self {
            standard_names: StandardNameTable::cf_1_8(),
            units_validator: UnitsValidator::new(),
            coordinate_detector: CoordinateDetector::new(),
            cf_version: "1.8".to_string(),
        }
    }

    /// Set the CF version to validate against.
    pub fn set_version(&mut self, version: impl Into<String>) {
        self.cf_version = version.into();
    }

    /// Validate global attributes.
    pub fn validate_global_attributes(&self, attrs: &Attributes, report: &mut CfValidationReport) {
        // Check Conventions attribute (required)
        if !attrs.contains("Conventions") {
            report.add_issue(
                CfValidationIssue::new(
                    CfComplianceLevel::Required,
                    CfIssueType::MissingAttribute,
                    "Missing 'Conventions' global attribute",
                )
                .with_attribute("Conventions")
                .with_section("2.6.1"),
            );
        } else if let Some(attr) = attrs.get("Conventions") {
            if let Ok(conv) = attr.value().as_text() {
                if !conv.contains("CF-") {
                    report.add_issue(
                        CfValidationIssue::new(
                            CfComplianceLevel::Required,
                            CfIssueType::InvalidAttributeValue,
                            format!("Conventions attribute '{}' does not contain 'CF-'", conv),
                        )
                        .with_attribute("Conventions")
                        .with_section("2.6.1"),
                    );
                }
            }
        }

        // Check recommended attributes
        let recommended = ["title", "institution", "source", "history", "references"];
        for attr_name in recommended {
            if !attrs.contains(attr_name) {
                report.add_issue(
                    CfValidationIssue::new(
                        CfComplianceLevel::Recommended,
                        CfIssueType::MissingAttribute,
                        format!("Missing recommended global attribute '{}'", attr_name),
                    )
                    .with_attribute(attr_name)
                    .with_section("2.6.2"),
                );
            }
        }
    }

    /// Validate a variable's attributes.
    pub fn validate_variable(
        &self,
        var: &Variable,
        dimensions: &Dimensions,
        report: &mut CfValidationReport,
    ) {
        let var_name = var.name();
        let attrs = var.attributes();

        // Check standard_name
        if let Some(attr) = attrs.get("standard_name") {
            if let Ok(std_name) = attr.value().as_text() {
                // Validate standard name exists
                if !self.standard_names.contains(std_name) {
                    report.add_issue(
                        CfValidationIssue::new(
                            CfComplianceLevel::Recommended,
                            CfIssueType::InvalidStandardName,
                            format!("Unknown standard_name '{}'", std_name),
                        )
                        .with_variable(var_name)
                        .with_attribute("standard_name")
                        .with_section("3.3"),
                    );
                }

                // Check units match canonical units
                if let Some(canonical) = self.standard_names.canonical_units(std_name) {
                    if let Some(units_attr) = attrs.get("units") {
                        if let Ok(units) = units_attr.value().as_text() {
                            if !self.units_validator.are_compatible(units, canonical) {
                                report.add_issue(
                                    CfValidationIssue::new(
                                        CfComplianceLevel::Required,
                                        CfIssueType::UnitsMismatch,
                                        format!(
                                            "Units '{}' not compatible with canonical units '{}' for standard_name '{}'",
                                            units, canonical, std_name
                                        ),
                                    )
                                    .with_variable(var_name)
                                    .with_section("3.1"),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Check units attribute for data variables
        if !self
            .coordinate_detector
            .is_coordinate_variable(var, dimensions)
            && !attrs.contains("units")
            && var.data_type() != DataType::Char
            && var.data_type() != DataType::String
        {
            report.add_issue(
                CfValidationIssue::new(
                    CfComplianceLevel::Recommended,
                    CfIssueType::MissingAttribute,
                    format!("Variable '{}' missing 'units' attribute", var_name),
                )
                .with_variable(var_name)
                .with_section("3.1"),
            );
        }

        // Validate units if present
        if let Some(units_attr) = attrs.get("units") {
            if let Ok(units) = units_attr.value().as_text() {
                if !self.units_validator.is_valid(units) {
                    report.add_issue(
                        CfValidationIssue::new(
                            CfComplianceLevel::Recommended,
                            CfIssueType::InvalidUnits,
                            format!("Invalid units '{}' for variable '{}'", units, var_name),
                        )
                        .with_variable(var_name)
                        .with_attribute("units")
                        .with_section("3.1"),
                    );
                }
            }
        }

        // Check cell_methods
        if let Some(cm_attr) = attrs.get("cell_methods") {
            if let Ok(cell_methods) = cm_attr.value().as_text() {
                if let Err(e) = CellMethod::parse_cell_methods(cell_methods) {
                    report.add_issue(
                        CfValidationIssue::new(
                            CfComplianceLevel::Required,
                            CfIssueType::InvalidCellMethods,
                            format!("Invalid cell_methods '{}': {}", cell_methods, e),
                        )
                        .with_variable(var_name)
                        .with_attribute("cell_methods")
                        .with_section("7.3"),
                    );
                }
            }
        }

        // Check bounds attribute
        if let Some(bounds_attr) = attrs.get("bounds") {
            if let Ok(_bounds_var_name) = bounds_attr.value().as_text() {
                // Bounds validation would require access to all variables
                // This is handled in validate_all
            }
        }

        // Check cell_measures
        if let Some(cm_attr) = attrs.get("cell_measures") {
            if let Ok(cell_measures) = cm_attr.value().as_text() {
                if let Err(e) = CellMeasure::parse_cell_measures(cell_measures) {
                    report.add_issue(
                        CfValidationIssue::new(
                            CfComplianceLevel::Required,
                            CfIssueType::InvalidCellMeasures,
                            format!("Invalid cell_measures '{}': {}", cell_measures, e),
                        )
                        .with_variable(var_name)
                        .with_attribute("cell_measures")
                        .with_section("7.2"),
                    );
                }
            }
        }
    }

    /// Validate grid mappings.
    pub fn validate_grid_mapping(
        &self,
        var: &Variable,
        variables: &Variables,
        report: &mut CfValidationReport,
    ) {
        let var_name = var.name();

        if let Some(gm_attr) = var.attributes().get("grid_mapping") {
            if let Ok(gm_var_name) = gm_attr.value().as_text() {
                // Check grid mapping variable exists
                if let Some(gm_var) = variables.get(gm_var_name) {
                    // Check grid_mapping_name attribute
                    if let Some(gmn_attr) = gm_var.attributes().get("grid_mapping_name") {
                        if let Ok(gm_name) = gmn_attr.value().as_text() {
                            let gm_type = GridMappingType::from_cf_name(gm_name);
                            if gm_type == GridMappingType::Unknown {
                                report.add_issue(
                                    CfValidationIssue::new(
                                        CfComplianceLevel::Recommended,
                                        CfIssueType::InvalidGridMapping,
                                        format!("Unknown grid_mapping_name '{}'", gm_name),
                                    )
                                    .with_variable(gm_var_name)
                                    .with_section("5.6"),
                                );
                            }
                        }
                    } else {
                        report.add_issue(
                            CfValidationIssue::new(
                                CfComplianceLevel::Required,
                                CfIssueType::InvalidGridMapping,
                                format!("Grid mapping variable '{}' missing 'grid_mapping_name' attribute", gm_var_name),
                            )
                            .with_variable(gm_var_name)
                            .with_section("5.6"),
                        );
                    }
                } else {
                    report.add_issue(
                        CfValidationIssue::new(
                            CfComplianceLevel::Required,
                            CfIssueType::InvalidGridMapping,
                            format!(
                                "Grid mapping variable '{}' referenced by '{}' not found",
                                gm_var_name, var_name
                            ),
                        )
                        .with_variable(var_name)
                        .with_section("5.6"),
                    );
                }
            }
        }
    }

    /// Validate coordinate variables.
    pub fn validate_coordinates(
        &self,
        dimensions: &Dimensions,
        variables: &Variables,
        report: &mut CfValidationReport,
    ) {
        // Check that each dimension has a coordinate variable (recommended)
        for dim in dimensions.iter() {
            let dim_name = dim.name();
            if variables.get(dim_name).is_none() {
                report.add_issue(
                    CfValidationIssue::new(
                        CfComplianceLevel::Recommended,
                        CfIssueType::MissingCoordinateVariable,
                        format!(
                            "Dimension '{}' has no corresponding coordinate variable",
                            dim_name
                        ),
                    )
                    .with_section("5.1"),
                );
            }
        }

        // Validate coordinate variables
        for var in variables.iter() {
            if self
                .coordinate_detector
                .is_coordinate_variable(var, dimensions)
            {
                // Coordinate variables should be monotonic
                // (This would require data access, so we just check attributes)

                // Check for axis attribute (recommended for coordinate variables)
                if var.attributes().get("axis").is_none() {
                    if let Some(_axis) = self.coordinate_detector.detect_axis(var) {
                        report.add_issue(
                            CfValidationIssue::new(
                                CfComplianceLevel::Recommended,
                                CfIssueType::MissingAttribute,
                                format!(
                                    "Coordinate variable '{}' should have 'axis' attribute",
                                    var.name()
                                ),
                            )
                            .with_variable(var.name())
                            .with_section("4"),
                        );
                    }
                }
            }
        }
    }

    /// Validate bounds variables.
    pub fn validate_bounds(
        &self,
        variables: &Variables,
        dimensions: &Dimensions,
        report: &mut CfValidationReport,
    ) {
        for var in variables.iter() {
            if let Some(bounds_attr) = var.attributes().get("bounds") {
                if let Ok(bounds_var_name) = bounds_attr.value().as_text() {
                    if let Some(bounds_var) = variables.get(bounds_var_name) {
                        // Validate bounds structure
                        let bounds = BoundsVariable::new(bounds_var_name, var.name(), 2);
                        if let Err(e) = bounds.validate(var, bounds_var, dimensions) {
                            report.add_issue(
                                CfValidationIssue::new(
                                    CfComplianceLevel::Required,
                                    CfIssueType::InvalidBounds,
                                    format!("Invalid bounds variable '{}': {}", bounds_var_name, e),
                                )
                                .with_variable(var.name())
                                .with_section("7.1"),
                            );
                        }
                    } else {
                        report.add_issue(
                            CfValidationIssue::new(
                                CfComplianceLevel::Required,
                                CfIssueType::MissingBounds,
                                format!(
                                    "Bounds variable '{}' referenced by '{}' not found",
                                    bounds_var_name,
                                    var.name()
                                ),
                            )
                            .with_variable(var.name())
                            .with_section("7.1"),
                        );
                    }
                }
            }
        }
    }

    /// Perform full validation.
    pub fn validate(
        &self,
        global_attrs: &Attributes,
        dimensions: &Dimensions,
        variables: &Variables,
    ) -> CfValidationReport {
        let mut report = CfValidationReport::new(&self.cf_version);

        // Validate global attributes
        self.validate_global_attributes(global_attrs, &mut report);

        // Validate each variable
        for var in variables.iter() {
            self.validate_variable(var, dimensions, &mut report);
            self.validate_grid_mapping(var, variables, &mut report);
        }

        // Validate coordinates
        self.validate_coordinates(dimensions, variables, &mut report);

        // Validate bounds
        self.validate_bounds(variables, dimensions, &mut report);

        report
    }
}
