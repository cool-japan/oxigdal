//! Data validation utilities
//!
//! This module provides tools for validating geospatial data integrity,
//! format compliance, and correctness.

use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether validation passed
    pub passed: bool,
    /// Validation errors
    pub errors: Vec<ValidationError>,
    /// Validation warnings
    pub warnings: Vec<ValidationWarning>,
    /// Validation info
    pub info: Vec<String>,
}

/// Validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Error category
    pub category: ErrorCategory,
    /// Error message
    pub message: String,
    /// Error location (optional)
    pub location: Option<String>,
}

/// Validation warning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    /// Warning category
    pub category: WarningCategory,
    /// Warning message
    pub message: String,
}

/// Error category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCategory {
    /// Format error
    Format,
    /// Data integrity error
    Integrity,
    /// Metadata error
    Metadata,
    /// Projection error
    Projection,
    /// Bounds error
    Bounds,
}

/// Warning category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningCategory {
    /// Performance warning
    Performance,
    /// Compatibility warning
    Compatibility,
    /// Best practices warning
    BestPractices,
}

impl ValidationResult {
    /// Create a new validation result
    pub fn new() -> Self {
        Self {
            passed: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            info: Vec::new(),
        }
    }

    /// Add an error
    pub fn add_error(&mut self, category: ErrorCategory, message: impl Into<String>) {
        self.passed = false;
        self.errors.push(ValidationError {
            category,
            message: message.into(),
            location: None,
        });
    }

    /// Add a warning
    pub fn add_warning(&mut self, category: WarningCategory, message: impl Into<String>) {
        self.warnings.push(ValidationWarning {
            category,
            message: message.into(),
        });
    }

    /// Add info
    pub fn add_info(&mut self, message: impl Into<String>) {
        self.info.push(message.into());
    }

    /// Format as report
    pub fn report(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!("\n{}\n", "Validation Report".bold()));
        report.push_str(&format!("{}\n\n", "=".repeat(60)));

        let status = if self.passed {
            "PASSED".green().bold()
        } else {
            "FAILED".red().bold()
        };
        report.push_str(&format!("Status: {}\n\n", status));

        if !self.errors.is_empty() {
            report.push_str(&format!(
                "{} ({}):\n",
                "Errors".red().bold(),
                self.errors.len()
            ));
            for (i, error) in self.errors.iter().enumerate() {
                report.push_str(&format!(
                    "  {}. [{:?}] {}\n",
                    i + 1,
                    error.category,
                    error.message
                ));
            }
            report.push('\n');
        }

        if !self.warnings.is_empty() {
            report.push_str(&format!(
                "{} ({}):\n",
                "Warnings".yellow().bold(),
                self.warnings.len()
            ));
            for (i, warning) in self.warnings.iter().enumerate() {
                report.push_str(&format!(
                    "  {}. [{:?}] {}\n",
                    i + 1,
                    warning.category,
                    warning.message
                ));
            }
            report.push('\n');
        }

        if !self.info.is_empty() {
            report.push_str(&format!("{}:\n", "Info".cyan()));
            for info in &self.info {
                report.push_str(&format!("  - {}\n", info));
            }
        }

        report
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Data validator
pub struct DataValidator;

impl DataValidator {
    /// Validate raster dimensions
    pub fn validate_raster_dimensions(
        width: usize,
        height: usize,
        bands: usize,
    ) -> ValidationResult {
        let mut result = ValidationResult::new();

        if width == 0 {
            result.add_error(ErrorCategory::Format, "Width cannot be zero");
        }
        if height == 0 {
            result.add_error(ErrorCategory::Format, "Height cannot be zero");
        }
        if bands == 0 {
            result.add_error(ErrorCategory::Format, "Bands cannot be zero");
        }

        if width > 100000 || height > 100000 {
            result.add_warning(
                WarningCategory::Performance,
                format!("Large raster dimensions: {}x{}", width, height),
            );
        }

        result.add_info(format!("Dimensions: {}x{}x{}", width, height, bands));

        result
    }

