//! Quality rules engine for configurable validation.
//!
//! This module provides a rules engine for defining and executing
//! custom quality control rules.

use crate::error::{QcError, QcIssue, QcResult, Severity};
use std::collections::HashMap;

/// A typed data value used when executing rules against feature/record data.
///
/// `Threshold`/`Range` rules operate on [`QcValue::Number`]; `Enumeration`/
/// `Pattern` rules operate on [`QcValue::Text`]. A field present with the
/// "wrong" variant for a given rule type is treated the same as a missing
/// field (the rule cannot be evaluated, so it is reported as violated).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum QcValue {
    /// A numeric measurement.
    Number(f64),
    /// A text/string value.
    Text(String),
}

impl QcValue {
    /// Returns the numeric value, if this is a [`QcValue::Number`].
    #[must_use]
    pub const fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Text(_) => None,
        }
    }

    /// Returns the text value, if this is a [`QcValue::Text`].
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            Self::Number(_) => None,
        }
    }
}

impl From<f64> for QcValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<String> for QcValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for QcValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

/// A registered handler for [`RuleType::Custom`] rules.
///
/// Returns `true` when the rule is violated for the given data row.
type CustomRuleFn = Box<dyn Fn(&HashMap<String, QcValue>) -> bool + Send + Sync>;

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
        rules.sort_by_key(|x| std::cmp::Reverse(x.priority));
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
    custom_fns: HashMap<String, CustomRuleFn>,
}

