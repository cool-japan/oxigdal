//! Quality rules engine for configurable validation.
//!
//! This module provides a rules engine for defining and executing
//! custom quality control rules.

use crate::error::{QcError, QcIssue, QcResult, Severity};
use std::collections::HashMap;

/// Quality rule definition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QualityRule {
    /// Unique rule identifier.
    pub id: String,

    /// Rule name.
    pub name: String,

    /// Rule description.
    pub description: String,

    /// Rule category.
    pub category: RuleCategory,

    /// Rule severity if violated.
    pub severity: Severity,

    /// Rule priority (higher priority rules run first).
    pub priority: i32,

    /// Rule type.
    pub rule_type: RuleType,

    /// Rule configuration.
    pub config: RuleConfig,

    /// Whether the rule is enabled.
    pub enabled: bool,
}

/// Rule category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RuleCategory {
    /// Raster data rules.
    Raster,

    /// Vector data rules.
    Vector,

    /// Metadata rules.
    Metadata,

    /// Topology rules.
    Topology,

    /// Attribution rules.
    Attribution,

    /// General rules.
    General,
}

/// Type of quality rule.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RuleType {
    /// Threshold-based rule.
    Threshold {
        /// Field to check.
        field: String,
        /// Comparison operator.
        operator: ComparisonOperator,
        /// Threshold value.
        value: f64,
    },

    /// Range validation rule.
    Range {
        /// Field to check.
        field: String,
        /// Minimum value.
        min: f64,
        /// Maximum value.
        max: f64,
    },

    /// Enumeration validation.
    Enumeration {
        /// Field to check.
        field: String,
        /// Allowed values.
        allowed_values: Vec<String>,
    },

    /// Pattern matching rule (regex).
    Pattern {
        /// Field to check.
        field: String,
        /// Pattern to match.
        pattern: String,
    },

    /// Custom validation function.
    Custom {
        /// Function name.
        function_name: String,
    },
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ComparisonOperator {
    /// Equal to.
    Equal,
    /// Not equal to.
    NotEqual,
    /// Greater than.
    GreaterThan,
    /// Greater than or equal to.
    GreaterThanOrEqual,
    /// Less than.
    LessThan,
    /// Less than or equal to.
    LessThanOrEqual,
}

/// Rule configuration parameters.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RuleConfig {
    /// Additional configuration parameters.
    pub parameters: HashMap<String, String>,

    /// Pass threshold (percentage).
    pub pass_threshold: Option<f64>,

    /// Fail threshold (percentage).
    pub fail_threshold: Option<f64>,
}

/// Rule set containing multiple rules.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuleSet {
    /// Rule set name.
    pub name: String,

    /// Rule set description.
    pub description: String,

    /// Version of the rule set.
    pub version: String,

    /// Rules in the set.
    pub rules: Vec<QualityRule>,
}

impl RuleSet {
    /// Creates a new empty rule set.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            version: "1.0".to_string(),
            rules: Vec::new(),
        }
    }

    /// Adds a rule to the rule set.
    pub fn add_rule(&mut self, rule: QualityRule) {
        self.rules.push(rule);
    }

    /// Loads a rule set from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn from_toml_file(path: impl AsRef<std::path::Path>) -> QcResult<Self> {
        let content = std::fs::read_to_string(path).map_err(QcError::Io)?;
        let ruleset: RuleSet = toml::from_str(&content)?;
        Ok(ruleset)
    }

    /// Saves the rule set to a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn to_toml_file(&self, path: impl AsRef<std::path::Path>) -> QcResult<()> {
        let content = toml::to_string_pretty(self).map_err(|e| {
            QcError::InvalidConfiguration(format!("Failed to serialize rule set: {}", e))
        })?;
        std::fs::write(path, content).map_err(QcError::Io)?;
        Ok(())
    }

    /// Gets enabled rules sorted by priority.
    #[must_use]
    pub fn get_enabled_rules(&self) -> Vec<&QualityRule> {
        let mut rules: Vec<&QualityRule> = self.rules.iter().filter(|r| r.enabled).collect();
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        rules
    }

    /// Gets rules by category.
    #[must_use]
    pub fn get_rules_by_category(&self, category: RuleCategory) -> Vec<&QualityRule> {
        self.rules
            .iter()
            .filter(|r| r.category == category)
            .collect()
    }
}

