//! Main alert engine and manager

use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{ObservabilityError, Result};
use super::*;
use super::evaluator::{ConditionEvaluator, MetricProvider};
use super::channels::NotificationSender;
use super::silence::SilenceManager;
use super::history::{AlertHistory, AlertHistoryEvent, AlertHistoryEventType};
use super::grouping::AlertGrouper;
use super::rules::AlertRuleDefinition;
use super::instance::AlertInstance;
use super::{AlertLevel, AlertState};
/// Main alert engine coordinating all alert operations.
pub struct AlertEngine<P: MetricProvider> {
    /// Alert rules.
    rules: Arc<RwLock<HashMap<String, AlertRuleDefinition>>>,
    /// Active alert instances.
    alerts: Arc<RwLock<HashMap<String, AlertInstance>>>,
    /// Condition evaluator.
    evaluator: Arc<ConditionEvaluator<P>>,
    /// Notification sender.
    notifier: Arc<RwLock<NotificationSender>>,
    /// Silence manager.
    silence_manager: Arc<SilenceManager>,
    /// Alert history.
    history: Arc<AlertHistory>,
    /// Alert grouper.
    grouper: Arc<AlertGrouper>,
}

impl<P: MetricProvider + 'static> AlertEngine<P> {
    /// Create a new alert engine.
    pub fn new(metric_provider: Arc<P>) -> Self {
        Self {
            rules: Arc::new(RwLock::new(HashMap::new())),
            alerts: Arc::new(RwLock::new(HashMap::new())),
            evaluator: Arc::new(ConditionEvaluator::new(metric_provider)),
            notifier: Arc::new(RwLock::new(NotificationSender::new())),
            silence_manager: Arc::new(SilenceManager::new()),
            history: Arc::new(AlertHistory::new()),
            grouper: Arc::new(AlertGrouper::new()),
        }
    }

    /// Add an alert rule.
    pub fn add_rule(&self, rule: AlertRuleDefinition) -> Result<()> {
        self.rules.write().insert(rule.id.clone(), rule);
        Ok(())
    }

    /// Remove an alert rule.
    pub fn remove_rule(&self, rule_id: &str) -> Option<AlertRuleDefinition> {
        self.rules.write().remove(rule_id)
    }

    /// Add a notification channel.
    pub fn add_notification_channel(&self, channel: NotificationChannel) -> Result<()> {
        self.notifier.write().add_channel(channel);
        Ok(())
    }

    /// Add a silence rule.
    pub fn add_silence(&self, silence: SilenceRule) -> String {
        self.silence_manager.add_silence(silence)
    }

    /// Evaluate all alert rules.
    pub async fn evaluate_all(&self) -> Result<Vec<AlertInstance>> {
        let rules: Vec<AlertRuleDefinition> = self.rules.read().values().cloned().collect();
        let mut processed_alerts = Vec::new();

        for rule in rules {
            if !rule.enabled {
                continue;
            }

            if let Some(ref condition) = rule.condition {
                let condition_met = self.evaluator.evaluate(condition);
                let alert = self.process_rule_evaluation(&rule, condition_met).await?;
                if let Some(a) = alert {
                    processed_alerts.push(a);
                }
            }
        }

        Ok(processed_alerts)
    }

    async fn process_rule_evaluation(
        &self,
        rule: &AlertRuleDefinition,
        condition_met: bool,
    ) -> Result<Option<AlertInstance>> {
        let mut alerts = self.alerts.write();
        let alert = alerts
            .entry(rule.id.clone())
            .or_insert_with(|| AlertInstance::from_rule(rule));

        let previous_state = alert.state;

        if condition_met {
            match alert.state {
                AlertState::Inactive | AlertState::Resolved => {
                    if rule.pending_duration.is_zero() {
                        alert.transition_to_firing();
                        self.history.record(AlertHistoryEvent::new(
                            &alert.id,
                            AlertHistoryEventType::Firing,
                        ));
                    } else {
                        alert.transition_to_pending();
                        self.history.record(AlertHistoryEvent::new(
                            &alert.id,
                            AlertHistoryEventType::Pending,
                        ));
                    }
                }
                AlertState::Pending => {
                    if let Some(pending_at) = alert.pending_at {
                        let elapsed = Utc::now() - pending_at;
                        if elapsed >= Duration::from_std(rule.pending_duration).unwrap_or(Duration::zero()) {
                            alert.transition_to_firing();
                            self.history.record(AlertHistoryEvent::new(
                                &alert.id,
                                AlertHistoryEventType::Firing,
                            ));
                        }
                    }
                }
                _ => {}
            }
        } else if alert.state.is_active() {
            alert.transition_to_resolved();
            self.history.record(AlertHistoryEvent::new(
                &alert.id,
                AlertHistoryEventType::Resolved,
            ));
        }

        // Check silencing
        if self.silence_manager.is_silenced(alert) {
            alert.transition_to_silenced();
            self.history.record(AlertHistoryEvent::new(
                &alert.id,
                AlertHistoryEventType::Silenced,
            ));
        }

        // Update grouping
        self.grouper.add_alert(alert);

        // Send notifications on state change
        if previous_state != alert.state && alert.state.requires_attention() {
            let notifier = self.notifier.read();
            if let Err(e) = notifier.send(alert).await {
                self.history.record(
                    AlertHistoryEvent::new(&alert.id, AlertHistoryEventType::NotificationFailed {
                        channel: "all".to_string(),
                        error: e.to_string(),
                    })
                );
            } else {
                self.history.record(AlertHistoryEvent::new(
                    &alert.id,
                    AlertHistoryEventType::NotificationSent {
                        channel: "all".to_string(),
                    },
                ));
            }
        }

        Ok(Some(alert.clone()))
    }

    /// Get all active alerts.
    pub fn get_active_alerts(&self) -> Vec<AlertInstance> {
        self.alerts
            .read()
            .values()
            .filter(|a| a.state.is_active())
            .cloned()
            .collect()
    }

    /// Get all alerts.
    pub fn get_all_alerts(&self) -> Vec<AlertInstance> {
        self.alerts.read().values().cloned().collect()
    }

    /// Get alert by ID.
    pub fn get_alert(&self, alert_id: &str) -> Option<AlertInstance> {
        self.alerts.read().get(alert_id).cloned()
    }

    /// Acknowledge an alert.
    pub fn acknowledge_alert(&self, alert_id: &str) -> Result<()> {
        if let Some(alert) = self.alerts.write().get_mut(alert_id) {
            alert.acknowledge();
            self.history.record(AlertHistoryEvent::new(
                alert_id,
                AlertHistoryEventType::Acknowledged,
            ));
            Ok(())
        } else {
            Err(ObservabilityError::NotFound(format!(
                "Alert {} not found",
                alert_id
            )))
        }
    }

    /// Get alert history.
    pub fn get_alert_history(&self, alert_id: &str) -> Vec<AlertHistoryEvent> {
        self.history.get_alert_history(alert_id)
    }

    /// Get alert groups.
    pub fn get_alert_groups(&self) -> Vec<AlertGroup> {
        self.grouper.get_groups()
    }

    /// Get active silences.
    pub fn get_active_silences(&self) -> Vec<SilenceRule> {
        self.silence_manager.active_silences()
    }
}

