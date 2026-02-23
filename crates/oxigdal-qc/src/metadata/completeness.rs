//! Metadata completeness checks.
//!
//! This module provides quality control checks for metadata completeness,
//! including ISO 19115 and STAC metadata validation.

use crate::error::{QcIssue, QcResult, Severity};
use std::collections::{HashMap, HashSet};

/// Result of metadata completeness analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetadataResult {
    /// Metadata standard being checked.
    pub standard: MetadataStandard,

    /// Overall completeness score (0.0 - 100.0).
    pub completeness_score: f64,

    /// Number of required fields present.
    pub required_fields_present: usize,

    /// Number of required fields missing.
    pub required_fields_missing: usize,

    /// Number of optional fields present.
    pub optional_fields_present: usize,

    /// Missing required fields.
    pub missing_required: Vec<MissingField>,

    /// Missing optional fields.
    pub missing_optional: Vec<MissingField>,

    /// Controlled vocabulary violations.
    pub vocabulary_violations: Vec<VocabularyViolation>,

    /// Citation completeness.
    pub citation_completeness: Option<CitationCompleteness>,

    /// Quality control issues found.
    pub issues: Vec<QcIssue>,
}

/// Metadata standard being validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MetadataStandard {
    /// ISO 19115 metadata standard.
    Iso19115,

    /// STAC (SpatioTemporal Asset Catalog) metadata.
    Stac,

    /// Dublin Core metadata.
    DublinCore,

    /// Custom metadata schema.
    Custom,
}

/// Missing metadata field.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MissingField {
    /// Field name or path.
    pub field_name: String,

    /// Field description.
    pub description: String,

    /// Whether the field is required or optional.
    pub required: bool,

    /// Severity if missing.
    pub severity: Severity,
}

/// Controlled vocabulary violation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VocabularyViolation {
    /// Field name.
    pub field_name: String,

    /// Invalid value.
    pub value: String,

    /// Expected vocabulary name.
    pub vocabulary: String,

    /// List of valid values (if small enough to list).
    pub valid_values: Option<Vec<String>>,

    /// Severity of the violation.
    pub severity: Severity,
}

/// Citation completeness assessment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CitationCompleteness {
    /// Whether title is present.
    pub has_title: bool,

    /// Whether author(s) are present.
    pub has_authors: bool,

    /// Whether publication date is present.
    pub has_date: bool,

    /// Whether DOI or identifier is present.
    pub has_identifier: bool,

    /// Whether publisher is present.
    pub has_publisher: bool,

    /// Overall citation completeness score (0.0 - 100.0).
    pub completeness_score: f64,
}

/// Metadata field definition.
#[derive(Debug, Clone)]
pub struct MetadataFieldDef {
    /// Field name or path.
    pub name: String,

    /// Field description.
    pub description: String,

    /// Whether the field is required.
    pub required: bool,

    /// Controlled vocabulary (if applicable).
    pub vocabulary: Option<String>,

    /// Valid values (if vocabulary is defined).
    pub valid_values: Option<HashSet<String>>,
}

/// Configuration for metadata checks.
#[derive(Debug, Clone)]
pub struct MetadataConfig {
    /// Metadata standard to validate against.
    pub standard: MetadataStandard,

    /// Field definitions.
    pub field_definitions: Vec<MetadataFieldDef>,

    /// Minimum completeness score threshold (0.0 - 100.0).
    pub min_completeness_score: f64,

    /// Whether to check controlled vocabularies.
    pub check_vocabularies: bool,

    /// Whether to check citation completeness.
    pub check_citation: bool,
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self::iso19115_config()
    }
}

