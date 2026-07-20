//! Graceful degradation for fault tolerance.
//!
//! Provides mechanisms for gracefully degrading service functionality
//! when components fail, including:
//! - Fallback execution
//! - Feature flags for degraded mode
//! - Load shedding
//! - Priority-based request handling

use crate::error::Result;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Degradation level indicating system health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum DegradationLevel {
    /// Normal operation - all features available
    #[default]
    Normal = 0,
    /// Light degradation - non-essential features disabled
    Light = 1,
    /// Moderate degradation - some features disabled
    Moderate = 2,
    /// Severe degradation - only essential features
    Severe = 3,
    /// Critical - minimal functionality
    Critical = 4,
}

impl std::fmt::Display for DegradationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "Normal"),
            Self::Light => write!(f, "Light"),
            Self::Moderate => write!(f, "Moderate"),
            Self::Severe => write!(f, "Severe"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

impl DegradationLevel {
    /// Check if a feature at the given level should be available.
    pub fn is_available(&self, feature_level: Self) -> bool {
        *self <= feature_level
    }
}

/// Feature flag configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    /// Feature name
    pub name: String,
    /// Minimum degradation level at which this feature is disabled
    pub disable_at: DegradationLevel,
    /// Whether the feature is currently enabled
    pub enabled: bool,
    /// Description
    pub description: String,
}

/// Degradation manager configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationConfig {
    /// Initial degradation level
    pub initial_level: DegradationLevel,
    /// Enable automatic level adjustment
    pub auto_adjust: bool,
    /// Error rate threshold for light degradation
    pub light_threshold: f64,
    /// Error rate threshold for moderate degradation
    pub moderate_threshold: f64,
    /// Error rate threshold for severe degradation
    pub severe_threshold: f64,
    /// Error rate threshold for critical degradation
    pub critical_threshold: f64,
    /// Window for calculating error rates
    pub error_window: Duration,
    /// Cooldown between level changes
    pub level_change_cooldown: Duration,
    /// Enable load shedding
    pub load_shedding_enabled: bool,
    /// Load shedding threshold (0.0-1.0)
    pub load_shedding_threshold: f64,
}

impl Default for DegradationConfig {
    fn default() -> Self {
        Self {
            initial_level: DegradationLevel::Normal,
            auto_adjust: true,
            light_threshold: 0.05,
            moderate_threshold: 0.10,
            severe_threshold: 0.25,
            critical_threshold: 0.50,
            error_window: Duration::from_secs(60),
            level_change_cooldown: Duration::from_secs(30),
            load_shedding_enabled: true,
            load_shedding_threshold: 0.8,
        }
    }
}

/// Degradation statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DegradationStats {
    /// Current degradation level
    pub current_level: DegradationLevel,
    /// Total requests
    pub total_requests: u64,
    /// Total errors
    pub total_errors: u64,
    /// Current error rate
    pub error_rate: f64,
    /// Requests shed due to load
    pub requests_shed: u64,
    /// Fallbacks executed
    pub fallbacks_executed: u64,
    /// Level changes
    pub level_changes: u64,
    /// Time at current level
    pub time_at_current_level_secs: u64,
    /// Features disabled
    pub features_disabled: usize,
}

/// Internal state for degradation manager.
struct DegradationManagerInner {
    /// Configuration
    config: DegradationConfig,
    /// Current degradation level
    level: RwLock<DegradationLevel>,
    /// Feature flags
    features: RwLock<HashMap<String, FeatureFlag>>,
    /// Error timestamps within window
    error_times: RwLock<Vec<Instant>>,
    /// Request timestamps within window
    request_times: RwLock<Vec<Instant>>,
    /// Last level change time
    last_level_change: RwLock<Option<Instant>>,
    /// Statistics
    stats: RwLock<DegradationStats>,
    /// Level start time
    level_start_time: RwLock<Instant>,
    /// Current load (0.0-1.0)
    current_load: AtomicU64,
}

