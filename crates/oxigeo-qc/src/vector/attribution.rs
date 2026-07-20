//! Vector attribution (attribute) validation.
//!
//! This module provides quality control checks for vector feature attributes,
//! including required fields, domain validation, and data type validation.

use crate::error::{QcError, QcIssue, QcResult, Severity};
use oxigeo_core::vector::{FeatureCollection, FeatureId, FieldValue};
use std::collections::HashSet;

/// Helper function to convert FeatureId to String
fn feature_id_to_string(id: &FeatureId) -> String {
    match id {
        FeatureId::Integer(i) => i.to_string(),
        FeatureId::String(s) => s.clone(),
    }
}

/// Result of attribution validation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttributionResult {
    /// Total number of features checked.
    pub feature_count: usize,

    /// Number of features with all required fields.
    pub complete_features: usize,

    /// Number of features with missing required fields.
    pub incomplete_features: usize,

    /// Required field violations.
    pub required_field_violations: Vec<FieldViolation>,

    /// Domain validation violations.
    pub domain_violations: Vec<DomainViolation>,

    /// Data type violations.
    pub type_violations: Vec<TypeViolation>,

    /// Null value violations.
    pub null_violations: Vec<NullViolation>,

    /// Referential integrity violations.
    pub referential_violations: Vec<ReferentialViolation>,

    /// Quality control issues found.
    pub issues: Vec<QcIssue>,
}

/// Required field violation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FieldViolation {
    /// Feature ID.
    pub feature_id: Option<String>,

    /// Missing field name.
    pub field_name: String,

    /// Severity of the violation.
    pub severity: Severity,
}

/// Domain validation violation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DomainViolation {
    /// Feature ID.
    pub feature_id: Option<String>,

    /// Field name.
    pub field_name: String,

    /// Invalid value.
    pub value: String,

    /// Expected domain description.
    pub expected_domain: String,

    /// Severity of the violation.
    pub severity: Severity,
}

/// Data type violation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TypeViolation {
    /// Feature ID.
    pub feature_id: Option<String>,

    /// Field name.
    pub field_name: String,

    /// Actual type.
    pub actual_type: String,

    /// Expected type.
    pub expected_type: String,

    /// Severity of the violation.
    pub severity: Severity,
}

/// Null value violation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NullViolation {
    /// Feature ID.
    pub feature_id: Option<String>,

    /// Field name that should not be null.
    pub field_name: String,

    /// Severity of the violation.
    pub severity: Severity,
}

/// Referential integrity violation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReferentialViolation {
    /// Feature ID.
    pub feature_id: Option<String>,

    /// Field name.
    pub field_name: String,

    /// Foreign key value that doesn't exist.
    pub foreign_key: String,

    /// Referenced table/collection.
    pub referenced_table: String,

    /// Severity of the violation.
    pub severity: Severity,
}

/// Field definition for validation.
#[derive(Debug, Clone)]
pub struct FieldDefinition {
    /// Field name.
    pub name: String,

    /// Expected data type.
    pub expected_type: FieldType,

    /// Whether the field is required.
    pub required: bool,

    /// Whether null values are allowed.
    pub nullable: bool,

    /// Value domain (if applicable).
    pub domain: Option<ValueDomain>,

    /// Default value (if any).
    pub default_value: Option<FieldValue>,
}

/// Expected field type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// Boolean type.
    Boolean,

    /// Integer type.
    Integer,

    /// Float type.
    Float,

    /// String type.
    String,

    /// Date type.
    Date,

    /// DateTime type.
    DateTime,

    /// Any type accepted.
    Any,
}

/// Value domain for field validation.
#[derive(Debug, Clone)]
pub enum ValueDomain {
    /// Range of numeric values.
    NumericRange {
        /// Minimum allowed value.
        min: f64,
        /// Maximum allowed value.
        max: f64,
    },

    /// Enumeration of allowed values.
    Enumeration(HashSet<String>),

    /// String pattern (regex).
    Pattern(String),

    /// String length constraints.
    StringLength {
        /// Minimum length.
        min: usize,
        /// Maximum length.
        max: usize,
    },
}

/// Configuration for attribution checks.
#[derive(Debug, Clone, Default)]
pub struct AttributionConfig {
    /// Field definitions for validation.
    pub field_definitions: Vec<FieldDefinition>,

    /// Referential integrity constraints.
    pub referential_constraints: Vec<ReferentialConstraint>,

    /// Whether to validate date formats.
    pub validate_date_formats: bool,

    /// Date format string (if validating dates).
    pub date_format: Option<String>,
}

