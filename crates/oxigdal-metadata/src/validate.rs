//! Metadata validation and quality assessment.
//!
//! This module provides comprehensive validation for metadata standards,
//! including completeness checking, required field validation, controlled
//! vocabulary validation, and quality scoring.

use crate::datacite::DataCiteMetadata;
use crate::dcat::Dataset as DcatDataset;
use crate::error::Result;
use crate::fgdc::FgdcMetadata;
use crate::inspire::InspireMetadata;
use crate::iso19115::Iso19115Metadata;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Validation report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Whether the metadata is valid
    pub is_valid: bool,
    /// Validation errors (mandatory fields/constraints)
    pub errors: Vec<ValidationError>,
    /// Validation warnings (recommended fields)
    pub warnings: Vec<ValidationWarning>,
    /// Quality score (0-100)
    pub quality_score: f32,
    /// Completeness percentage (0-100)
    pub completeness: f32,
    /// Missing required fields
    pub missing_required: Vec<String>,
    /// Missing recommended fields
    pub missing_recommended: Vec<String>,
}

impl Default for ValidationReport {
    fn default() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            quality_score: 0.0,
            completeness: 0.0,
            missing_required: Vec::new(),
            missing_recommended: Vec::new(),
        }
    }
}

impl ValidationReport {
    /// Check if metadata is complete (all required fields present).
    pub fn is_complete(&self) -> bool {
        self.missing_required.is_empty() && self.is_valid
    }

    /// Get list of all missing fields.
    pub fn missing_fields(&self) -> Vec<String> {
        let mut fields = self.missing_required.clone();
        fields.extend(self.missing_recommended.clone());
        fields
    }

    /// Calculate overall quality score.
    pub fn calculate_quality_score(&mut self) {
        // Base score from completeness
        let mut score = self.completeness;

        // Penalize for errors
        score -= self.errors.len() as f32 * 5.0;

        // Penalize for warnings
        score -= self.warnings.len() as f32 * 2.0;

        // Clamp to 0-100
        self.quality_score = score.clamp(0.0, 100.0);
    }
}

/// Validation error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Field name
    pub field: String,
    /// Error message
    pub message: String,
    /// Error severity
    pub severity: ErrorSeverity,
}

/// Error severity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ErrorSeverity {
    /// Critical error
    Critical,
    /// Error
    Error,
}

/// Validation warning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    /// Field name
    pub field: String,
    /// Warning message
    pub message: String,
}

/// Validate ISO 19115 metadata.
pub fn validate_iso19115(metadata: &Iso19115Metadata) -> Result<ValidationReport> {
    let mut report = ValidationReport::default();

    // Check mandatory fields
    if metadata.identification_info.is_empty() {
        report.errors.push(ValidationError {
            field: "identification_info".to_string(),
            message: "At least one identification info is required".to_string(),
            severity: ErrorSeverity::Critical,
        });
        report
            .missing_required
            .push("identification_info".to_string());
    } else {
        // Validate first identification info
        let ident = &metadata.identification_info[0];

        if ident.citation.title.is_empty() {
            report.errors.push(ValidationError {
                field: "identification_info.citation.title".to_string(),
                message: "Title is required".to_string(),
                severity: ErrorSeverity::Critical,
            });
            report.missing_required.push("title".to_string());
        }

        if ident.abstract_text.is_empty() {
            report.errors.push(ValidationError {
                field: "identification_info.abstract".to_string(),
                message: "Abstract is required".to_string(),
                severity: ErrorSeverity::Critical,
            });
            report.missing_required.push("abstract".to_string());
        }

        // Check recommended fields
        if ident.keywords.is_empty() {
            report.warnings.push(ValidationWarning {
                field: "keywords".to_string(),
                message: "Keywords are recommended".to_string(),
            });
            report.missing_recommended.push("keywords".to_string());
        }

        if ident.extent.geographic_extent.is_none() {
            report.warnings.push(ValidationWarning {
                field: "extent.geographic_extent".to_string(),
                message: "Geographic extent is recommended".to_string(),
            });
            report
                .missing_recommended
                .push("geographic_extent".to_string());
        }

        if ident.point_of_contact.is_empty() {
            report.warnings.push(ValidationWarning {
                field: "point_of_contact".to_string(),
                message: "Point of contact is recommended".to_string(),
            });
            report
                .missing_recommended
                .push("point_of_contact".to_string());
        }
    }

    if metadata.contact.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "contact".to_string(),
            message: "Metadata contact is recommended".to_string(),
        });
        report
            .missing_recommended
            .push("metadata_contact".to_string());
    }

    // Validate bounding box if present
    if let Some(ident) = metadata.identification_info.first() {
        if let Some(bbox) = ident.extent.geographic_extent {
            if !bbox.is_valid() {
                report.errors.push(ValidationError {
                    field: "extent.geographic_extent".to_string(),
                    message: "Invalid bounding box coordinates".to_string(),
                    severity: ErrorSeverity::Error,
                });
            }
        }
    }

    // Calculate completeness
    let total_fields = 10; // Number of important fields to check
    let present_fields =
        total_fields - report.missing_required.len() - report.missing_recommended.len();
    report.completeness = (present_fields as f32 / total_fields as f32) * 100.0;

    // Set validity
    report.is_valid = report.errors.is_empty();

    // Calculate quality score
    report.calculate_quality_score();

    Ok(report)
}

