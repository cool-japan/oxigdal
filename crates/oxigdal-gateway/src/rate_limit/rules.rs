//! Rate limit rules and rule engine.
//!
//! Provides flexible rule-based rate limiting with support for per-user, per-IP,
//! per-endpoint, and combined rules.

use crate::error::{GatewayError, Result};
use crate::rate_limit::{Decision, RateLimitKey, RateLimiter};
use std::sync::Arc;
use std::time::Duration;

/// Rate limit rule configuration.
#[derive(Debug, Clone)]
pub struct RateLimitRule {
    /// Rule identifier.
    pub id: String,
    /// Requests limit.
    pub limit: u64,
    /// Time window.
    pub window: Duration,
    /// Rule priority (higher = higher priority).
    pub priority: i32,
    /// Rule matcher.
    pub matcher: RuleMatcher,
}

impl RateLimitRule {
    /// Creates a new rate limit rule.
    pub fn new(id: impl Into<String>, limit: u64, window: Duration, matcher: RuleMatcher) -> Self {
        Self {
            id: id.into(),
            limit,
            window,
            priority: 0,
            matcher,
        }
    }

    /// Sets the priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Checks if rule matches the given context.
    pub fn matches(&self, context: &RuleContext) -> bool {
        self.matcher.matches(context)
    }
}

/// Rule matcher for determining which requests a rule applies to.
#[derive(Debug, Clone)]
pub enum RuleMatcher {
    /// Matches all requests.
    All,
    /// Matches specific user.
    User(String),
    /// Matches IP address pattern.
    IpPattern(String),
    /// Matches endpoint path.
    Endpoint(String),
    /// Matches endpoint prefix.
    EndpointPrefix(String),
    /// Matches API key.
    ApiKey(String),
    /// Matches multiple conditions (AND).
    And(Vec<RuleMatcher>),
    /// Matches any condition (OR).
    Or(Vec<RuleMatcher>),
    /// Negates condition.
    Not(Box<RuleMatcher>),
}

impl RuleMatcher {
    /// Checks if matcher matches the given context.
    pub fn matches(&self, context: &RuleContext) -> bool {
        match self {
            Self::All => true,
            Self::User(user) => context.user_id.as_ref() == Some(user),
            Self::IpPattern(pattern) => context
                .ip_address
                .as_ref()
                .map(|ip| ip.starts_with(pattern))
                .unwrap_or(false),
            Self::Endpoint(endpoint) => context.endpoint.as_ref() == Some(endpoint),
            Self::EndpointPrefix(prefix) => context
                .endpoint
                .as_ref()
                .map(|ep| ep.starts_with(prefix))
                .unwrap_or(false),
            Self::ApiKey(key) => context.api_key.as_ref() == Some(key),
            Self::And(matchers) => matchers.iter().all(|m| m.matches(context)),
            Self::Or(matchers) => matchers.iter().any(|m| m.matches(context)),
            Self::Not(matcher) => !matcher.matches(context),
        }
    }
}

/// Context for rule matching.
#[derive(Debug, Clone, Default)]
pub struct RuleContext {
    /// User identifier.
    pub user_id: Option<String>,
    /// IP address.
    pub ip_address: Option<String>,
    /// Endpoint path.
    pub endpoint: Option<String>,
    /// API key.
    pub api_key: Option<String>,
    /// Additional metadata.
    pub metadata: std::collections::HashMap<String, String>,
}

impl RuleContext {
    /// Creates a new rule context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets user ID.
    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Sets IP address.
    pub fn with_ip(mut self, ip: impl Into<String>) -> Self {
        self.ip_address = Some(ip.into());
        self
    }

    /// Sets endpoint.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Sets API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Rule engine for evaluating rate limit rules.
pub struct RuleEngine<L: RateLimiter> {
    rules: Vec<RateLimitRule>,
    limiter: Arc<L>,
}

impl<L: RateLimiter> RuleEngine<L> {
    /// Creates a new rule engine.
    pub fn new(limiter: L) -> Self {
        Self {
            rules: Vec::new(),
            limiter: Arc::new(limiter),
        }
    }

    /// Adds a rule to the engine.
    pub fn add_rule(&mut self, rule: RateLimitRule) {
        self.rules.push(rule);
        // Sort by priority (descending)
        self.rules.sort_by_key(|x| std::cmp::Reverse(x.priority));
    }

    /// Finds matching rule for context.
    pub fn find_matching_rule(&self, context: &RuleContext) -> Option<&RateLimitRule> {
        self.rules.iter().find(|rule| rule.matches(context))
    }

    /// Checks rate limit with rule engine.
    pub async fn check(&self, context: &RuleContext) -> Result<Decision> {
        let rule = self
            .find_matching_rule(context)
            .ok_or_else(|| GatewayError::ConfigError("No matching rule found".to_string()))?;

        let key = self.build_key(context, rule);
        self.limiter.check(&key).await
    }

    /// Records request with rule engine.
    pub async fn record(&self, context: &RuleContext) -> Result<()> {
        let rule = self
            .find_matching_rule(context)
            .ok_or_else(|| GatewayError::ConfigError("No matching rule found".to_string()))?;

        let key = self.build_key(context, rule);
        self.limiter.record(&key).await
    }

