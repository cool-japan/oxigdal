//! Event-driven workflow scheduling.

use crate::error::{Result, WorkflowError};
use crate::scheduler::SchedulerConfig;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Event trigger definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTrigger {
    /// Event type/name to match.
    pub event_type: String,
    /// Event pattern (regex or exact match).
    pub pattern: EventPattern,
    /// Event filter conditions.
    pub filters: Vec<EventFilter>,
    /// Description of the trigger.
    pub description: Option<String>,
}

/// Event pattern matching strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventPattern {
    /// Exact match.
    Exact {
        /// Value to match exactly.
        value: String,
    },
    /// Regex pattern.
    Regex {
        /// Regex pattern string.
        pattern: String,
    },
    /// Prefix match.
    Prefix {
        /// Prefix to match.
        prefix: String,
    },
    /// Suffix match.
    Suffix {
        /// Suffix to match.
        suffix: String,
    },
    /// Contains match.
    Contains {
        /// Substring to search for.
        substring: String,
    },
}

/// Event filter for conditional matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
    /// Field path to check (dot-separated).
    pub field: String,
    /// Filter operator.
    pub operator: FilterOperator,
    /// Filter value.
    pub value: serde_json::Value,
}

/// Filter operator enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOperator {
    /// Equal to.
    Eq,
    /// Not equal to.
    Ne,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Contains (for arrays/strings).
    Contains,
    /// Exists (field is present).
    Exists,
}

/// Workflow event for triggering executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEvent {
    /// Event ID.
    pub id: String,
    /// Event type.
    pub event_type: String,
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,
    /// Event payload.
    pub payload: serde_json::Value,
    /// Event source.
    pub source: String,
    /// Event metadata.
    pub metadata: HashMap<String, String>,
}

impl WorkflowEvent {
    /// Create a new workflow event.
    pub fn new<S: Into<String>>(event_type: S, payload: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.into(),
            timestamp: Utc::now(),
            payload,
            source: "system".to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Set the event source.
    pub fn with_source<S: Into<String>>(mut self, source: S) -> Self {
        self.source = source.into();
        self
    }

    /// Add metadata to the event.
    pub fn with_metadata<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get a value from the payload by field path.
    fn get_field_value(&self, field: &str) -> Option<&serde_json::Value> {
        let parts: Vec<&str> = field.split('.').collect();
        let mut current = &self.payload;

        for part in parts {
            current = current.get(part)?;
        }

        Some(current)
    }
}

impl EventTrigger {
    /// Create a new event trigger with exact matching.
    pub fn exact<S: Into<String>>(event_type: S, value: S) -> Self {
        Self {
            event_type: event_type.into(),
            pattern: EventPattern::Exact {
                value: value.into(),
            },
            filters: Vec::new(),
            description: None,
        }
    }

    /// Create a new event trigger with regex matching.
    pub fn regex<S: Into<String>>(event_type: S, pattern: S) -> Result<Self> {
        let pattern_str = pattern.into();

        // Validate regex
        Regex::new(&pattern_str)
            .map_err(|e| WorkflowError::validation(format!("Invalid regex pattern: {}", e)))?;

        Ok(Self {
            event_type: event_type.into(),
            pattern: EventPattern::Regex {
                pattern: pattern_str,
            },
            filters: Vec::new(),
            description: None,
        })
    }