/// Rules engine for executing quality rules.
pub struct RulesEngine {
    rule_set: RuleSet,
}

impl RulesEngine {
    /// Creates a new rules engine with the given rule set.
    #[must_use]
    pub fn new(rule_set: RuleSet) -> Self {
        Self { rule_set }
    }

    /// Creates a rules engine from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn from_toml_file(path: impl AsRef<std::path::Path>) -> QcResult<Self> {
        let rule_set = RuleSet::from_toml_file(path)?;
        Ok(Self::new(rule_set))
    }

    /// Executes a specific rule.
    ///
    /// # Errors
    ///
    /// Returns an error if rule execution fails.
    pub fn execute_rule(
        &self,
        rule: &QualityRule,
        data: &HashMap<String, f64>,
    ) -> QcResult<Option<QcIssue>> {
        if !rule.enabled {
            return Ok(None);
        }

        let violated = match &rule.rule_type {
            RuleType::Threshold {
                field,
                operator,
                value,
            } => {
                if let Some(&field_value) = data.get(field) {
                    !self.compare_values(field_value, *value, *operator)
                } else {
                    true // Field missing
                }
            }
            RuleType::Range { field, min, max } => {
                if let Some(&field_value) = data.get(field) {
                    field_value < *min || field_value > *max
                } else {
                    true // Field missing
                }
            }
            RuleType::Enumeration { .. } => {
                // Would need string data for enumeration checks
                false
            }
            RuleType::Pattern { .. } => {
                // Would need string data for pattern checks
                false
            }
            RuleType::Custom { .. } => {
                // Custom functions would be registered separately
                false
            }
        };

        if violated {
            Ok(Some(
                QcIssue::new(
                    rule.severity,
                    format!("{:?}", rule.category).to_lowercase(),
                    &rule.name,
                    format!("{}: Rule violated", rule.description),
                )
                .with_rule_id(&rule.id),
            ))
        } else {
            Ok(None)
        }
    }

    /// Executes all enabled rules in the rule set.
    ///
    /// # Errors
    ///
    /// Returns an error if rule execution fails.
    pub fn execute_all(&self, data: &HashMap<String, f64>) -> QcResult<Vec<QcIssue>> {
        let mut issues = Vec::new();

        for rule in self.rule_set.get_enabled_rules() {
            if let Some(issue) = self.execute_rule(rule, data)? {
                issues.push(issue);
            }
        }

        Ok(issues)
    }

    /// Executes rules for a specific category.
    ///
    /// # Errors
    ///
    /// Returns an error if rule execution fails.
    pub fn execute_category(
        &self,
        category: RuleCategory,
        data: &HashMap<String, f64>,
    ) -> QcResult<Vec<QcIssue>> {
        let mut issues = Vec::new();

        for rule in self.rule_set.get_rules_by_category(category) {
            if let Some(issue) = self.execute_rule(rule, data)? {
                issues.push(issue);
            }
        }

        Ok(issues)
    }

    /// Returns the rule set.
    #[must_use]
    pub const fn rule_set(&self) -> &RuleSet {
        &self.rule_set
    }

    fn compare_values(&self, a: f64, b: f64, op: ComparisonOperator) -> bool {
        match op {
            ComparisonOperator::Equal => (a - b).abs() < f64::EPSILON,
            ComparisonOperator::NotEqual => (a - b).abs() >= f64::EPSILON,
            ComparisonOperator::GreaterThan => a > b,
            ComparisonOperator::GreaterThanOrEqual => a >= b,
            ComparisonOperator::LessThan => a < b,
            ComparisonOperator::LessThanOrEqual => a <= b,
        }
    }
}