impl MetadataConfig {
    /// Creates configuration for ISO 19115 metadata.
    #[must_use]
    pub fn iso19115_config() -> Self {
        // Core ISO 19115 required fields
        let field_definitions = vec![
            MetadataFieldDef {
                name: "title".to_string(),
                description: "Dataset title".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "abstract".to_string(),
                description: "Dataset abstract or description".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "topic_category".to_string(),
                description: "ISO topic category".to_string(),
                required: true,
                vocabulary: Some("ISO_TopicCategory".to_string()),
                valid_values: Some(Self::iso_topic_categories()),
            },
            MetadataFieldDef {
                name: "contact".to_string(),
                description: "Responsible party contact".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "date".to_string(),
                description: "Reference date".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "spatial_extent".to_string(),
                description: "Geographic bounding box".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            // Optional but recommended fields
            MetadataFieldDef {
                name: "keywords".to_string(),
                description: "Descriptive keywords".to_string(),
                required: false,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "license".to_string(),
                description: "Data license or use constraints".to_string(),
                required: false,
                vocabulary: None,
                valid_values: None,
            },
        ];

        Self {
            standard: MetadataStandard::Iso19115,
            field_definitions,
            min_completeness_score: 80.0,
            check_vocabularies: true,
            check_citation: true,
        }
    }

    /// Creates configuration for STAC metadata.
    #[must_use]
    pub fn stac_config() -> Self {
        // STAC required fields
        let field_definitions = vec![
            MetadataFieldDef {
                name: "type".to_string(),
                description: "STAC object type".to_string(),
                required: true,
                vocabulary: Some("STAC_Type".to_string()),
                valid_values: Some(
                    ["Feature", "Catalog", "Collection"]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
            },
            MetadataFieldDef {
                name: "stac_version".to_string(),
                description: "STAC specification version".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "id".to_string(),
                description: "Unique identifier".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "geometry".to_string(),
                description: "GeoJSON geometry".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "bbox".to_string(),
                description: "Bounding box".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "properties".to_string(),
                description: "Additional properties".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "links".to_string(),
                description: "Related links".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
            MetadataFieldDef {
                name: "assets".to_string(),
                description: "Asset objects".to_string(),
                required: true,
                vocabulary: None,
                valid_values: None,
            },
        ];

        Self {
            standard: MetadataStandard::Stac,
            field_definitions,
            min_completeness_score: 90.0,
            check_vocabularies: true,
            check_citation: false,
        }
    }

    /// Returns ISO topic categories.
    fn iso_topic_categories() -> HashSet<String> {
        [
            "farming",
            "biota",
            "boundaries",
            "climatologyMeteorologyAtmosphere",
            "economy",
            "elevation",
            "environment",
            "geoscientificInformation",
            "health",
            "imageryBaseMapsEarthCover",
            "intelligenceMilitary",
            "inlandWaters",
            "location",
            "oceans",
            "planningCadastre",
            "society",
            "structure",
            "transportation",
            "utilitiesCommunication",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }
}

/// Metadata checker.
pub struct MetadataChecker {
    config: MetadataConfig,
}

impl MetadataChecker {
    /// Creates a new metadata checker with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: MetadataConfig::default(),
        }
    }

    /// Creates a new metadata checker with custom configuration.
    #[must_use]
    pub fn with_config(config: MetadataConfig) -> Self {
        Self { config }
    }

    /// Checks metadata completeness.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails.
    pub fn check(&self, metadata: &HashMap<String, String>) -> QcResult<MetadataResult> {
        let mut issues = Vec::new();
        let mut missing_required = Vec::new();
        let mut missing_optional = Vec::new();
        let mut vocabulary_violations = Vec::new();
        let mut required_fields_present = 0;
        let mut optional_fields_present = 0;

        // Check each field definition
        for field_def in &self.config.field_definitions {
            if let Some(value) = metadata.get(&field_def.name) {
                // Field is present
                if field_def.required {
                    required_fields_present += 1;
                } else {
                    optional_fields_present += 1;
                }

                // Check controlled vocabulary if applicable
                if self.config.check_vocabularies {
                    if let Some(ref valid_values) = field_def.valid_values {
                        if !valid_values.contains(value) {
                            vocabulary_violations.push(VocabularyViolation {
                                field_name: field_def.name.clone(),
                                value: value.clone(),
                                vocabulary: field_def
                                    .vocabulary
                                    .clone()
                                    .unwrap_or_else(|| "Unknown".to_string()),
                                valid_values: if valid_values.len() <= 10 {
                                    Some(valid_values.iter().cloned().collect())
                                } else {
                                    None
                                },
                                severity: Severity::Minor,
                            });
                        }
                    }
                }
            } else {
                // Field is missing
                let missing_field = MissingField {
                    field_name: field_def.name.clone(),
                    description: field_def.description.clone(),
                    required: field_def.required,
                    severity: if field_def.required {
                        Severity::Major
                    } else {
                        Severity::Warning
                    },
                };

                if field_def.required {
                    missing_required.push(missing_field);
                } else {
                    missing_optional.push(missing_field);
                }
            }
        }

        let total_required = self
            .config
            .field_definitions
            .iter()
            .filter(|f| f.required)
            .count();
        let required_fields_missing = missing_required.len();

        // Calculate completeness score
        let completeness_score = if total_required > 0 {
            (required_fields_present as f64 / total_required as f64) * 100.0
        } else {
            100.0
        };

        // Create issues for missing required fields
        for missing in &missing_required {
            issues.push(
                QcIssue::new(
                    missing.severity,
                    "metadata",
                    "Missing required metadata field",
                    format!(
                        "Required field '{}' is missing: {}",
                        missing.field_name, missing.description
                    ),
                )
                .with_suggestion("Add the required metadata field"),
            );
        }

        // Check completeness score threshold
        if completeness_score < self.config.min_completeness_score {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "metadata",
                    "Insufficient metadata completeness",
                    format!(
                        "Completeness score ({:.1}%) is below threshold ({:.1}%)",
                        completeness_score, self.config.min_completeness_score
                    ),
                )
                .with_suggestion("Complete all required metadata fields"),
            );
        }

        // Check vocabulary violations
        for violation in &vocabulary_violations {
            let valid_str = if let Some(ref valid) = violation.valid_values {
                format!(" Valid values: {:?}", valid)
            } else {
                String::new()
            };

            issues.push(
                QcIssue::new(
                    violation.severity,
                    "metadata",
                    "Controlled vocabulary violation",
                    format!(
                        "Field '{}' has invalid value '{}' for vocabulary '{}'.{}",
                        violation.field_name, violation.value, violation.vocabulary, valid_str
                    ),
                )
                .with_suggestion("Use a valid value from the controlled vocabulary"),
            );
        }

        // Check citation completeness if enabled
        let citation_completeness = if self.config.check_citation {
            Some(self.check_citation(metadata)?)
        } else {
            None
        };

        if let Some(ref citation) = citation_completeness {
            if citation.completeness_score < 60.0 {
                issues.push(
                    QcIssue::new(
                        Severity::Warning,
                        "metadata",
                        "Incomplete citation information",
                        format!(
                            "Citation completeness ({:.1}%) is low",
                            citation.completeness_score
                        ),
                    )
                    .with_suggestion("Add citation fields: title, authors, date, identifier"),
                );
            }
        }

        Ok(MetadataResult {
            standard: self.config.standard,
            completeness_score,
            required_fields_present,
            required_fields_missing,
            optional_fields_present,
            missing_required,
            missing_optional,
            vocabulary_violations,
            citation_completeness,
            issues,
        })
    }