    /// Add a filter to this trigger.
    pub fn with_filter(mut self, filter: EventFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Set the description.
    pub fn with_description<S: Into<String>>(mut self, description: S) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Check if this trigger matches the given event.
    pub fn matches(&self, event: &WorkflowEvent) -> Result<bool> {
        // Check event type
        if event.event_type != self.event_type {
            return Ok(false);
        }

        // Check pattern matching (if applicable)
        // Skip pattern matching if payload is not a string or pattern is empty
        let pattern_matches = match &self.pattern {
            EventPattern::Exact { value } => {
                if value.is_empty() {
                    // Empty pattern matches any payload (useful for filter-only matching)
                    true
                } else {
                    event.payload.as_str() == Some(value)
                }
            }
            EventPattern::Regex { pattern } => {
                let re = Regex::new(pattern)
                    .map_err(|e| WorkflowError::validation(format!("Invalid regex: {}", e)))?;
                event
                    .payload
                    .as_str()
                    .map(|s| re.is_match(s))
                    .unwrap_or(false)
            }
            EventPattern::Prefix { prefix } => event
                .payload
                .as_str()
                .map(|s| s.starts_with(prefix))
                .unwrap_or(false),
            EventPattern::Suffix { suffix } => event
                .payload
                .as_str()
                .map(|s| s.ends_with(suffix))
                .unwrap_or(false),
            EventPattern::Contains { substring } => event
                .payload
                .as_str()
                .map(|s| s.contains(substring.as_str()))
                .unwrap_or(false),
        };

        if !pattern_matches {
            return Ok(false);
        }

        // Check all filters
        for filter in &self.filters {
            if !self.evaluate_filter(filter, event)? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Evaluate a single filter against an event.
    fn evaluate_filter(&self, filter: &EventFilter, event: &WorkflowEvent) -> Result<bool> {
        let field_value = event.get_field_value(&filter.field);

        match filter.operator {
            FilterOperator::Exists => Ok(field_value.is_some()),
            FilterOperator::Eq => Ok(field_value == Some(&filter.value)),
            FilterOperator::Ne => Ok(field_value != Some(&filter.value)),
            FilterOperator::Gt => {
                if let (Some(field), Some(value)) =
                    (field_value.and_then(|v| v.as_f64()), filter.value.as_f64())
                {
                    Ok(field > value)
                } else {
                    Ok(false)
                }
            }
            FilterOperator::Gte => {
                if let (Some(field), Some(value)) =
                    (field_value.and_then(|v| v.as_f64()), filter.value.as_f64())
                {
                    Ok(field >= value)
                } else {
                    Ok(false)
                }
            }
            FilterOperator::Lt => {
                if let (Some(field), Some(value)) =
                    (field_value.and_then(|v| v.as_f64()), filter.value.as_f64())
                {
                    Ok(field < value)
                } else {
                    Ok(false)
                }
            }
            FilterOperator::Lte => {
                if let (Some(field), Some(value)) =
                    (field_value.and_then(|v| v.as_f64()), filter.value.as_f64())
                {
                    Ok(field <= value)
                } else {
                    Ok(false)
                }
            }
            FilterOperator::Contains => {
                if let Some(field_array) = field_value.and_then(|v| v.as_array()) {
                    Ok(field_array.contains(&filter.value))
                } else if let (Some(field_str), Some(value_str)) =
                    (field_value.and_then(|v| v.as_str()), filter.value.as_str())
                {
                    Ok(field_str.contains(value_str))
                } else {
                    Ok(false)
                }
            }
        }
    }
}

/// Event scheduler for managing event-driven workflow executions.
pub struct EventScheduler {
    /// Scheduler configuration (reserved for future enhancements).
    _config: SchedulerConfig,
    triggers: Arc<DashMap<String, EventTrigger>>,
    event_queue: Arc<RwLock<Vec<WorkflowEvent>>>,
}

impl EventScheduler {
    /// Create a new event scheduler.
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            _config: config,
            triggers: Arc::new(DashMap::new()),
            event_queue: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a new event trigger.
    pub async fn register_trigger(&self, trigger_id: String, trigger: EventTrigger) -> Result<()> {
        self.triggers.insert(trigger_id, trigger);
        Ok(())
    }

    /// Unregister an event trigger.
    pub async fn unregister_trigger(&self, trigger_id: &str) -> Result<()> {
        self.triggers
            .remove(trigger_id)
            .ok_or_else(|| WorkflowError::not_found(trigger_id))?;
        Ok(())
    }

    /// Publish an event to the scheduler.
    pub async fn publish_event(&self, event: WorkflowEvent) -> Result<Vec<String>> {
        let mut matched_triggers = Vec::new();

        for entry in self.triggers.iter() {
            let (trigger_id, trigger) = (entry.key(), entry.value());
            if trigger.matches(&event)? {
                matched_triggers.push(trigger_id.clone());
            }
        }

        // Add event to queue
        let mut queue = self.event_queue.write().await;
        queue.push(event);

        // Keep queue size manageable
        if queue.len() > 1000 {
            queue.remove(0);
        }

        Ok(matched_triggers)
    }

    /// Get recent events.
    pub async fn get_recent_events(&self, limit: usize) -> Vec<WorkflowEvent> {
        let queue = self.event_queue.read().await;
        let start = if queue.len() > limit {
            queue.len() - limit
        } else {
            0
        };
        queue[start..].to_vec()
    }

    /// Clear event queue.
    pub async fn clear_queue(&self) {
        let mut queue = self.event_queue.write().await;
        queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_trigger_exact_match() {
        let trigger = EventTrigger::exact("test_event", "test_value");
        let event = WorkflowEvent::new("test_event", serde_json::json!("test_value"));

        assert!(trigger.matches(&event).expect("Match failed"));
    }

    #[test]
    fn test_event_trigger_regex_match() {
        let trigger =
            EventTrigger::regex("test_event", r"^test_.*").expect("Failed to create trigger");
        let event = WorkflowEvent::new("test_event", serde_json::json!("test_value"));

        assert!(trigger.matches(&event).expect("Match failed"));
    }

    #[test]
    fn test_event_filter() {
        let filter = EventFilter {
            field: "value".to_string(),
            operator: FilterOperator::Gt,
            value: serde_json::json!(10),
        };

        let trigger = EventTrigger::exact("test_event", "").with_filter(filter);

        let event = WorkflowEvent::new("test_event", serde_json::json!({"value": 15}));

        assert!(trigger.matches(&event).expect("Match failed"));
    }

    #[tokio::test]
    async fn test_event_scheduler() {
        let scheduler = EventScheduler::new(SchedulerConfig::default());

        let trigger = EventTrigger::exact("test_event", "test");
        scheduler
            .register_trigger("trigger1".to_string(), trigger)
            .await
            .expect("Failed to register trigger");

        let event = WorkflowEvent::new("test_event", serde_json::json!("test"));
        let matched = scheduler
            .publish_event(event)
            .await
            .expect("Failed to publish event");

        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0], "trigger1");
    }
}