/// Degradation manager for graceful service degradation.
#[derive(Clone)]
pub struct DegradationManager {
    inner: Arc<DegradationManagerInner>,
}

impl DegradationManager {
    /// Create a new degradation manager.
    pub fn new(config: DegradationConfig) -> Self {
        let initial_level = config.initial_level;
        Self {
            inner: Arc::new(DegradationManagerInner {
                config,
                level: RwLock::new(initial_level),
                features: RwLock::new(HashMap::new()),
                error_times: RwLock::new(Vec::new()),
                request_times: RwLock::new(Vec::new()),
                last_level_change: RwLock::new(None),
                stats: RwLock::new(DegradationStats {
                    current_level: initial_level,
                    ..Default::default()
                }),
                level_start_time: RwLock::new(Instant::now()),
                current_load: AtomicU64::new(0),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(DegradationConfig::default())
    }

    /// Get current degradation level.
    pub fn level(&self) -> DegradationLevel {
        *self.inner.level.read()
    }

    /// Set degradation level manually.
    pub fn set_level(&self, level: DegradationLevel) {
        let can_change = {
            let last_change = self.inner.last_level_change.read();
            last_change
                .map(|t| t.elapsed() >= self.inner.config.level_change_cooldown)
                .unwrap_or(true)
        };

        if can_change {
            let old_level = *self.inner.level.read();
            if old_level != level {
                *self.inner.level.write() = level;
                *self.inner.last_level_change.write() = Some(Instant::now());
                *self.inner.level_start_time.write() = Instant::now();

                {
                    let mut stats = self.inner.stats.write();
                    stats.current_level = level;
                    stats.level_changes += 1;
                } // Release stats lock before calling update_features_for_level

                info!("Degradation level changed: {} -> {}", old_level, level);

                self.update_features_for_level(level);
            }
        }
    }

    /// Register a feature flag.
    pub fn register_feature(&self, feature: FeatureFlag) {
        let name = feature.name.clone();
        self.inner.features.write().insert(name, feature);
    }

    /// Check if a feature is available.
    pub fn is_feature_available(&self, name: &str) -> bool {
        let features = self.inner.features.read();
        if let Some(feature) = features.get(name) {
            if !feature.enabled {
                return false;
            }
            let current_level = *self.inner.level.read();
            current_level.is_available(feature.disable_at)
        } else {
            // Unknown features are available by default
            true
        }
    }

    /// Update features based on degradation level.
    fn update_features_for_level(&self, level: DegradationLevel) {
        let mut disabled_count = 0;
        let mut features = self.inner.features.write();

        for feature in features.values_mut() {
            if level >= feature.disable_at {
                feature.enabled = false;
                disabled_count += 1;
                debug!("Feature disabled due to degradation: {}", feature.name);
            } else {
                feature.enabled = true;
            }
        }

        self.inner.stats.write().features_disabled = disabled_count;
    }

    /// Record a request.
    pub fn record_request(&self) {
        let now = Instant::now();

        {
            let mut times = self.inner.request_times.write();
            times.push(now);

            // Clean up old entries
            let window_start = now - self.inner.config.error_window;
            times.retain(|&t| t >= window_start);
        }

        {
            let mut stats = self.inner.stats.write();
            stats.total_requests += 1;
        }

        if self.inner.config.auto_adjust {
            self.maybe_adjust_level();
        }
    }

    /// Record an error.
    pub fn record_error(&self) {
        let now = Instant::now();

        {
            let mut times = self.inner.error_times.write();
            times.push(now);

            // Clean up old entries
            let window_start = now - self.inner.config.error_window;
            times.retain(|&t| t >= window_start);
        }

        {
            let mut stats = self.inner.stats.write();
            stats.total_errors += 1;
        }

        if self.inner.config.auto_adjust {
            self.maybe_adjust_level();
        }
    }

    /// Get current error rate.
    pub fn error_rate(&self) -> f64 {
        let now = Instant::now();
        let window_start = now - self.inner.config.error_window;

        let request_count = self
            .inner
            .request_times
            .read()
            .iter()
            .filter(|&&t| t >= window_start)
            .count();

        let error_count = self
            .inner
            .error_times
            .read()
            .iter()
            .filter(|&&t| t >= window_start)
            .count();

        if request_count == 0 {
            0.0
        } else {
            error_count as f64 / request_count as f64
        }
    }

    /// Maybe adjust degradation level based on error rate.
    fn maybe_adjust_level(&self) {
        let error_rate = self.error_rate();
        let current_level = *self.inner.level.read();

        // Update stats
        self.inner.stats.write().error_rate = error_rate;

        let new_level = if error_rate >= self.inner.config.critical_threshold {
            DegradationLevel::Critical
        } else if error_rate >= self.inner.config.severe_threshold {
            DegradationLevel::Severe
        } else if error_rate >= self.inner.config.moderate_threshold {
            DegradationLevel::Moderate
        } else if error_rate >= self.inner.config.light_threshold {
            DegradationLevel::Light
        } else {
            DegradationLevel::Normal
        };

        if new_level != current_level {
            self.set_level(new_level);
        }
    }

    /// Update current load (0.0-1.0).
    pub fn update_load(&self, load: f64) {
        let load_bits = (load.clamp(0.0, 1.0) * 1_000_000.0) as u64;
        self.inner.current_load.store(load_bits, Ordering::SeqCst);
    }

    /// Get current load.
    pub fn current_load(&self) -> f64 {
        let load_bits = self.inner.current_load.load(Ordering::SeqCst);
        load_bits as f64 / 1_000_000.0
    }

    /// Check if request should be shed due to load.
    pub fn should_shed_load(&self) -> bool {
        if !self.inner.config.load_shedding_enabled {
            return false;
        }

        let load = self.current_load();
        let level = *self.inner.level.read();

        // Adjust threshold based on degradation level
        let threshold = match level {
            DegradationLevel::Normal => self.inner.config.load_shedding_threshold,
            DegradationLevel::Light => self.inner.config.load_shedding_threshold * 0.9,
            DegradationLevel::Moderate => self.inner.config.load_shedding_threshold * 0.8,
            DegradationLevel::Severe => self.inner.config.load_shedding_threshold * 0.6,
            DegradationLevel::Critical => self.inner.config.load_shedding_threshold * 0.4,
        };

        if load >= threshold {
            self.inner.stats.write().requests_shed += 1;
            warn!(
                "Load shedding triggered: load={:.2}, threshold={:.2}",
                load, threshold
            );
            true
        } else {
            false
        }
    }

    /// Execute with fallback on failure.
    pub async fn with_fallback<F, FB, T, E>(&self, primary: F, fallback: FB) -> Result<T>
    where
        F: std::future::Future<Output = std::result::Result<T, E>>,
        FB: FnOnce() -> T,
        E: std::fmt::Display,
    {
        match primary.await {
            Ok(result) => {
                self.record_request();
                Ok(result)
            }
            Err(e) => {
                self.record_error();
                self.inner.stats.write().fallbacks_executed += 1;

                warn!("Executing fallback due to error: {}", e);
                Ok(fallback())
            }
        }
    }

    /// Execute with optional functionality based on degradation level.
    pub async fn execute_if_available<F, T>(&self, feature: &str, operation: F) -> Result<Option<T>>
    where
        F: std::future::Future<Output = T>,
    {
        if self.is_feature_available(feature) {
            self.record_request();
            Ok(Some(operation.await))
        } else {
            debug!(
                "Feature {} not available at current degradation level",
                feature
            );
            Ok(None)
        }
    }

    /// Get degradation statistics.
    pub fn get_stats(&self) -> DegradationStats {
        let mut stats = self.inner.stats.read().clone();
        stats.time_at_current_level_secs = self.inner.level_start_time.read().elapsed().as_secs();
        stats
    }

    /// Reset to normal operation.
    pub fn reset(&self) {
        self.set_level(DegradationLevel::Normal);
        self.inner.error_times.write().clear();
        self.inner.request_times.write().clear();
        self.inner.current_load.store(0, Ordering::SeqCst);

        let mut stats = self.inner.stats.write();
        *stats = DegradationStats {
            current_level: DegradationLevel::Normal,
            ..Default::default()
        };

        info!("Degradation manager reset to normal operation");
    }
}

/// Priority level for requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RequestPriority {
    /// Low priority - can be dropped under load
    Low = 0,
    /// Normal priority - standard handling
    Normal = 1,
    /// High priority - should be processed even under load
    High = 2,
    /// Critical priority - must be processed
    Critical = 3,
}

impl RequestPriority {
    /// Check if this priority should be processed at given degradation level.
    pub fn should_process(&self, level: DegradationLevel) -> bool {
        match level {
            DegradationLevel::Normal => true,
            DegradationLevel::Light => *self >= RequestPriority::Low,
            DegradationLevel::Moderate => *self >= RequestPriority::Normal,
            DegradationLevel::Severe => *self >= RequestPriority::High,
            DegradationLevel::Critical => *self >= RequestPriority::Critical,
        }
    }
}

/// Request classifier for priority-based handling.
#[derive(Clone)]
pub struct RequestClassifier {
    rules: Arc<RwLock<Vec<ClassificationRule>>>,
}

/// Classification rule for determining request priority.
#[derive(Clone)]
pub struct ClassificationRule {
    /// Rule name
    pub name: String,
    /// Matcher function (returns true if rule applies)
    pub matcher: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    /// Priority to assign
    pub priority: RequestPriority,
}

impl std::fmt::Debug for ClassificationRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClassificationRule")
            .field("name", &self.name)
            .field("matcher", &"<function>")
            .field("priority", &self.priority)
            .finish()
    }
}

