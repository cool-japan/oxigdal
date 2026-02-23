//! Error types for quality control operations.

use std::fmt;

/// Result type for QC operations.
pub type QcResult<T> = Result<T, QcError>;

/// Errors that can occur during quality control operations.
#[derive(Debug, thiserror::Error)]
pub enum QcError {
    /// Invalid configuration error.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Invalid input data error.
    #[error("Invalid input data: {0}")]
    InvalidInput(String),

    /// Validation rule error.
    #[error("Validation rule error: {0}")]
    ValidationRule(String),

    /// Topology validation error.
    #[error("Topology error: {0}")]
    TopologyError(String),

    /// Attribute validation error.
    #[error("Attribute error: {0}")]
    AttributeError(String),

    /// Metadata validation error.
    #[error("Metadata error: {0}")]
    MetadataError(String),

    /// Raster validation error.
    #[error("Raster error: {0}")]
    RasterError(String),

    /// Report generation error.
    #[error("Report generation error: {0}")]
    ReportError(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML parsing error.
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    /// Core library error.
    #[error("Core error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),

    /// Algorithm error.
    #[error("Algorithm error: {0}")]
    Algorithm(#[from] oxigdal_algorithms::error::AlgorithmError),

    /// Numerical computation error.
    #[error("Numerical error: {0}")]
    Numerical(String),

    /// Geometry fix error.
    #[error("Fix error: {0}")]
    FixError(String),

    /// Threshold exceeded error.
    #[error("Threshold exceeded: {0}")]
    ThresholdExceeded(String),

    /// Unsupported operation error.
    #[error("Unsupported operation: {0}")]
    Unsupported(String),
}

/// Severity level for quality control issues.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational message, no action required.
    Info,

    /// Warning, data is usable but may have minor issues.
    Warning,

    /// Minor issue, data is usable but should be reviewed.
    Minor,

    /// Major issue, data quality is compromised.
    Major,

    /// Critical issue, data is unusable or seriously compromised.
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARNING"),
            Self::Minor => write!(f, "MINOR"),
            Self::Major => write!(f, "MAJOR"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

impl Severity {
    /// Returns the color code for HTML reporting.
    pub fn color(&self) -> &'static str {
        match self {
            Self::Info => "#17a2b8",     // cyan
            Self::Warning => "#ffc107",  // yellow
            Self::Minor => "#fd7e14",    // orange
            Self::Major => "#dc3545",    // red
            Self::Critical => "#721c24", // dark red
        }
    }

    /// Returns the emoji for console reporting.
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Info => "ℹ️",
            Self::Warning => "⚠️",
            Self::Minor => "⚡",
            Self::Major => "❌",
            Self::Critical => "🚨",
        }
    }
}

/// Quality control issue details.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QcIssue {
    /// Issue severity level.
    pub severity: Severity,

    /// Category of the issue.
    pub category: String,

    /// Short description of the issue.
    pub description: String,

    /// Detailed message about the issue.
    pub message: String,

    /// Location information (e.g., feature ID, pixel coordinates).
    pub location: Option<String>,

    /// Suggested fix or remediation.
    pub suggestion: Option<String>,

    /// Rule ID that detected this issue.
    pub rule_id: Option<String>,

    /// Timestamp when the issue was detected.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl QcIssue {
    /// Creates a new quality control issue.
    pub fn new(
        severity: Severity,
        category: impl Into<String>,
        description: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            category: category.into(),
            description: description.into(),
            message: message.into(),
            location: None,
            suggestion: None,
            rule_id: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Sets the location information.
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Sets the suggestion for fixing the issue.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Sets the rule ID.
    pub fn with_rule_id(mut self, rule_id: impl Into<String>) -> Self {
        self.rule_id = Some(rule_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Minor);
        assert!(Severity::Minor < Severity::Major);
        assert!(Severity::Major < Severity::Critical);
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(Severity::Info.to_string(), "INFO");
        assert_eq!(Severity::Warning.to_string(), "WARNING");
        assert_eq!(Severity::Minor.to_string(), "MINOR");
        assert_eq!(Severity::Major.to_string(), "MAJOR");
        assert_eq!(Severity::Critical.to_string(), "CRITICAL");
    }

    #[test]
    fn test_severity_color() {
        assert_eq!(Severity::Info.color(), "#17a2b8");
        assert_eq!(Severity::Critical.color(), "#721c24");
    }

    #[test]
    fn test_qc_issue_creation() {
        let issue = QcIssue::new(
            Severity::Major,
            "topology",
            "Invalid geometry",
            "Polygon has self-intersection",
        )
        .with_location("feature_123")
        .with_suggestion("Use ST_MakeValid to repair")
        .with_rule_id("TOPO-001");

        assert_eq!(issue.severity, Severity::Major);
        assert_eq!(issue.category, "topology");
        assert_eq!(issue.description, "Invalid geometry");
        assert_eq!(issue.message, "Polygon has self-intersection");
        assert_eq!(issue.location, Some("feature_123".to_string()));
        assert_eq!(
            issue.suggestion,
            Some("Use ST_MakeValid to repair".to_string())
        );
        assert_eq!(issue.rule_id, Some("TOPO-001".to_string()));
    }

    #[test]
    fn test_qc_issue_serialization() {
        let issue = QcIssue::new(
            Severity::Warning,
            "attribute",
            "Missing field",
            "Required field 'name' is missing",
        );

        let json = serde_json::to_string(&issue).ok();
        assert!(json.is_some());

        let deserialized: Result<QcIssue, _> = serde_json::from_str(&json.unwrap_or_default());
        assert!(deserialized.is_ok());
    }
}