    fn build_key(&self, context: &RuleContext, rule: &RateLimitRule) -> RateLimitKey {
        let identifier = context
            .user_id
            .clone()
            .or_else(|| context.api_key.clone())
            .or_else(|| context.ip_address.clone())
            .unwrap_or_else(|| "anonymous".to_string());

        RateLimitKey::new(identifier)
            .with_resource(rule.id.clone())
            .with_namespace(context.endpoint.clone().unwrap_or_default())
    }
}

/// Pre-configured rate limit profiles.
pub struct RateLimitProfiles;

impl RateLimitProfiles {
    /// Free tier: 1000 requests per hour.
    pub fn free_tier() -> RateLimitRule {
        RateLimitRule::new(
            "free_tier",
            1000,
            Duration::from_secs(3600),
            RuleMatcher::All,
        )
    }

    /// Basic tier: 10,000 requests per hour.
    pub fn basic_tier() -> RateLimitRule {
        RateLimitRule::new(
            "basic_tier",
            10_000,
            Duration::from_secs(3600),
            RuleMatcher::All,
        )
        .with_priority(10)
    }

    /// Pro tier: 100,000 requests per hour.
    pub fn pro_tier() -> RateLimitRule {
        RateLimitRule::new(
            "pro_tier",
            100_000,
            Duration::from_secs(3600),
            RuleMatcher::All,
        )
        .with_priority(20)
    }

    /// Enterprise tier: 1,000,000 requests per hour.
    pub fn enterprise_tier() -> RateLimitRule {
        RateLimitRule::new(
            "enterprise_tier",
            1_000_000,
            Duration::from_secs(3600),
            RuleMatcher::All,
        )
        .with_priority(30)
    }

    /// Per-endpoint strict limit: 100 requests per minute.
    pub fn strict_endpoint(endpoint: impl Into<String>) -> RateLimitRule {
        let endpoint_str = endpoint.into();
        RateLimitRule::new(
            format!("strict_{}", endpoint_str),
            100,
            Duration::from_secs(60),
            RuleMatcher::Endpoint(endpoint_str),
        )
        .with_priority(100)
    }

    /// Anonymous/unauthenticated: 100 requests per hour.
    pub fn anonymous() -> RateLimitRule {
        RateLimitRule::new(
            "anonymous",
            100,
            Duration::from_secs(3600),
            RuleMatcher::All,
        )
        .with_priority(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_matcher_all() {
        let matcher = RuleMatcher::All;
        let context = RuleContext::new();
        assert!(matcher.matches(&context));
    }

    #[test]
    fn test_rule_matcher_user() {
        let matcher = RuleMatcher::User("user123".to_string());
        let context = RuleContext::new().with_user("user123");
        assert!(matcher.matches(&context));

        let context2 = RuleContext::new().with_user("user456");
        assert!(!matcher.matches(&context2));
    }

    #[test]
    fn test_rule_matcher_endpoint() {
        let matcher = RuleMatcher::Endpoint("/api/v1/data".to_string());
        let context = RuleContext::new().with_endpoint("/api/v1/data");
        assert!(matcher.matches(&context));

        let context2 = RuleContext::new().with_endpoint("/api/v2/data");
        assert!(!matcher.matches(&context2));
    }

    #[test]
    fn test_rule_matcher_and() {
        let matcher = RuleMatcher::And(vec![
            RuleMatcher::User("user123".to_string()),
            RuleMatcher::Endpoint("/api/v1/data".to_string()),
        ]);

        let context = RuleContext::new()
            .with_user("user123")
            .with_endpoint("/api/v1/data");
        assert!(matcher.matches(&context));

        let context2 = RuleContext::new().with_user("user123");
        assert!(!matcher.matches(&context2));
    }

    #[test]
    fn test_rule_matcher_or() {
        let matcher = RuleMatcher::Or(vec![
            RuleMatcher::User("user123".to_string()),
            RuleMatcher::User("user456".to_string()),
        ]);

        let context1 = RuleContext::new().with_user("user123");
        assert!(matcher.matches(&context1));

        let context2 = RuleContext::new().with_user("user456");
        assert!(matcher.matches(&context2));

        let context3 = RuleContext::new().with_user("user789");
        assert!(!matcher.matches(&context3));
    }

    #[test]
    fn test_rule_matcher_not() {
        let matcher = RuleMatcher::Not(Box::new(RuleMatcher::User("blocked".to_string())));

        let context1 = RuleContext::new().with_user("allowed");
        assert!(matcher.matches(&context1));

        let context2 = RuleContext::new().with_user("blocked");
        assert!(!matcher.matches(&context2));
    }

    #[test]
    fn test_rate_limit_profiles() {
        let free = RateLimitProfiles::free_tier();
        assert_eq!(free.limit, 1000);

        let pro = RateLimitProfiles::pro_tier();
        assert_eq!(pro.limit, 100_000);
        assert_eq!(pro.priority, 20);
    }
}