impl RequestClassifier {
    /// Create a new request classifier.
    pub fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a classification rule.
    pub fn add_rule(&self, rule: ClassificationRule) {
        self.rules.write().push(rule);
    }

    /// Classify a request.
    pub fn classify(&self, request_type: &str) -> RequestPriority {
        let rules = self.rules.read();
        for rule in rules.iter() {
            if (rule.matcher)(request_type) {
                return rule.priority;
            }
        }
        RequestPriority::Normal
    }

    /// Check if a request should be processed given the degradation level.
    pub fn should_process(&self, request_type: &str, level: DegradationLevel) -> bool {
        let priority = self.classify(request_type);
        priority.should_process(level)
    }
}

impl Default for RequestClassifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Load shedder for controlled request rejection.
#[derive(Clone)]
pub struct LoadShedder {
    inner: Arc<LoadShedderInner>,
}

struct LoadShedderInner {
    /// Current load (0.0-1.0)
    load: AtomicU64,
    /// Shedding threshold
    threshold: f64,
    /// Requests accepted
    accepted: AtomicU64,
    /// Requests rejected
    rejected: AtomicU64,
}

impl LoadShedder {
    /// Create a new load shedder.
    pub fn new(threshold: f64) -> Self {
        Self {
            inner: Arc::new(LoadShedderInner {
                load: AtomicU64::new(0),
                threshold: threshold.clamp(0.0, 1.0),
                accepted: AtomicU64::new(0),
                rejected: AtomicU64::new(0),
            }),
        }
    }

