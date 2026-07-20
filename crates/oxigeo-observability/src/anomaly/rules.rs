//! Rule-based anomaly detection.

use super::{Anomaly, AnomalyDetector, AnomalySeverity, AnomalyType, DataPoint};
use crate::error::Result;

/// Rule-based anomaly detector.
pub struct RuleBasedDetector {
    rules: Vec<Rule>,
}

/// Detection rule.
pub struct Rule {
    /// Name identifying this rule.
    pub name: String,
    /// Condition function that returns true when anomaly is detected.
    pub condition: Box<dyn Fn(f64) -> bool + Send + Sync>,
    /// Severity level assigned to anomalies detected by this rule.
    pub severity: AnomalySeverity,
    /// Type of anomaly this rule detects.
    pub anomaly_type: AnomalyType,
    /// Human-readable description of the rule.
    pub description: String,
}

impl RuleBasedDetector {
    /// Create a new rule-based detector.
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    /// Add a rule.
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }
}

impl AnomalyDetector for RuleBasedDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let mut anomalies = Vec::new();

        for point in data {
            for rule in &self.rules {
                if (rule.condition)(point.value) {
                    anomalies.push(Anomaly {
                        timestamp: point.timestamp,
                        metric_name: rule.name.clone(),
                        observed_value: point.value,
                        expected_value: 0.0,
                        score: 1.0,
                        severity: rule.severity,
                        anomaly_type: rule.anomaly_type,
                        description: rule.description.clone(),
                    });
                }
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, _data: &[DataPoint]) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_rule_based_detector() {
        let rules = vec![Rule {
            name: "high_value".to_string(),
            condition: Box::new(|v| v > 100.0),
            severity: AnomalySeverity::High,
            anomaly_type: AnomalyType::Spike,
            description: "Value exceeds 100".to_string(),
        }];

        let detector = RuleBasedDetector::new(rules);

        let data = vec![DataPoint::new(Utc::now(), 150.0)];

        let anomalies = detector.detect(&data).expect("Failed to detect");
        assert_eq!(anomalies.len(), 1);
    }
}