/// Builder for creating quality rules.
pub struct RuleBuilder {
    rule: QualityRule,
}

impl RuleBuilder {
    /// Creates a new rule builder.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            rule: QualityRule {
                id: id.into(),
                name: name.into(),
                description: String::new(),
                category: RuleCategory::General,
                severity: Severity::Warning,
                priority: 0,
                rule_type: RuleType::Custom {
                    function_name: "default".to_string(),
                },
                config: RuleConfig::default(),
                enabled: true,
            },
        }
    }

    /// Sets the rule description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.rule.description = description.into();
        self
    }

    /// Sets the rule category.
    #[must_use]
    pub const fn category(mut self, category: RuleCategory) -> Self {
        self.rule.category = category;
        self
    }

    /// Sets the rule severity.
    #[must_use]
    pub const fn severity(mut self, severity: Severity) -> Self {
        self.rule.severity = severity;
        self
    }

    /// Sets the rule priority.
    #[must_use]
    pub const fn priority(mut self, priority: i32) -> Self {
        self.rule.priority = priority;
        self
    }

    /// Sets the rule type to threshold.
    #[must_use]
    pub fn threshold(
        mut self,
        field: impl Into<String>,
        operator: ComparisonOperator,
        value: f64,
    ) -> Self {
        self.rule.rule_type = RuleType::Threshold {
            field: field.into(),
            operator,
            value,
        };
        self
    }

    /// Sets the rule type to range.
    #[must_use]
    pub fn range(mut self, field: impl Into<String>, min: f64, max: f64) -> Self {
        self.rule.rule_type = RuleType::Range {
            field: field.into(),
            min,
            max,
        };
        self
    }

    /// Builds the rule.
    #[must_use]
    pub fn build(self) -> QualityRule {
        self.rule
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_builder() {
        let rule = RuleBuilder::new("TEST-001", "Test Rule")
            .description("Test description")
            .category(RuleCategory::Raster)
            .severity(Severity::Major)
            .priority(10)
            .threshold("field1", ComparisonOperator::GreaterThan, 100.0)
            .build();

        assert_eq!(rule.id, "TEST-001");
        assert_eq!(rule.name, "Test Rule");
        assert_eq!(rule.category, RuleCategory::Raster);
        assert_eq!(rule.severity, Severity::Major);
        assert_eq!(rule.priority, 10);
    }

    #[test]
    fn test_rule_set() {
        let mut ruleset = RuleSet::new("Test Rules", "Test rule set");

        let rule = RuleBuilder::new("R001", "Rule 1")
            .threshold("value", ComparisonOperator::LessThan, 50.0)
            .build();

        ruleset.add_rule(rule);
        assert_eq!(ruleset.rules.len(), 1);
    }

    #[test]
    fn test_rules_engine() {
        let mut ruleset = RuleSet::new("Test", "Test");

        let rule = RuleBuilder::new("R001", "Max Value Check")
            .threshold("max_value", ComparisonOperator::LessThanOrEqual, 100.0)
            .severity(Severity::Major)
            .build();

        ruleset.add_rule(rule);

        let engine = RulesEngine::new(ruleset);

        let mut data = HashMap::new();
        data.insert("max_value".to_string(), 150.0);

        let result = engine.execute_all(&data);
        assert!(result.is_ok());

        let issues = result.ok().unwrap_or_default();
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn test_comparison_operators() {
        let engine = RulesEngine::new(RuleSet::new("Test", "Test"));

        assert!(engine.compare_values(10.0, 5.0, ComparisonOperator::GreaterThan));
        assert!(engine.compare_values(5.0, 10.0, ComparisonOperator::LessThan));
        assert!(engine.compare_values(10.0, 10.0, ComparisonOperator::Equal));
        assert!(engine.compare_values(10.0, 5.0, ComparisonOperator::NotEqual));
    }
}