    /// Update current load.
    pub fn update_load(&self, load: f64) {
        let load_bits = (load.clamp(0.0, 1.0) * 1_000_000.0) as u64;
        self.inner.load.store(load_bits, Ordering::SeqCst);
    }

    /// Get current load.
    pub fn load(&self) -> f64 {
        let load_bits = self.inner.load.load(Ordering::SeqCst);
        load_bits as f64 / 1_000_000.0
    }

    /// Check if request should be accepted.
    pub fn should_accept(&self, priority: RequestPriority) -> bool {
        let load = self.load();

        // Adjust threshold based on priority
        let adjusted_threshold = match priority {
            RequestPriority::Critical => 1.0, // Always accept
            RequestPriority::High => self.inner.threshold + 0.15,
            RequestPriority::Normal => self.inner.threshold,
            RequestPriority::Low => self.inner.threshold - 0.15,
        };

        if load < adjusted_threshold {
            self.inner.accepted.fetch_add(1, Ordering::SeqCst);
            true
        } else {
            self.inner.rejected.fetch_add(1, Ordering::SeqCst);
            false
        }
    }

    /// Get acceptance rate.
    pub fn acceptance_rate(&self) -> f64 {
        let accepted = self.inner.accepted.load(Ordering::SeqCst);
        let rejected = self.inner.rejected.load(Ordering::SeqCst);
        let total = accepted + rejected;

        if total == 0 {
            1.0
        } else {
            accepted as f64 / total as f64
        }
    }

