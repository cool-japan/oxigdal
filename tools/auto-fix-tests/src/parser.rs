use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Confidence level of failure classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Confidence {
    High,
    Medium,
    Low,
    None,
}

/// Priority level for fixing the failure
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    #[serde(rename = "P1_CRITICAL")]
    P1Critical,
    #[serde(rename = "P2_HIGH")]
    P2High,
    #[serde(rename = "P3_MEDIUM")]
    P3Medium,
    #[serde(rename = "P4_LOW")]
    P4Low,
    #[serde(rename = "P5_MANUAL")]
    P5Manual,
}

/// Auto-fix strategy recommendation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoFix {
    None,
    AddIgnore,
    AddShouldPanic,
    EnvCheck,
    AddTimeoutOrIgnore,
    SkipIfUnavailable,
}

/// Individual test failure from fail-tests.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFailure {
    pub test_name: String,
    pub package: String,
    pub file_path: String,
    pub duration: f64,
    pub category: String,
    pub subcategory: String,
    pub confidence: Confidence,
    pub auto_fix: AutoFix,
    pub priority: Priority,
    pub error_context: String,
    pub recommended_action: String,
}

impl TestFailure {
    /// Extract the function name from the test name (e.g., "module::test_fn" -> "test_fn")
    pub fn function_name(&self) -> &str {
        self.test_name.split("::").last().unwrap_or(&self.test_name)
    }

    /// Check if this failure should be auto-fixed
    pub fn is_auto_fixable(&self) -> bool {
        self.auto_fix != AutoFix::None
    }

    /// Get a short description of the fix strategy
    pub fn fix_description(&self) -> &str {
        match self.auto_fix {
            AutoFix::None => "No automatic fix available",
            AutoFix::AddIgnore => "Add #[ignore] attribute",
            AutoFix::AddShouldPanic => "Add #[should_panic] attribute",
            AutoFix::EnvCheck => "Add environment variable check",
            AutoFix::AddTimeoutOrIgnore => "Add timeout or ignore",
            AutoFix::SkipIfUnavailable => "Skip if resource unavailable",
        }
    }
}

/// Root structure of fail-tests.json
#[derive(Debug, Serialize, Deserialize)]
pub struct FailureReport {
    pub timestamp: String,
    pub total_failures: usize,
    pub by_confidence: HashMap<String, usize>,
    pub failures: Vec<TestFailure>,
}

impl FailureReport {
    /// Load failure report from JSON file
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let report: FailureReport = serde_json::from_str(&content)?;
        Ok(report)
    }

    /// Filter failures by minimum confidence level
    pub fn filter_by_confidence(&self, min_confidence: Confidence) -> Vec<&TestFailure> {
        self.failures
            .iter()
            .filter(|f| f.confidence >= min_confidence)
            .collect()
    }

    /// Filter failures that are auto-fixable
    pub fn auto_fixable_failures(&self) -> Vec<&TestFailure> {
        self.failures.iter().filter(|f| f.is_auto_fixable()).collect()
    }

    /// Group failures by package
    pub fn by_package(&self) -> HashMap<String, Vec<&TestFailure>> {
        let mut map: HashMap<String, Vec<&TestFailure>> = HashMap::new();
        for failure in &self.failures {
            map.entry(failure.package.clone())
                .or_default()
                .push(failure);
        }
        map
    }

    /// Get statistics summary
    pub fn summary(&self) -> String {
        format!(
            "Total: {} failures, Auto-fixable: {}, HIGH: {}, MEDIUM: {}, LOW: {}",
            self.total_failures,
            self.auto_fixable_failures().len(),
            self.by_confidence.get("HIGH").unwrap_or(&0),
            self.by_confidence.get("MEDIUM").unwrap_or(&0),
            self.by_confidence.get("LOW").unwrap_or(&0),
        )
    }
}
