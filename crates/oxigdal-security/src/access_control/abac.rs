//! Attribute-Based Access Control (ABAC).

use crate::access_control::{AccessControlEvaluator, AccessDecision, AccessRequest, Action};
use crate::error::Result;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// ABAC policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbacPolicy {
    /// Policy ID.
    pub id: String,
    /// Policy name.
    pub name: String,
    /// Policy description.
    pub description: Option<String>,
    /// Subject conditions.
    pub subject_conditions: Vec<Condition>,
    /// Resource conditions.
    pub resource_conditions: Vec<Condition>,
    /// Context conditions.
    pub context_conditions: Vec<Condition>,
    /// Actions allowed.
    pub actions: Vec<Action>,
    /// Effect (Allow or Deny).
    pub effect: PolicyEffect,
    /// Priority (higher priority policies evaluated first).
    pub priority: i32,
}

impl AbacPolicy {
    /// Create a new ABAC policy.
    pub fn new(id: String, name: String, actions: Vec<Action>, effect: PolicyEffect) -> Self {
        Self {
            id,
            name,
            description: None,
            subject_conditions: Vec::new(),
            resource_conditions: Vec::new(),
            context_conditions: Vec::new(),
            actions,
            effect,
            priority: 0,
        }
    }

    /// Set description.
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Add subject condition.
    pub fn with_subject_condition(mut self, condition: Condition) -> Self {
        self.subject_conditions.push(condition);
        self
    }

    /// Add resource condition.
    pub fn with_resource_condition(mut self, condition: Condition) -> Self {
        self.resource_conditions.push(condition);
        self
    }

    /// Add context condition.
    pub fn with_context_condition(mut self, condition: Condition) -> Self {
        self.context_conditions.push(condition);
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Evaluate policy against a request.
    pub fn evaluate(&self, request: &AccessRequest) -> Option<PolicyEffect> {
        // Check if action matches
        if !self.actions.contains(&request.action) {
            return None;
        }

        // Evaluate subject conditions
        if !self.evaluate_conditions(&self.subject_conditions, &request.subject.attributes) {
            return None;
        }

        // Evaluate resource conditions
        if !self.evaluate_conditions(&self.resource_conditions, &request.resource.attributes) {
            return None;
        }

        // Evaluate context conditions
        if !self.evaluate_conditions(&self.context_conditions, &request.context.attributes) {
            return None;
        }

        Some(self.effect)
    }

    fn evaluate_conditions(
        &self,
        conditions: &[Condition],
        attributes: &HashMap<String, String>,
    ) -> bool {
        conditions.iter().all(|cond| cond.evaluate(attributes))
    }
}

/// Policy effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyEffect {
    /// Allow access.
    Allow,
    /// Deny access.
    Deny,
}

/// Attribute condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    /// Attribute key.
    pub key: String,
    /// Operator.
    pub operator: ConditionOperator,
    /// Expected value.
    pub value: String,
}

impl Condition {
    /// Create a new condition.
    pub fn new(key: String, operator: ConditionOperator, value: String) -> Self {
        Self {
            key,
            operator,
            value,
        }
    }

    /// Evaluate condition against attributes.
    pub fn evaluate(&self, attributes: &HashMap<String, String>) -> bool {
        let attr_value = match attributes.get(&self.key) {
            Some(v) => v,
            None => return false,
        };

        match self.operator {
            ConditionOperator::Equals => attr_value == &self.value,
            ConditionOperator::NotEquals => attr_value != &self.value,
            ConditionOperator::Contains => attr_value.contains(&self.value),
            ConditionOperator::StartsWith => attr_value.starts_with(&self.value),
            ConditionOperator::EndsWith => attr_value.ends_with(&self.value),
            ConditionOperator::GreaterThan => {
                if let (Ok(a), Ok(b)) = (attr_value.parse::<f64>(), self.value.parse::<f64>()) {
                    a > b
                } else {
                    false
                }
            }
            ConditionOperator::LessThan => {
                if let (Ok(a), Ok(b)) = (attr_value.parse::<f64>(), self.value.parse::<f64>()) {
                    a < b
                } else {
                    false
                }
            }
            ConditionOperator::In => {
                let values: Vec<&str> = self.value.split(',').map(|s| s.trim()).collect();
                values.contains(&attr_value.as_str())
            }
        }
    }
}

/// Condition operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionOperator {
    /// Equals.
    Equals,
    /// Not equals.
    NotEquals,
    /// Contains substring.
    Contains,
    /// Starts with.
    StartsWith,
    /// Ends with.
    EndsWith,
    /// Greater than (numeric).
    GreaterThan,
    /// Less than (numeric).
    LessThan,
    /// In list.
    In,
}

/// ABAC policy engine.
pub struct AbacEngine {
    policies: Arc<DashMap<String, AbacPolicy>>,
    /// Policy evaluation cache (request_hash -> decision).
    cache: Arc<DashMap<u64, AccessDecision>>,
}

impl AbacEngine {
    /// Create a new ABAC engine.
    pub fn new() -> Self {
        Self {
            policies: Arc::new(DashMap::new()),
            cache: Arc::new(DashMap::new()),
        }
    }