impl RulesEngine {
    /// Creates a new rules engine with the given rule set.
    #[must_use]
    pub fn new(rule_set: RuleSet) -> Self {
        Self {
            rule_set,
            custom_fns: HashMap::new(),
        }
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

    /// Registers a handler for a [`RuleType::Custom`] rule.
    ///
    /// `function_name` must match the `function_name` configured on the
    /// `Custom` rule. The handler receives the row's data and returns `true`
    /// when the rule is violated. Calling `execute_rule`/`execute_all`/
    /// `execute_category` on a `Custom` rule whose `function_name` has no
    /// registered handler returns [`QcError::InvalidConfiguration`] instead
    /// of silently treating the rule as always-passing.
    pub fn register_custom_fn<F>(&mut self, function_name: impl Into<String>, handler: F)
    where
        F: Fn(&HashMap<String, QcValue>) -> bool + Send + Sync + 'static,
    {
        self.custom_fns
            .insert(function_name.into(), Box::new(handler));
    }

    /// Executes a specific rule.
    ///
    /// # Errors
    ///
    /// Returns [`QcError::InvalidConfiguration`] if the rule is a `Pattern`
    /// rule with an invalid regex, or a `Custom` rule whose function name
    /// has no registered handler.
    pub fn execute_rule(
        &self,
        rule: &QualityRule,
        data: &HashMap<String, QcValue>,
    ) -> QcResult<Option<QcIssue>> {
        if !rule.enabled {
            return Ok(None);
        }

        let violated = match &rule.rule_type {
            RuleType::Threshold {
                field,
                operator,
                value,
            } => match data.get(field).and_then(QcValue::as_number) {
                Some(field_value) => !self.compare_values(field_value, *value, *operator),
                None => true, // Field missing or not numeric
            },
            RuleType::Range { field, min, max } => {
                match data.get(field).and_then(QcValue::as_number) {
                    Some(field_value) => field_value < *min || field_value > *max,
                    None => true, // Field missing or not numeric
                }
            }
            RuleType::Enumeration {
                field,
                allowed_values,
            } => match data.get(field).and_then(QcValue::as_text) {
                Some(text) => !allowed_values.iter().any(|allowed| allowed == text),
                None => true, // Field missing or not text
            },
            RuleType::Pattern { field, pattern } => {
                match data.get(field).and_then(QcValue::as_text) {
                    Some(text) => {
                        let re = regex::Regex::new(pattern).map_err(|e| {
                            QcError::InvalidConfiguration(format!(
                                "Rule '{}': invalid pattern '{}': {e}",
                                rule.id, pattern
                            ))
                        })?;
                        !re.is_match(text)
                    }
                    None => true, // Field missing or not text
                }
            }
            RuleType::Custom { function_name } => match self.custom_fns.get(function_name) {
                Some(handler) => handler(data),
                None => {
                    return Err(QcError::InvalidConfiguration(format!(
                        "Rule '{}': custom function '{}' is not registered",
                        rule.id, function_name
                    )));
                }
            },
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
    pub fn execute_all(&self, data: &HashMap<String, QcValue>) -> QcResult<Vec<QcIssue>> {
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
        data: &HashMap<String, QcValue>,
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

    /// Sets the rule type to enumeration.
    #[must_use]
    pub fn enumeration(mut self, field: impl Into<String>, allowed_values: Vec<String>) -> Self {
        self.rule.rule_type = RuleType::Enumeration {
            field: field.into(),
            allowed_values,
        };
        self
    }

    /// Sets the rule type to pattern (regex).
    #[must_use]
    pub fn pattern(mut self, field: impl Into<String>, pattern: impl Into<String>) -> Self {
        self.rule.rule_type = RuleType::Pattern {
            field: field.into(),
            pattern: pattern.into(),
        };
        self
    }

    /// Sets the rule type to a custom, separately registered function.
    #[must_use]
    pub fn custom(mut self, function_name: impl Into<String>) -> Self {
        self.rule.rule_type = RuleType::Custom {
            function_name: function_name.into(),
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
        data.insert("max_value".to_string(), QcValue::Number(150.0));

        let result = engine.execute_all(&data);
        assert!(result.is_ok());

        let issues = result.ok().unwrap_or_default();
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn test_enumeration_rule_pass_and_violate() {
        let mut ruleset = RuleSet::new("Test", "Test");
        let rule = RuleBuilder::new("R-ENUM", "Land Cover Enum")
            .enumeration(
                "land_cover",
                vec!["forest".to_string(), "water".to_string()],
            )
            .severity(Severity::Minor)
            .build();
        ruleset.add_rule(rule);
        let engine = RulesEngine::new(ruleset);

        let mut passing = HashMap::new();
        passing.insert(
            "land_cover".to_string(),
            QcValue::Text("forest".to_string()),
        );
        let issues = engine
            .execute_all(&passing)
            .expect("enumeration rule should execute for a valid value");
        assert!(
            issues.is_empty(),
            "an allowed enumeration value must not raise an issue"
        );

        let mut violating = HashMap::new();
        violating.insert("land_cover".to_string(), QcValue::Text("urban".to_string()));
        let issues = engine
            .execute_all(&violating)
            .expect("enumeration rule should execute for an invalid value");
        assert_eq!(
            issues.len(),
            1,
            "a value outside the allowed enumeration must raise an issue"
        );

        // Missing field is treated the same as a violation.
        let empty: HashMap<String, QcValue> = HashMap::new();
        let issues = engine
            .execute_all(&empty)
            .expect("enumeration rule should execute when the field is missing");
        assert_eq!(issues.len(), 1, "a missing field must raise an issue");
    }

    #[test]
    fn test_pattern_rule_match_and_violate() {
        let mut ruleset = RuleSet::new("Test", "Test");
        let rule = RuleBuilder::new("R-PATTERN", "ID Format")
            .pattern("station_id", r"^ST-\d{3}$")
            .severity(Severity::Major)
            .build();
        ruleset.add_rule(rule);
        let engine = RulesEngine::new(ruleset);

        let mut passing = HashMap::new();
        passing.insert(
            "station_id".to_string(),
            QcValue::Text("ST-042".to_string()),
        );
        let issues = engine
            .execute_all(&passing)
            .expect("pattern rule should execute for a matching value");
        assert!(
            issues.is_empty(),
            "a matching pattern must not raise an issue"
        );

        let mut violating = HashMap::new();
        violating.insert(
            "station_id".to_string(),
            QcValue::Text("bad-id".to_string()),
        );
        let issues = engine
            .execute_all(&violating)
            .expect("pattern rule should execute for a non-matching value");
        assert_eq!(
            issues.len(),
            1,
            "a value not matching the pattern must raise an issue"
        );
    }

    #[test]
    fn test_pattern_rule_invalid_regex_is_error() {
        let mut ruleset = RuleSet::new("Test", "Test");
        let rule = RuleBuilder::new("R-BAD-PATTERN", "Broken Pattern")
            .pattern("field", "(unterminated")
            .build();
        ruleset.add_rule(rule);
        let engine = RulesEngine::new(ruleset);

        let mut data = HashMap::new();
        data.insert("field".to_string(), QcValue::Text("x".to_string()));
        let result = engine.execute_all(&data);
        assert!(
            result.is_err(),
            "an invalid regex pattern must surface as an error"
        );
    }

    #[test]
    fn test_custom_rule_registered_and_unregistered() {
        let mut ruleset = RuleSet::new("Test", "Test");
        let rule = RuleBuilder::new("R-CUSTOM", "Custom Rule")
            .custom("always_violate")
            .build();
        ruleset.add_rule(rule);

        // Unregistered: must error, never silently pass.
        let engine = RulesEngine::new(ruleset.clone());
        let data: HashMap<String, QcValue> = HashMap::new();
        let result = engine.execute_all(&data);
        assert!(
            result.is_err(),
            "an unregistered custom function must be reported as an error"
        );

        // Registered: handler is actually invoked.
        let mut engine = RulesEngine::new(ruleset);
        engine.register_custom_fn("always_violate", |_data| true);
        let issues = engine
            .execute_all(&data)
            .expect("registered custom function should execute successfully");
        assert_eq!(
            issues.len(),
            1,
            "the registered custom handler's result must be honored"
        );

        let mut engine_pass = RulesEngine::new(engine.rule_set().clone());
        engine_pass.register_custom_fn("always_violate", |_data| false);
        let issues = engine_pass
            .execute_all(&data)
            .expect("registered custom function should execute successfully");
        assert!(
            issues.is_empty(),
            "a custom handler returning false must not raise an issue"
        );
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