/// Validate FGDC metadata.
pub fn validate_fgdc(metadata: &FgdcMetadata) -> Result<ValidationReport> {
    let mut report = ValidationReport::default();

    // Check mandatory fields
    if metadata.idinfo.citation.citeinfo.title.is_empty() {
        report.errors.push(ValidationError {
            field: "idinfo.citation.citeinfo.title".to_string(),
            message: "Title is required".to_string(),
            severity: ErrorSeverity::Critical,
        });
        report.missing_required.push("title".to_string());
    }

    if metadata.idinfo.descript.abstract_text.is_empty() {
        report.errors.push(ValidationError {
            field: "idinfo.descript.abstract".to_string(),
            message: "Abstract is required".to_string(),
            severity: ErrorSeverity::Critical,
        });
        report.missing_required.push("abstract".to_string());
    }

    // Check recommended fields
    if metadata.idinfo.keywords.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "idinfo.keywords".to_string(),
            message: "Keywords are recommended".to_string(),
        });
        report.missing_recommended.push("keywords".to_string());
    }

    if metadata.idinfo.ptcontac.is_none() {
        report.warnings.push(ValidationWarning {
            field: "idinfo.ptcontac".to_string(),
            message: "Point of contact is recommended".to_string(),
        });
        report
            .missing_recommended
            .push("point_of_contact".to_string());
    }

    if metadata.dataqual.is_none() {
        report.warnings.push(ValidationWarning {
            field: "dataqual".to_string(),
            message: "Data quality information is recommended".to_string(),
        });
        report.missing_recommended.push("data_quality".to_string());
    }

    // Validate bounding box
    let bbox = &metadata.idinfo.spdom.bounding;
    if !bbox.is_valid() {
        report.errors.push(ValidationError {
            field: "idinfo.spdom.bounding".to_string(),
            message: "Invalid bounding box coordinates".to_string(),
            severity: ErrorSeverity::Error,
        });
    }

    // Calculate completeness
    let total_fields = 8;
    let present_fields =
        total_fields - report.missing_required.len() - report.missing_recommended.len();
    report.completeness = (present_fields as f32 / total_fields as f32) * 100.0;

    report.is_valid = report.errors.is_empty();
    report.calculate_quality_score();

    Ok(report)
}

/// Validate INSPIRE metadata.
pub fn validate_inspire(metadata: &InspireMetadata) -> Result<ValidationReport> {
    let inspire_report = metadata.validate()?;

    // Convert inspire::ValidationReport to validate::ValidationReport
    let errors = inspire_report
        .errors
        .into_iter()
        .map(|e| ValidationError {
            field: "unknown".to_string(),
            message: e,
            severity: ErrorSeverity::Error,
        })
        .collect();
    let warnings = inspire_report
        .warnings
        .into_iter()
        .map(|w| ValidationWarning {
            field: "unknown".to_string(),
            message: w,
        })
        .collect();

    let mut report = ValidationReport {
        is_valid: inspire_report.is_valid,
        errors,
        warnings,
        ..Default::default()
    };
    report.calculate_quality_score();

    Ok(report)
}

/// Validate DataCite metadata.
pub fn validate_datacite(metadata: &DataCiteMetadata) -> Result<ValidationReport> {
    let mut report = ValidationReport::default();

    // DataCite has strict requirements - most are already enforced by the builder
    // But we can check for recommended fields

    if metadata.subjects.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "subjects".to_string(),
            message: "Subjects are recommended".to_string(),
        });
        report.missing_recommended.push("subjects".to_string());
    }

    if metadata.descriptions.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "descriptions".to_string(),
            message: "Descriptions are recommended".to_string(),
        });
        report.missing_recommended.push("descriptions".to_string());
    }

    if metadata.related_identifiers.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "related_identifiers".to_string(),
            message: "Related identifiers are recommended".to_string(),
        });
        report
            .missing_recommended
            .push("related_identifiers".to_string());
    }

    if metadata.geo_locations.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "geo_locations".to_string(),
            message: "Geo locations are recommended for geospatial data".to_string(),
        });
        report.missing_recommended.push("geo_locations".to_string());
    }

    // Calculate completeness
    let total_fields = 6;
    let present_fields = total_fields - report.missing_recommended.len();
    report.completeness = (present_fields as f32 / total_fields as f32) * 100.0;

    report.is_valid = true; // If it was built, it's valid
    report.calculate_quality_score();

    Ok(report)
}