    /// Add a policy.
    pub fn add_policy(&self, policy: AbacPolicy) -> Result<()> {
        self.policies.insert(policy.id.clone(), policy);
        self.cache.clear(); // Clear cache when policies change
        Ok(())
    }

    /// Remove a policy.
    pub fn remove_policy(&self, policy_id: &str) -> Result<()> {
        self.policies.remove(policy_id);
        self.cache.clear();
        Ok(())
    }

    /// Get a policy by ID.
    pub fn get_policy(&self, policy_id: &str) -> Option<AbacPolicy> {
        self.policies.get(policy_id).map(|p| p.clone())
    }

    /// List all policies.
    pub fn list_policies(&self) -> Vec<AbacPolicy> {
        let mut policies: Vec<_> = self.policies.iter().map(|p| p.clone()).collect();
        // Sort by priority (descending)
        policies.sort_by(|a, b| b.priority.cmp(&a.priority));
        policies
    }

    /// Clear all policies.
    pub fn clear_policies(&self) {
        self.policies.clear();
        self.cache.clear();
    }

    /// Clear evaluation cache.
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.cache.len(), self.policies.len())
    }

    fn compute_request_hash(request: &AccessRequest) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        request.subject.id.hash(&mut hasher);
        request.resource.id.hash(&mut hasher);
        format!("{:?}", request.action).hash(&mut hasher);
        hasher.finish()
    }
}

impl Default for AbacEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessControlEvaluator for AbacEngine {
    fn evaluate(&self, request: &AccessRequest) -> Result<AccessDecision> {
        // Check cache first
        let cache_key = Self::compute_request_hash(request);
        if let Some(decision) = self.cache.get(&cache_key) {
            return Ok(*decision);
        }

        // Evaluate policies in priority order
        let policies = self.list_policies();
        let mut explicit_allow = false;
        let mut explicit_deny = false;

        for policy in policies {
            if let Some(effect) = policy.evaluate(request) {
                match effect {
                    PolicyEffect::Allow => explicit_allow = true,
                    PolicyEffect::Deny => {
                        explicit_deny = true;
                        break; // Deny takes precedence
                    }
                }
            }
        }

        let decision = if explicit_deny {
            AccessDecision::Deny
        } else if explicit_allow {
            AccessDecision::Allow
        } else {
            AccessDecision::Deny // Default deny
        };

        // Cache the decision
        self.cache.insert(cache_key, decision);

        Ok(decision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access_control::{AccessContext, Resource, ResourceType, Subject, SubjectType};

    #[test]
    fn test_condition_evaluation() {
        let condition = Condition::new(
            "department".to_string(),
            ConditionOperator::Equals,
            "engineering".to_string(),
        );

        let mut attributes = HashMap::new();
        attributes.insert("department".to_string(), "engineering".to_string());

        assert!(condition.evaluate(&attributes));

        attributes.insert("department".to_string(), "sales".to_string());
        assert!(!condition.evaluate(&attributes));
    }

    #[test]
    fn test_condition_operators() {
        let mut attributes = HashMap::new();
        attributes.insert("name".to_string(), "test_user".to_string());
        attributes.insert("age".to_string(), "25".to_string());

        let cond = Condition::new(
            "name".to_string(),
            ConditionOperator::StartsWith,
            "test".to_string(),
        );
        assert!(cond.evaluate(&attributes));

        let cond = Condition::new(
            "age".to_string(),
            ConditionOperator::GreaterThan,
            "20".to_string(),
        );
        assert!(cond.evaluate(&attributes));

        let cond = Condition::new(
            "name".to_string(),
            ConditionOperator::In,
            "test_user, admin, guest".to_string(),
        );
        assert!(cond.evaluate(&attributes));
    }

    #[test]
    fn test_abac_policy() {
        let policy = AbacPolicy::new(
            "policy-1".to_string(),
            "Engineering Read Access".to_string(),
            vec![Action::Read],
            PolicyEffect::Allow,
        )
        .with_subject_condition(Condition::new(
            "department".to_string(),
            ConditionOperator::Equals,
            "engineering".to_string(),
        ));

        let subject = Subject::new("user-123".to_string(), SubjectType::User)
            .with_attribute("department".to_string(), "engineering".to_string());

        let resource = Resource::new("dataset-456".to_string(), ResourceType::Dataset);
        let context = AccessContext::new();

        let request = AccessRequest::new(subject, resource, Action::Read, context);

        assert_eq!(policy.evaluate(&request), Some(PolicyEffect::Allow));
    }

    #[test]
    fn test_abac_engine() {
        let engine = AbacEngine::new();

        let policy = AbacPolicy::new(
            "policy-1".to_string(),
            "Allow Read".to_string(),
            vec![Action::Read],
            PolicyEffect::Allow,
        );

        engine.add_policy(policy).expect("Failed to add policy");

        let subject = Subject::new("user-123".to_string(), SubjectType::User);
        let resource = Resource::new("dataset-456".to_string(), ResourceType::Dataset);
        let context = AccessContext::new();
        let request = AccessRequest::new(subject, resource, Action::Read, context);

        let decision = engine.evaluate(&request).expect("Evaluation failed");
        assert_eq!(decision, AccessDecision::Allow);
    }
}
