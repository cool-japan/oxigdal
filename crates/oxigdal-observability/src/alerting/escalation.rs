//! Alert escalation policies.

use super::Alert;
use crate::error::Result;
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use std::sync::Arc;

/// Escalation policy.
pub struct EscalationPolicy {
    /// Name of the escalation policy.
    pub name: String,
    /// Ordered list of escalation levels.
    pub levels: Vec<EscalationLevel>,
}

/// Escalation level.
pub struct EscalationLevel {
    /// Time to wait before escalating to this level.
    pub delay: Duration,
    /// List of recipients to notify at this level.
    pub recipients: Vec<String>,
}

/// Escalation manager.
pub struct EscalationManager {
    policies: Arc<RwLock<HashMap<String, EscalationPolicy>>>,
    escalations: Arc<RwLock<HashMap<String, EscalationState>>>,
}

use std::collections::HashMap;

#[derive(Clone)]
#[allow(dead_code)]
struct EscalationState {
    alert: Alert,
    current_level: usize,
    last_escalated_at: DateTime<Utc>,
}

impl EscalationManager {
    /// Create a new escalation manager.
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(HashMap::new())),
            escalations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add an escalation policy.
    pub fn add_policy(&self, policy: EscalationPolicy) {
        self.policies.write().insert(policy.name.clone(), policy);
    }

    /// Start escalation for an alert.
    pub fn start_escalation(&self, alert: Alert, policy_name: &str) -> Result<()> {
        let policies = self.policies.read();
        let _policy = policies.get(policy_name).ok_or_else(|| {
            crate::error::ObservabilityError::NotFound(format!("Policy {}", policy_name))
        })?;

        let state = EscalationState {
            alert: alert.clone(),
            current_level: 0,
            last_escalated_at: Utc::now(),
        };

        self.escalations.write().insert(alert.id.clone(), state);
        Ok(())
    }

    /// Check and perform escalations if needed.
    pub async fn check_escalations(&self) -> Result<()> {
        let policies = self.policies.read();
        let mut escalations = self.escalations.write();

        for (_alert_id, state) in escalations.iter_mut() {
            // Check if enough time has passed for escalation
            let elapsed = Utc::now() - state.last_escalated_at;

            // Find matching policy and check if escalation is needed
            for policy in policies.values() {
                if state.current_level < policy.levels.len() {
                    let level = &policy.levels[state.current_level];
                    if elapsed >= level.delay {
                        // Perform escalation
                        state.current_level += 1;
                        state.last_escalated_at = Utc::now();
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for EscalationManager {
    fn default() -> Self {
        Self::new()
    }
}