    /// Checks citation completeness.
    fn check_citation(&self, metadata: &HashMap<String, String>) -> QcResult<CitationCompleteness> {
        let has_title = metadata.contains_key("title");
        let has_authors = metadata.contains_key("authors") || metadata.contains_key("creator");
        let has_date = metadata.contains_key("date")
            || metadata.contains_key("publication_date")
            || metadata.contains_key("created");
        let has_identifier = metadata.contains_key("doi") || metadata.contains_key("identifier");
        let has_publisher = metadata.contains_key("publisher");

        let mut score = 0.0;
        if has_title {
            score += 30.0;
        }
        if has_authors {
            score += 25.0;
        }
        if has_date {
            score += 20.0;
        }
        if has_identifier {
            score += 15.0;
        }
        if has_publisher {
            score += 10.0;
        }

        Ok(CitationCompleteness {
            has_title,
            has_authors,
            has_date,
            has_identifier,
            has_publisher,
            completeness_score: score,
        })
    }
}

impl Default for MetadataChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_checker_creation() {
        let checker = MetadataChecker::new();
        assert_eq!(checker.config.standard, MetadataStandard::Iso19115);
    }

    #[test]
    fn test_complete_metadata() {
        let checker = MetadataChecker::new();
        let mut metadata = HashMap::new();
        metadata.insert("title".to_string(), "Test Dataset".to_string());
        metadata.insert("abstract".to_string(), "Test description".to_string());
        metadata.insert("topic_category".to_string(), "elevation".to_string());
        metadata.insert("contact".to_string(), "test@example.com".to_string());
        metadata.insert("date".to_string(), "2024-01-01".to_string());
        metadata.insert("spatial_extent".to_string(), "-180,-90,180,90".to_string());

        let result = checker.check(&metadata);
        assert!(result.is_ok());

        #[allow(clippy::unwrap_used)]
        let result = result.expect("metadata check should succeed for complete metadata");
        assert_eq!(result.required_fields_missing, 0);
        assert!(result.completeness_score > 90.0);
    }

    #[test]
    fn test_incomplete_metadata() {
        let checker = MetadataChecker::new();
        let metadata = HashMap::new(); // Empty metadata

        let result = checker.check(&metadata);
        assert!(result.is_ok());

        #[allow(clippy::unwrap_used)]
        let result = result.expect("metadata check should succeed even for empty metadata");
        assert!(result.required_fields_missing > 0);
        assert!(result.completeness_score < 50.0);
        assert!(!result.issues.is_empty());
    }

    #[test]
    fn test_stac_config() {
        let config = MetadataConfig::stac_config();
        assert_eq!(config.standard, MetadataStandard::Stac);
        assert!(!config.field_definitions.is_empty());
    }

    #[test]
    fn test_vocabulary_validation() {
        let checker = MetadataChecker::new();
        let mut metadata = HashMap::new();
        metadata.insert("title".to_string(), "Test".to_string());
        metadata.insert("abstract".to_string(), "Test".to_string());
        metadata.insert("topic_category".to_string(), "invalid_category".to_string());
        metadata.insert("contact".to_string(), "test@example.com".to_string());
        metadata.insert("date".to_string(), "2024-01-01".to_string());
        metadata.insert("spatial_extent".to_string(), "-180,-90,180,90".to_string());

        let result = checker.check(&metadata);
        assert!(result.is_ok());

        #[allow(clippy::unwrap_used)]
        let result = result.expect("metadata check should succeed for vocabulary validation test");
        assert!(!result.vocabulary_violations.is_empty());
    }
}