    /// Get rejection count.
    pub fn rejection_count(&self) -> u64 {
        self.inner.rejected.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_degradation_manager_creation() {
        let manager = DegradationManager::with_defaults();
        assert_eq!(manager.level(), DegradationLevel::Normal);
    }

    #[test]
    fn test_set_level() {
        let manager = DegradationManager::with_defaults();
        manager.set_level(DegradationLevel::Severe);
        assert_eq!(manager.level(), DegradationLevel::Severe);
    }

    #[test]
    fn test_feature_availability() {
        let manager = DegradationManager::with_defaults();

        manager.register_feature(FeatureFlag {
            name: "analytics".to_string(),
            disable_at: DegradationLevel::Light,
            enabled: true,
            description: "Analytics feature".to_string(),
        });

        assert!(manager.is_feature_available("analytics"));

        manager.set_level(DegradationLevel::Moderate);
        assert!(!manager.is_feature_available("analytics"));
    }

    #[test]
    fn test_error_recording() {
        let manager = DegradationManager::with_defaults();

        manager.record_request();
        manager.record_request();
        manager.record_error();

        let rate = manager.error_rate();
        assert!(rate > 0.0 && rate < 1.0);
    }

    #[test]
    fn test_load_shedding() {
        let config = DegradationConfig {
            load_shedding_enabled: true,
            load_shedding_threshold: 0.8,
            ..Default::default()
        };
        let manager = DegradationManager::new(config);

        manager.update_load(0.5);
        assert!(!manager.should_shed_load());

        manager.update_load(0.9);
        assert!(manager.should_shed_load());
    }

    #[test]
    fn test_request_priority() {
        assert!(RequestPriority::Critical.should_process(DegradationLevel::Critical));
        assert!(!RequestPriority::Low.should_process(DegradationLevel::Critical));
        assert!(RequestPriority::Normal.should_process(DegradationLevel::Normal));
    }

    #[test]
    fn test_load_shedder() {
        let shedder = LoadShedder::new(0.8);

        shedder.update_load(0.5);
        assert!(shedder.should_accept(RequestPriority::Normal));

        shedder.update_load(0.9);
        assert!(!shedder.should_accept(RequestPriority::Low));
        assert!(shedder.should_accept(RequestPriority::Critical));
    }

    #[test]
    fn test_degradation_level_ordering() {
        assert!(DegradationLevel::Normal < DegradationLevel::Light);
        assert!(DegradationLevel::Light < DegradationLevel::Moderate);
        assert!(DegradationLevel::Moderate < DegradationLevel::Severe);
        assert!(DegradationLevel::Severe < DegradationLevel::Critical);
    }

    #[tokio::test]
    async fn test_with_fallback() {
        let manager = DegradationManager::with_defaults();

        // Test successful primary
        let result = manager
            .with_fallback(async { Ok::<_, &str>(42) }, || 0)
            .await;
        assert_eq!(result.ok(), Some(42));

        // Test fallback execution
        let result = manager
            .with_fallback(async { Err::<i32, _>("error") }, || 99)
            .await;
        assert_eq!(result.ok(), Some(99));
    }
}