    /// Validate bounds
    pub fn validate_bounds(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> ValidationResult {
        let mut result = ValidationResult::new();

        if min_x >= max_x {
            result.add_error(
                ErrorCategory::Bounds,
                format!("Invalid X bounds: min ({}) >= max ({})", min_x, max_x),
            );
        }

        if min_y >= max_y {
            result.add_error(
                ErrorCategory::Bounds,
                format!("Invalid Y bounds: min ({}) >= max ({})", min_y, max_y),
            );
        }

        if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
            result.add_error(ErrorCategory::Bounds, "Bounds contain non-finite values");
        }

        result.add_info(format!(
            "Bounds: [{}, {}] x [{}, {}]",
            min_x, min_y, max_x, max_y
        ));

        result
    }

    /// Validate data range
    pub fn validate_data_range(
        data: &[f64],
        expected_min: f64,
        expected_max: f64,
    ) -> ValidationResult {
        let mut result = ValidationResult::new();

        if data.is_empty() {
            result.add_error(ErrorCategory::Integrity, "Empty data array");
            return result;
        }

        let (actual_min, actual_max) = data
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), &v| {
                (min.min(v), max.max(v))
            });

        if actual_min < expected_min || actual_max > expected_max {
            result.add_warning(
                WarningCategory::BestPractices,
                format!(
                    "Data range [{}, {}] outside expected range [{}, {}]",
                    actual_min, actual_max, expected_min, expected_max
                ),
            );
        }

        let nan_count = data.iter().filter(|v| !v.is_finite()).count();
        if nan_count > 0 {
            result.add_warning(
                WarningCategory::BestPractices,
                format!("Found {} non-finite values", nan_count),
            );
        }

        result.add_info(format!("Data range: [{}, {}]", actual_min, actual_max));
        result.add_info(format!("Data count: {}", data.len()));

        result
    }

    /// Validate file path
    pub fn validate_file_path(path: &Path) -> ValidationResult {
        let mut result = ValidationResult::new();

        if !path.exists() {
            result.add_error(ErrorCategory::Format, "File does not exist");
            return result;
        }

        if !path.is_file() {
            result.add_error(ErrorCategory::Format, "Path is not a file");
            return result;
        }

        if let Ok(metadata) = path.metadata() {
            let size = metadata.len();
            result.add_info(format!("File size: {} bytes", size));

            if size == 0 {
                result.add_error(ErrorCategory::Format, "File is empty");
            }

            if size > 1_000_000_000 {
                result.add_warning(
                    WarningCategory::Performance,
                    format!("Large file size: {:.2} GB", size as f64 / 1e9),
                );
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_creation() {
        let result = ValidationResult::new();
        assert!(result.passed);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_validation_error() {
        let mut result = ValidationResult::new();
        result.add_error(ErrorCategory::Format, "test error");

        assert!(!result.passed);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_validate_raster_dimensions() {
        let result = DataValidator::validate_raster_dimensions(100, 100, 3);
        assert!(result.passed);

        let result = DataValidator::validate_raster_dimensions(0, 100, 3);
        assert!(!result.passed);
    }

    #[test]
    fn test_validate_bounds() {
        let result = DataValidator::validate_bounds(0.0, 0.0, 100.0, 100.0);
        assert!(result.passed);

        let result = DataValidator::validate_bounds(100.0, 0.0, 0.0, 100.0);
        assert!(!result.passed);
    }

    #[test]
    fn test_validate_data_range() {
        let data = vec![0.0, 50.0, 100.0];
        let result = DataValidator::validate_data_range(&data, 0.0, 100.0);
        assert!(result.passed);

        let data = vec![0.0, 150.0, 100.0];
        let result = DataValidator::validate_data_range(&data, 0.0, 100.0);
        assert!(result.passed);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn test_validate_empty_data() {
        let data: Vec<f64> = vec![];
        let result = DataValidator::validate_data_range(&data, 0.0, 100.0);
        assert!(!result.passed);
    }
}