/// Validate DCAT dataset.
pub fn validate_dcat(dataset: &DcatDataset) -> Result<ValidationReport> {
    let mut report = ValidationReport::default();

    // DCAT mandatory fields are enforced by builder
    // Check recommended fields

    if dataset.keyword.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "keyword".to_string(),
            message: "Keywords are recommended".to_string(),
        });
        report.missing_recommended.push("keyword".to_string());
    }

    if dataset.theme.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "theme".to_string(),
            message: "Theme is recommended".to_string(),
        });
        report.missing_recommended.push("theme".to_string());
    }

    if dataset.contact_point.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "contact_point".to_string(),
            message: "Contact point is recommended".to_string(),
        });
        report.missing_recommended.push("contact_point".to_string());
    }

    if dataset.distribution.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "distribution".to_string(),
            message: "At least one distribution is recommended".to_string(),
        });
        report.missing_recommended.push("distribution".to_string());
    }

    if dataset.spatial.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "spatial".to_string(),
            message: "Spatial coverage is recommended".to_string(),
        });
        report.missing_recommended.push("spatial".to_string());
    }

    if dataset.temporal.is_empty() {
        report.warnings.push(ValidationWarning {
            field: "temporal".to_string(),
            message: "Temporal coverage is recommended".to_string(),
        });
        report.missing_recommended.push("temporal".to_string());
    }

    // Calculate completeness
    let total_fields = 8;
    let present_fields = total_fields - report.missing_recommended.len();
    report.completeness = (present_fields as f32 / total_fields as f32) * 100.0;

    report.is_valid = true;
    report.calculate_quality_score();

    Ok(report)
}

/// Controlled vocabulary validator.
pub struct VocabularyValidator {
    /// Valid terms
    terms: HashSet<String>,
    /// Case sensitive
    case_sensitive: bool,
}

impl VocabularyValidator {
    /// Create a new vocabulary validator.
    pub fn new(terms: Vec<String>, case_sensitive: bool) -> Self {
        let terms = if case_sensitive {
            terms.into_iter().collect()
        } else {
            terms.into_iter().map(|t| t.to_lowercase()).collect()
        };

        Self {
            terms,
            case_sensitive,
        }
    }

    /// Validate a term.
    pub fn validate(&self, term: &str) -> bool {
        let term = if self.case_sensitive {
            term.to_string()
        } else {
            term.to_lowercase()
        };

        self.terms.contains(&term)
    }

    /// Get suggestions for an invalid term.
    pub fn suggest(&self, term: &str) -> Vec<String> {
        let term_lower = term.to_lowercase();
        self.terms
            .iter()
            .filter(|t| t.contains(&term_lower) || term_lower.contains(t.as_str()))
            .cloned()
            .collect()
    }
}

/// Cross-reference validator for related metadata.
pub struct CrossReferenceValidator;

impl CrossReferenceValidator {
    /// Validate that related identifiers exist.
    pub fn validate_related_identifiers(
        _metadata: &DataCiteMetadata,
        _resolver: impl Fn(&str) -> bool,
    ) -> Result<Vec<String>> {
        // Placeholder for cross-reference validation
        // Would check if related DOIs/identifiers are valid and resolvable
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vocabulary_validator() {
        let vocab = VocabularyValidator::new(
            vec![
                "dataset".to_string(),
                "service".to_string(),
                "series".to_string(),
            ],
            false,
        );

        assert!(vocab.validate("Dataset"));
        assert!(vocab.validate("SERVICE"));
        assert!(!vocab.validate("unknown"));
    }

    #[test]
    fn test_vocabulary_suggestions() {
        let vocab = VocabularyValidator::new(
            vec!["elevation".to_string(), "temperature".to_string()],
            false,
        );

        let suggestions = vocab.suggest("elev");
        assert!(suggestions.contains(&"elevation".to_string()));
    }

    #[test]
    fn test_validation_report_quality_score() {
        let mut report = ValidationReport {
            completeness: 80.0,
            ..Default::default()
        };
        report.errors.push(ValidationError {
            field: "test".to_string(),
            message: "test error".to_string(),
            severity: ErrorSeverity::Error,
        });
        report.calculate_quality_score();

        // 80 - 5 = 75
        assert!((report.quality_score - 75.0).abs() < 0.1);
    }
}
