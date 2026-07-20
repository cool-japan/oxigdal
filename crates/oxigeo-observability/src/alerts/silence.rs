//! Alert silencing rules

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::AlertInstance;

/// Silence rule for muting alerts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilenceRule {
    /// Unique identifier.
    pub id: String,
    /// Matchers for the silence rule.
    pub matchers: Vec<SilenceMatcher>,
    /// When the silence starts.
    pub starts_at: DateTime<Utc>,
    /// When the silence ends.
    pub ends_at: DateTime<Utc>,
    /// Who created the silence.
    pub created_by: String,
    /// Comment explaining the silence.
    pub comment: String,
    /// Whether the silence is active.
    pub active: bool,
}

/// Matcher for silence rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilenceMatcher {
    /// Label name to match.
    pub name: String,
    /// Value to match (regex pattern).
    pub value: String,
    /// Whether to use regex matching.
    pub is_regex: bool,
    /// Whether to negate the match.
    pub is_negative: bool,
}

impl SilenceRule {
    /// Create a new silence rule.
    pub fn new(
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        created_by: impl Into<String>,
        comment: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            matchers: Vec::new(),
            starts_at,
            ends_at,
            created_by: created_by.into(),
            comment: comment.into(),
            active: true,
        }
    }

    /// Add a matcher to the silence rule.
    #[must_use]
    pub fn with_matcher(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.matchers.push(SilenceMatcher {
            name: name.into(),
            value: value.into(),
            is_regex: false,
            is_negative: false,
        });
        self
    }

    /// Add a regex matcher to the silence rule.
    #[must_use]
    pub fn with_regex_matcher(mut self, name: impl Into<String>, pattern: impl Into<String>) -> Self {
        self.matchers.push(SilenceMatcher {
            name: name.into(),
            value: pattern.into(),
            is_regex: true,
            is_negative: false,
        });
        self
    }

    /// Check if the silence is currently active.
    #[must_use]
    pub fn is_currently_active(&self) -> bool {
        if !self.active {
            return false;
        }
        let now = Utc::now();
        now >= self.starts_at && now < self.ends_at
    }

    /// Check if an alert matches this silence rule.
    pub fn matches(&self, alert: &AlertInstance) -> bool {
        if !self.is_currently_active() {
            return false;
        }

        for matcher in &self.matchers {
            let label_value = alert.labels.get(&matcher.name);

            let matches = match label_value {
                Some(value) => {
                    if matcher.is_regex {
                        regex::Regex::new(&matcher.value)
                            .map(|re| re.is_match(value))
                            .unwrap_or(false)
                    } else {
                        value == &matcher.value
                    }
                }
                None => false,
            };

            let final_match = if matcher.is_negative { !matches } else { matches };
            if !final_match {
                return false;
            }
        }

        true
    }
}

/// Silence manager for managing silence rules.
pub struct SilenceManager {
    silences: Arc<RwLock<HashMap<String, SilenceRule>>>,
}

impl SilenceManager {
    /// Create a new silence manager.
    pub fn new() -> Self {
        Self {
            silences: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a silence rule.
    pub fn add_silence(&self, silence: SilenceRule) -> String {
        let id = silence.id.clone();
        self.silences.write().insert(id.clone(), silence);
        id
    }

    /// Remove a silence rule.
    pub fn remove_silence(&self, id: &str) -> Option<SilenceRule> {
        self.silences.write().remove(id)
    }

    /// Expire a silence rule (set active to false).
    pub fn expire_silence(&self, id: &str) -> bool {
        if let Some(silence) = self.silences.write().get_mut(id) {
            silence.active = false;
            return true;
        }
        false
    }

    /// Check if an alert is silenced.
    pub fn is_silenced(&self, alert: &AlertInstance) -> bool {
        self.silences.read().values().any(|s| s.matches(alert))
    }

    /// Get all active silences.
    pub fn active_silences(&self) -> Vec<SilenceRule> {
        self.silences
            .read()
            .values()
            .filter(|s| s.is_currently_active())
            .cloned()
            .collect()
    }

    /// Clean up expired silences.
    pub fn cleanup_expired(&self) {
        let now = Utc::now();
        self.silences.write().retain(|_, s| s.ends_at > now || s.active);
    }
}

impl Default for SilenceManager {
    fn default() -> Self {
        Self::new()
    }
}