/// Referential integrity constraint.
#[derive(Debug, Clone)]
pub struct ReferentialConstraint {
    /// Field name in this feature collection.
    pub field_name: String,

    /// Referenced table/collection name.
    pub referenced_table: String,

    /// Valid foreign key values.
    pub valid_values: HashSet<String>,
}

/// Attribution checker.
pub struct AttributionChecker {
    config: AttributionConfig,
}

impl AttributionChecker {
    /// Creates a new attribution checker with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: AttributionConfig::default(),
        }
    }

    /// Creates a new attribution checker with custom configuration.
    #[must_use]
    pub fn with_config(config: AttributionConfig) -> Self {
        Self { config }
    }

    /// Validates attribution of a feature collection.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails.
    pub fn validate(&self, features: &FeatureCollection) -> QcResult<AttributionResult> {
        let mut issues = Vec::new();
        let mut required_field_violations = Vec::new();
        let mut domain_violations = Vec::new();
        let mut type_violations = Vec::new();
        let mut null_violations = Vec::new();
        let mut referential_violations = Vec::new();
        let mut complete_features = 0;
        let mut incomplete_features = 0;

        for feature in &features.features {
            let feature_id = feature.id.as_ref().map(feature_id_to_string);
            let mut is_complete = true;

            // Check required fields
            for field_def in &self.config.field_definitions {
                if field_def.required && !feature.properties.contains_key(&field_def.name) {
                    is_complete = false;
                    required_field_violations.push(FieldViolation {
                        feature_id: feature_id.clone(),
                        field_name: field_def.name.clone(),
                        severity: Severity::Major,
                    });
                }

                // Check field if it exists
                if let Some(value) = feature.properties.get(&field_def.name) {
                    // Check null values
                    if value.is_null() && !field_def.nullable {
                        null_violations.push(NullViolation {
                            feature_id: feature_id.clone(),
                            field_name: field_def.name.clone(),
                            severity: Severity::Major,
                        });
                    }

                    // Check data type
                    if !value.is_null()
                        && let Some(type_violation) =
                            self.check_type(value, field_def, &feature_id)?
                    {
                        type_violations.push(type_violation);
                    }

                    // Check domain
                    if let Some(ref domain) = field_def.domain
                        && let Some(domain_violation) =
                            self.check_domain(value, domain, &field_def.name, &feature_id)?
                    {
                        domain_violations.push(domain_violation);
                    }
                }
            }

            // Check referential integrity
            for constraint in &self.config.referential_constraints {
                if let Some(value) = feature.properties.get(&constraint.field_name)
                    && let Some(ref_violation) =
                        self.check_referential_integrity(value, constraint, &feature_id)?
                {
                    referential_violations.push(ref_violation);
                }
            }

            if is_complete {
                complete_features += 1;
            } else {
                incomplete_features += 1;
            }
        }

        // Create issues from violations
        for violation in &required_field_violations {
            issues.push(
                QcIssue::new(
                    violation.severity,
                    "attribution",
                    "Missing required field",
                    format!("Field '{}' is required but missing", violation.field_name),
                )
                .with_location(violation.feature_id.clone().unwrap_or_default())
                .with_suggestion("Add the required field"),
            );
        }

        for violation in &domain_violations {
            issues.push(
                QcIssue::new(
                    violation.severity,
                    "attribution",
                    "Domain violation",
                    format!(
                        "Field '{}' has invalid value '{}', expected: {}",
                        violation.field_name, violation.value, violation.expected_domain
                    ),
                )
                .with_location(violation.feature_id.clone().unwrap_or_default())
                .with_suggestion("Use a valid value from the domain"),
            );
        }

        for violation in &type_violations {
            issues.push(
                QcIssue::new(
                    violation.severity,
                    "attribution",
                    "Data type mismatch",
                    format!(
                        "Field '{}' has type '{}', expected '{}'",
                        violation.field_name, violation.actual_type, violation.expected_type
                    ),
                )
                .with_location(violation.feature_id.clone().unwrap_or_default())
                .with_suggestion("Convert value to expected type"),
            );
        }

        for violation in &null_violations {
            issues.push(
                QcIssue::new(
                    violation.severity,
                    "attribution",
                    "Null value not allowed",
                    format!(
                        "Field '{}' does not allow null values",
                        violation.field_name
                    ),
                )
                .with_location(violation.feature_id.clone().unwrap_or_default())
                .with_suggestion("Provide a non-null value"),
            );
        }

        for violation in &referential_violations {
            issues.push(
                QcIssue::new(
                    violation.severity,
                    "attribution",
                    "Referential integrity violation",
                    format!(
                        "Foreign key '{}' in field '{}' does not exist in '{}'",
                        violation.foreign_key, violation.field_name, violation.referenced_table
                    ),
                )
                .with_location(violation.feature_id.clone().unwrap_or_default())
                .with_suggestion("Use a valid foreign key value"),
            );
        }

        Ok(AttributionResult {
            feature_count: features.features.len(),
            complete_features,
            incomplete_features,
            required_field_violations,
            domain_violations,
            type_violations,
            null_violations,
            referential_violations,
            issues,
        })
    }

    /// Checks data type of a property value.
    fn check_type(
        &self,
        value: &FieldValue,
        field_def: &FieldDefinition,
        feature_id: &Option<String>,
    ) -> QcResult<Option<TypeViolation>> {
        let actual_type = match value {
            FieldValue::Null => "Null",
            FieldValue::Bool(_) => "Boolean",
            FieldValue::Integer(_) => "Integer",
            FieldValue::UInteger(_) => "Integer",
            FieldValue::Float(_) => "Float",
            FieldValue::Date(_) => "Date",
            FieldValue::Blob(_) => "Blob",
            FieldValue::String(_) => "String",
            FieldValue::Array(_) => "Array",
            FieldValue::Object(_) => "Object",
        };

        let matches = match field_def.expected_type {
            FieldType::Boolean => matches!(value, FieldValue::Bool(_)),
            FieldType::Integer => {
                matches!(value, FieldValue::Integer(_) | FieldValue::UInteger(_))
            }
            FieldType::Float => matches!(
                value,
                FieldValue::Float(_) | FieldValue::Integer(_) | FieldValue::UInteger(_)
            ),
            FieldType::String => matches!(value, FieldValue::String(_)),
            FieldType::Date | FieldType::DateTime => matches!(value, FieldValue::String(_)),
            FieldType::Any => true,
        };

        if !matches {
            Ok(Some(TypeViolation {
                feature_id: feature_id.clone(),
                field_name: field_def.name.clone(),
                actual_type: actual_type.to_string(),
                expected_type: format!("{:?}", field_def.expected_type),
                severity: Severity::Major,
            }))
        } else {
            Ok(None)
        }
    }

    /// Checks domain constraints.
    fn check_domain(
        &self,
        value: &FieldValue,
        domain: &ValueDomain,
        field_name: &str,
        feature_id: &Option<String>,
    ) -> QcResult<Option<DomainViolation>> {
        match domain {
            ValueDomain::NumericRange { min, max } => {
                let num_value = match value {
                    FieldValue::Integer(i) => Some(*i as f64),
                    FieldValue::UInteger(u) => Some(*u as f64),
                    FieldValue::Float(f) => Some(*f),
                    _ => None,
                };

                if let Some(num) = num_value
                    && (num < *min || num > *max)
                {
                    return Ok(Some(DomainViolation {
                        feature_id: feature_id.clone(),
                        field_name: field_name.to_string(),
                        value: format!("{}", num),
                        expected_domain: format!("[{}, {}]", min, max),
                        severity: Severity::Minor,
                    }));
                }
            }
            ValueDomain::Enumeration(allowed_values) => {
                if let FieldValue::String(s) = value
                    && !allowed_values.contains(s)
                {
                    return Ok(Some(DomainViolation {
                        feature_id: feature_id.clone(),
                        field_name: field_name.to_string(),
                        value: s.clone(),
                        expected_domain: format!("One of: {:?}", allowed_values),
                        severity: Severity::Minor,
                    }));
                }
            }
            ValueDomain::StringLength { min, max } => {
                if let FieldValue::String(s) = value
                    && (s.len() < *min || s.len() > *max)
                {
                    return Ok(Some(DomainViolation {
                        feature_id: feature_id.clone(),
                        field_name: field_name.to_string(),
                        value: s.clone(),
                        expected_domain: format!("Length: [{}, {}]", min, max),
                        severity: Severity::Minor,
                    }));
                }
            }
            ValueDomain::Pattern(pattern) => {
                if let FieldValue::String(s) = value {
                    let re = regex::Regex::new(pattern).map_err(|e| {
                        QcError::InvalidConfiguration(format!(
                            "Invalid pattern for field '{field_name}': '{pattern}' is not a valid regex: {e}"
                        ))
                    })?;
                    if !re.is_match(s) {
                        return Ok(Some(DomainViolation {
                            feature_id: feature_id.clone(),
                            field_name: field_name.to_string(),
                            value: s.clone(),
                            expected_domain: format!("Matches pattern: {pattern}"),
                            severity: Severity::Minor,
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Checks referential integrity.
    fn check_referential_integrity(
        &self,
        value: &FieldValue,
        constraint: &ReferentialConstraint,
        feature_id: &Option<String>,
    ) -> QcResult<Option<ReferentialViolation>> {
        if let FieldValue::String(s) = value
            && !constraint.valid_values.contains(s)
        {
            return Ok(Some(ReferentialViolation {
                feature_id: feature_id.clone(),
                field_name: constraint.field_name.clone(),
                foreign_key: s.clone(),
                referenced_table: constraint.referenced_table.clone(),
                severity: Severity::Major,
            }));
        }

        Ok(None)
    }
}

impl Default for AttributionChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigeo_core::vector::{Feature, Geometry, Point};

    #[test]
    fn test_attribution_checker_creation() {
        let checker = AttributionChecker::new();
        assert!(checker.config.field_definitions.is_empty());
    }

    #[test]
    fn test_required_field_validation() {
        let mut config = AttributionConfig::default();
        config.field_definitions.push(FieldDefinition {
            name: "name".to_string(),
            expected_type: FieldType::String,
            required: true,
            nullable: false,
            domain: None,
            default_value: None,
        });

        let checker = AttributionChecker::with_config(config);

        let feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
        let collection = FeatureCollection {
            features: vec![feature],
            metadata: None,
        };

        let result = checker.validate(&collection);
        assert!(result.is_ok());

        #[allow(clippy::unwrap_used, clippy::expect_used)]
        let result = result.expect("validation should succeed for required field test");
        assert_eq!(result.incomplete_features, 1);
        assert_eq!(result.required_field_violations.len(), 1);
    }

    #[test]
    fn test_field_type() {
        assert_eq!(format!("{:?}", FieldType::String), "String");
        assert_eq!(format!("{:?}", FieldType::Integer), "Integer");
    }

    // ── ValueDomain::Pattern enforcement ───────────────────────────────────

    #[test]
    fn test_pattern_domain_matching_value_no_violation() {
        let checker = AttributionChecker::new();
        let domain = ValueDomain::Pattern(r"^[A-Z]{2}-\d{4}$".to_string());
        let value = FieldValue::String("AB-1234".to_string());

        let result = checker
            .check_domain(&value, &domain, "code", &None)
            .expect("pattern check should succeed for a valid regex");

        assert!(
            result.is_none(),
            "a value matching the pattern must not produce a violation"
        );
    }

    #[test]
    fn test_pattern_domain_non_matching_value_is_violation() {
        let checker = AttributionChecker::new();
        let domain = ValueDomain::Pattern(r"^[A-Z]{2}-\d{4}$".to_string());
        let value = FieldValue::String("not-a-code".to_string());

        let result = checker
            .check_domain(&value, &domain, "code", &None)
            .expect("pattern check should succeed for a valid regex");

        let violation = result.expect("a value that fails to match the pattern must be flagged");
        assert_eq!(violation.field_name, "code");
        assert_eq!(violation.value, "not-a-code");
    }

    #[test]
    fn test_pattern_domain_invalid_regex_returns_error() {
        let checker = AttributionChecker::new();
        // Unbalanced group is not a valid regex.
        let domain = ValueDomain::Pattern("(unterminated".to_string());
        let value = FieldValue::String("anything".to_string());

        let result = checker.check_domain(&value, &domain, "code", &None);
        assert!(
            result.is_err(),
            "an invalid pattern must surface as an error, not silently pass"
        );
    }

    #[test]
    fn test_pattern_domain_enforced_through_validate() {
        let mut config = AttributionConfig::default();
        config.field_definitions.push(FieldDefinition {
            name: "postal_code".to_string(),
            expected_type: FieldType::String,
            required: false,
            nullable: false,
            domain: Some(ValueDomain::Pattern(r"^\d{5}$".to_string())),
            default_value: None,
        });

        let checker = AttributionChecker::with_config(config);

        let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
        feature.properties.insert(
            "postal_code".to_string(),
            FieldValue::String("abc".to_string()),
        );
        let collection = FeatureCollection {
            features: vec![feature],
            metadata: None,
        };

        let result = checker
            .validate(&collection)
            .expect("validation should succeed even when a pattern domain is violated");
        assert_eq!(
            result.domain_violations.len(),
            1,
            "ValueDomain::Pattern must actually be enforced through validate()"
        );
    }
}
