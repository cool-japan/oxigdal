//! Alert module tests

use super::*;
#[cfg(test)]
mod tests {
    use super::*;

    struct MockMetricProvider {
        metrics: HashMap<String, f64>,
    }

    impl MockMetricProvider {
        fn new() -> Self {
            Self {
                metrics: HashMap::new(),
            }
        }

        fn set_metric(&mut self, name: &str, value: f64) {
            self.metrics.insert(name.to_string(), value);
        }
    }

    impl MetricProvider for MockMetricProvider {
        fn get_metric(&self, name: &str) -> Option<f64> {
            self.metrics.get(name).copied()
        }

        fn get_metric_range(&self, name: &str, _duration_seconds: u64) -> Vec<MetricDataPoint> {
            if let Some(&value) = self.metrics.get(name) {
                vec![MetricDataPoint {
                    name: name.to_string(),
                    value,
                    labels: HashMap::new(),
                    timestamp: Utc::now(),
                }]
            } else {
                Vec::new()
            }
        }
    }

    #[test]
    fn test_alert_level_ordering() {
        assert!(AlertLevel::Critical > AlertLevel::Warning);
        assert!(AlertLevel::Page > AlertLevel::Critical);
        assert!(AlertLevel::Warning > AlertLevel::Info);
    }

    #[test]
    fn test_threshold_operator() {
        assert!(ThresholdOperator::GreaterThan.evaluate(10.0, 5.0));
        assert!(!ThresholdOperator::GreaterThan.evaluate(5.0, 10.0));
        assert!(ThresholdOperator::LessThan.evaluate(5.0, 10.0));
        assert!(ThresholdOperator::Equal.evaluate(5.0, 5.0));
    }

    #[test]
    fn test_alert_instance_state_transitions() {
        let rule = AlertRuleDefinition::new("test_rule")
            .with_severity(AlertLevel::Warning);
        let mut alert = AlertInstance::from_rule(&rule);

        assert_eq!(alert.state, AlertState::Inactive);

        alert.transition_to_pending();
        assert_eq!(alert.state, AlertState::Pending);
        assert!(alert.pending_at.is_some());

        alert.transition_to_firing();
        assert_eq!(alert.state, AlertState::Firing);
        assert!(alert.firing_at.is_some());
        assert_eq!(alert.fire_count, 1);

        alert.transition_to_resolved();
        assert_eq!(alert.state, AlertState::Resolved);
        assert!(alert.resolved_at.is_some());
    }

    #[test]
    fn test_silence_rule_matching() {
        let silence = SilenceRule::new(
            Utc::now() - Duration::hours(1),
            Utc::now() + Duration::hours(1),
            "test",
            "Testing silence",
        )
        .with_matcher("env", "production");

        let rule = AlertRuleDefinition::new("test_rule")
            .with_label("env", "production");
        let alert = AlertInstance::from_rule(&rule);

        assert!(silence.matches(&alert));
    }

    #[test]
    fn test_alert_grouper() {
        let grouper = AlertGrouper::with_keys(vec!["service".to_string()]);

        let rule = AlertRuleDefinition::new("test_rule")
            .with_label("service", "api");
        let alert = AlertInstance::from_rule(&rule);

        let group_id = grouper.add_alert(&alert);
        assert!(group_id.contains("service=api"));

        let groups = grouper.get_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count(), 1);
    }

    #[test]
    fn test_alert_history() {
        let history = AlertHistory::new();

        let event = AlertHistoryEvent::new("alert-1", AlertHistoryEventType::Firing);
        history.record(event);

        let events = history.get_alert_history("alert-1");
        assert_eq!(events.len(), 1);

        let recent = history.get_recent_events(10);
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn test_condition_evaluator() {
        let mut provider = MockMetricProvider::new();
        provider.set_metric("cpu_usage", 95.0);

        let evaluator = ConditionEvaluator::new(Arc::new(provider));

        let condition = ConditionExpression::threshold(
            "cpu_usage",
            ThresholdOperator::GreaterThan,
            90.0,
        );

        assert!(evaluator.evaluate(&condition));
    }

    #[test]
    fn test_compound_conditions() {
        let mut provider = MockMetricProvider::new();
        provider.set_metric("cpu_usage", 95.0);
        provider.set_metric("memory_usage", 80.0);

        let evaluator = ConditionEvaluator::new(Arc::new(provider));

        let condition = ConditionExpression::and(vec![
            ConditionExpression::threshold("cpu_usage", ThresholdOperator::GreaterThan, 90.0),
            ConditionExpression::threshold("memory_usage", ThresholdOperator::GreaterThan, 70.0),
        ]);

        assert!(evaluator.evaluate(&condition));

        let or_condition = ConditionExpression::or(vec![
            ConditionExpression::threshold("cpu_usage", ThresholdOperator::GreaterThan, 99.0),
            ConditionExpression::threshold("memory_usage", ThresholdOperator::GreaterThan, 70.0),
        ]);

        assert!(evaluator.evaluate(&or_condition));
    }

    #[test]
    fn test_alert_rule_definition_builder() {
        let rule = AlertRuleDefinition::new("high_cpu")
            .with_name("High CPU Usage")
            .with_description("CPU usage is too high")
            .with_severity(AlertLevel::Critical)
            .with_pending_duration(std::time::Duration::from_secs(300))
            .with_label("team", "platform")
            .with_annotation("runbook", "https://wiki/runbook/cpu")
            .with_runbook_url("https://wiki/runbook/cpu");

        assert_eq!(rule.id, "high_cpu");
        assert_eq!(rule.name, "High CPU Usage");
        assert_eq!(rule.level, AlertLevel::Critical);
        assert_eq!(rule.pending_duration.as_secs(), 300);
        assert_eq!(rule.labels.get("team"), Some(&"platform".to_string()));
    }
}
