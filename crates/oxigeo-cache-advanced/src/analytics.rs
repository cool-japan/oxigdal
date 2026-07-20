//! Cache analytics and performance monitoring
//!
//! Provides:
//! - Hit rate tracking
//! - Access pattern visualization data
//! - Cache efficiency metrics
//! - Recommendation engine
//! - Anomaly detection

use crate::CacheStats;
use crate::multi_tier::CacheKey;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type alias for access history
type AccessHistory = Arc<RwLock<VecDeque<(CacheKey, chrono::DateTime<chrono::Utc>)>>>;

/// Type alias for statistics history
type StatsHistory = Arc<RwLock<VecDeque<(chrono::DateTime<chrono::Utc>, CacheStats)>>>;

/// Time series data point
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimeSeriesPoint {
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Value
    pub value: f64,
}

/// Cache metrics over time
#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    /// Hit rate over time
    pub hit_rate: Vec<TimeSeriesPoint>,
    /// Miss rate over time
    pub miss_rate: Vec<TimeSeriesPoint>,
    /// Eviction rate over time
    pub eviction_rate: Vec<TimeSeriesPoint>,
    /// Cache size over time
    pub cache_size: Vec<TimeSeriesPoint>,
    /// Average latency over time
    pub avg_latency: Vec<TimeSeriesPoint>,
}

impl CacheMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Add data point
    pub fn add_point(&mut self, metric_type: MetricType, value: f64) {
        let point = TimeSeriesPoint {
            timestamp: chrono::Utc::now(),
            value,
        };

        match metric_type {
            MetricType::HitRate => self.hit_rate.push(point),
            MetricType::MissRate => self.miss_rate.push(point),
            MetricType::EvictionRate => self.eviction_rate.push(point),
            MetricType::CacheSize => self.cache_size.push(point),
            MetricType::AvgLatency => self.avg_latency.push(point),
        }
    }

    /// Trim old data points (keep last N)
    pub fn trim(&mut self, keep_last: usize) {
        if self.hit_rate.len() > keep_last {
            self.hit_rate.drain(0..self.hit_rate.len() - keep_last);
        }
        if self.miss_rate.len() > keep_last {
            self.miss_rate.drain(0..self.miss_rate.len() - keep_last);
        }
        if self.eviction_rate.len() > keep_last {
            self.eviction_rate
                .drain(0..self.eviction_rate.len() - keep_last);
        }
        if self.cache_size.len() > keep_last {
            self.cache_size.drain(0..self.cache_size.len() - keep_last);
        }
        if self.avg_latency.len() > keep_last {
            self.avg_latency
                .drain(0..self.avg_latency.len() - keep_last);
        }
    }
}

/// Metric type
#[derive(Debug, Clone, Copy)]
pub enum MetricType {
    /// Hit rate
    HitRate,
    /// Miss rate
    MissRate,
    /// Eviction rate
    EvictionRate,
    /// Cache size
    CacheSize,
    /// Average latency
    AvgLatency,
}

/// Access pattern analysis
#[derive(Debug, Clone)]
pub struct AccessPattern {
    /// Pattern type
    pub pattern_type: PatternType,
    /// Confidence (0.0 - 1.0)
    pub confidence: f64,
    /// Description
    pub description: String,
}

/// Pattern type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternType {
    /// Sequential access pattern
    Sequential,
    /// Random access pattern
    Random,
    /// Temporal locality (recent items reaccessed)
    TemporalLocality,
    /// Spatial locality (nearby items accessed together)
    SpatialLocality,
    /// Periodic pattern
    Periodic,
}

impl std::fmt::Display for PatternType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatternType::Sequential => write!(f, "Sequential"),
            PatternType::Random => write!(f, "Random"),
            PatternType::TemporalLocality => write!(f, "Temporal Locality"),
            PatternType::SpatialLocality => write!(f, "Spatial Locality"),
            PatternType::Periodic => write!(f, "Periodic"),
        }
    }
}

/// Cache recommendation
#[derive(Debug, Clone)]
pub struct CacheRecommendation {
    /// Recommendation type
    pub recommendation_type: RecommendationType,
    /// Expected improvement (percentage)
    pub expected_improvement: f64,
    /// Rationale
    pub rationale: String,
}

/// Recommendation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecommendationType {
    /// Increase cache size
    IncreaseSize,
    /// Decrease cache size
    DecreaseSize,
    /// Change eviction policy
    ChangeEvictionPolicy,
    /// Enable prefetching
    EnablePrefetching,
    /// Adjust compression settings
    AdjustCompression,
    /// Enable distributed caching
    EnableDistributed,
}

impl std::fmt::Display for RecommendationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecommendationType::IncreaseSize => write!(f, "Increase Cache Size"),
            RecommendationType::DecreaseSize => write!(f, "Decrease Cache Size"),
            RecommendationType::ChangeEvictionPolicy => write!(f, "Change Eviction Policy"),
            RecommendationType::EnablePrefetching => write!(f, "Enable Prefetching"),
            RecommendationType::AdjustCompression => write!(f, "Adjust Compression"),
            RecommendationType::EnableDistributed => write!(f, "Enable Distributed Caching"),
        }
    }
}

/// Anomaly detection result
#[derive(Debug, Clone)]
pub struct Anomaly {
    /// Anomaly type
    pub anomaly_type: AnomalyType,
    /// Severity (0.0 - 1.0)
    pub severity: f64,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Description
    pub description: String,
}

/// Anomaly type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyType {
    /// Sudden drop in hit rate
    HitRateDrop,
    /// Unusual eviction spike
    EvictionSpike,
    /// Latency spike
    LatencySpike,
    /// Cache thrashing
    Thrashing,
}

impl std::fmt::Display for AnomalyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnomalyType::HitRateDrop => write!(f, "Hit Rate Drop"),
            AnomalyType::EvictionSpike => write!(f, "Eviction Spike"),
            AnomalyType::LatencySpike => write!(f, "Latency Spike"),
            AnomalyType::Thrashing => write!(f, "Cache Thrashing"),
        }
    }
}

/// Cache analytics engine
pub struct CacheAnalytics {
    /// Historical metrics
    metrics: Arc<RwLock<CacheMetrics>>,
    /// Access history for pattern analysis
    access_history: AccessHistory,
    /// Maximum history size
    max_history: usize,
    /// Statistics history
    stats_history: StatsHistory,
    /// Maximum stats history
    max_stats_history: usize,
}

impl CacheAnalytics {
    /// Create new cache analytics
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(CacheMetrics::new())),
            access_history: Arc::new(RwLock::new(VecDeque::new())),
            max_history: 10000,
            stats_history: Arc::new(RwLock::new(VecDeque::new())),
            max_stats_history: 1000,
        }
    }

    /// Record access
    pub async fn record_access(&self, key: CacheKey) {
        let mut history = self.access_history.write().await;

        if history.len() >= self.max_history {
            history.pop_front();
        }

        history.push_back((key, chrono::Utc::now()));
    }

    /// Record statistics
    pub async fn record_stats(&self, stats: CacheStats) {
        let mut history = self.stats_history.write().await;

        if history.len() >= self.max_stats_history {
            history.pop_front();
        }

        history.push_back((chrono::Utc::now(), stats.clone()));

        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.add_point(MetricType::HitRate, stats.hit_rate());
        metrics.add_point(MetricType::MissRate, 100.0 - stats.hit_rate());
        metrics.add_point(MetricType::CacheSize, stats.bytes_stored as f64);

        // Trim old data
        metrics.trim(1000);
    }

    /// Analyze access patterns
    pub async fn analyze_patterns(&self) -> Vec<AccessPattern> {
        let history = self.access_history.read().await;
        let mut patterns = Vec::new();

        if history.len() < 10 {
            return patterns;
        }

        // Detect sequential pattern
        let sequential_confidence = self.detect_sequential(&history);
        if sequential_confidence > 0.5 {
            patterns.push(AccessPattern {
                pattern_type: PatternType::Sequential,
                confidence: sequential_confidence,
                description: "Keys are accessed in sequential order".to_string(),
            });
        }

        // Detect temporal locality
        let temporal_confidence = self.detect_temporal_locality(&history);
        if temporal_confidence > 0.5 {
            patterns.push(AccessPattern {
                pattern_type: PatternType::TemporalLocality,
                confidence: temporal_confidence,
                description: "Recently accessed keys are frequently reaccessed".to_string(),
            });
        }

        patterns
    }

    /// Detect sequential access pattern
    fn detect_sequential(
        &self,
        history: &VecDeque<(CacheKey, chrono::DateTime<chrono::Utc>)>,
    ) -> f64 {
        // Simple heuristic: check if keys follow numeric or alphabetic order
        let mut sequential_count = 0;
        let mut total_comparisons = 0;

        for window in history.iter().collect::<Vec<_>>().windows(2) {
            if let [a, b] = window {
                total_comparisons += 1;
                if a.0 < b.0 {
                    sequential_count += 1;
                }
            }
        }

        if total_comparisons > 0 {
            sequential_count as f64 / total_comparisons as f64
        } else {
            0.0
        }
    }

    /// Detect temporal locality
    fn detect_temporal_locality(
        &self,
        history: &VecDeque<(CacheKey, chrono::DateTime<chrono::Utc>)>,
    ) -> f64 {
        // Count how many keys are reaccessed within a time window
        let window_size = 10;
        let time_threshold = chrono::Duration::seconds(60);

        let mut reaccess_count = 0;
        let mut total_count = 0;

        for i in window_size..history.len() {
            total_count += 1;
            let (key, ts) = &history[i];

            // Check if this key was accessed recently
            for (prev_key, prev_ts) in history.range(i.saturating_sub(window_size)..i) {
                if key == prev_key && (*ts - *prev_ts) < time_threshold {
                    reaccess_count += 1;
                    break;
                }
            }
        }

        if total_count > 0 {
            reaccess_count as f64 / total_count as f64
        } else {
            0.0
        }
    }

    /// Generate recommendations
    pub async fn generate_recommendations(&self) -> Vec<CacheRecommendation> {
        let stats_history = self.stats_history.read().await;
        let mut recommendations = Vec::new();

        if stats_history.len() < 10 {
            return recommendations;
        }

        // Calculate recent average hit rate
        let recent_stats: Vec<_> = stats_history
            .iter()
            .rev()
            .take(10)
            .map(|(_, s)| s)
            .collect();

        let avg_hit_rate: f64 =
            recent_stats.iter().map(|s| s.hit_rate()).sum::<f64>() / recent_stats.len() as f64;

        // Low hit rate -> increase cache size
        if avg_hit_rate < 50.0 {
            recommendations.push(CacheRecommendation {
                recommendation_type: RecommendationType::IncreaseSize,
                expected_improvement: 20.0,
                rationale: format!(
                    "Hit rate is low ({:.1}%). Increasing cache size may improve performance.",
                    avg_hit_rate
                ),
            });
        }

        // High eviction rate -> change eviction policy or increase size
        let avg_evictions: f64 = recent_stats.iter().map(|s| s.evictions as f64).sum::<f64>()
            / recent_stats.len() as f64;

        if avg_evictions > 10.0 {
            recommendations.push(CacheRecommendation {
                recommendation_type: RecommendationType::ChangeEvictionPolicy,
                expected_improvement: 15.0,
                rationale: format!(
                    "High eviction rate ({:.1} per snapshot). Consider ARC or LFU policy.",
                    avg_evictions
                ),
            });
        }

        recommendations
    }

    /// Detect anomalies
    pub async fn detect_anomalies(&self) -> Vec<Anomaly> {
        let stats_history = self.stats_history.read().await;
        let mut anomalies = Vec::new();

        if stats_history.len() < 20 {
            return anomalies;
        }

        // Calculate baseline metrics
        let baseline_stats: Vec<_> = stats_history
            .iter()
            .rev()
            .skip(5)
            .take(10)
            .map(|(_, s)| s)
            .collect();

        let baseline_hit_rate: f64 =
            baseline_stats.iter().map(|s| s.hit_rate()).sum::<f64>() / baseline_stats.len() as f64;

        // Check recent stats for anomalies
        let recent_stats: Vec<_> = stats_history.iter().rev().take(5).collect();

        for (ts, stats) in recent_stats {
            let hit_rate = stats.hit_rate();

            // Detect hit rate drop
            if hit_rate < baseline_hit_rate * 0.7 {
                anomalies.push(Anomaly {
                    anomaly_type: AnomalyType::HitRateDrop,
                    severity: (baseline_hit_rate - hit_rate) / baseline_hit_rate,
                    timestamp: *ts,
                    description: format!(
                        "Hit rate dropped from {:.1}% to {:.1}%",
                        baseline_hit_rate, hit_rate
                    ),
                });
            }

            // Detect eviction spike
            if stats.evictions > 100 {
                anomalies.push(Anomaly {
                    anomaly_type: AnomalyType::EvictionSpike,
                    severity: 0.8,
                    timestamp: *ts,
                    description: format!("Eviction spike: {} evictions", stats.evictions),
                });
            }
        }

        anomalies
    }

    /// Get metrics
    pub async fn metrics(&self) -> CacheMetrics {
        self.metrics.read().await.clone()
    }

    /// Clear analytics data
    pub async fn clear(&self) {
        self.access_history.write().await.clear();
        self.stats_history.write().await.clear();
        *self.metrics.write().await = CacheMetrics::new();
    }
}

impl Default for CacheAnalytics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_analytics() {
        let analytics = CacheAnalytics::new();

        // Record some accesses
        for i in 0..20 {
            analytics.record_access(format!("key{}", i)).await;
        }

        // Record stats
        let stats = CacheStats {
            hits: 80,
            misses: 20,
            evictions: 5,
            bytes_stored: 1024 * 1024,
            item_count: 100,
        };

        analytics.record_stats(stats).await;

        let metrics = analytics.metrics().await;
        assert!(!metrics.hit_rate.is_empty());
    }

    #[tokio::test]
    async fn test_pattern_analysis() {
        let analytics = CacheAnalytics::new();

        // Sequential pattern
        for i in 0..50 {
            analytics.record_access(format!("key{:03}", i)).await;
        }

        let patterns = analytics.analyze_patterns().await;
        assert!(!patterns.is_empty());
    }

    #[tokio::test]
    async fn test_recommendations() {
        let analytics = CacheAnalytics::new();

        // Low hit rate scenario
        for _ in 0..15 {
            let stats = CacheStats {
                hits: 30,
                misses: 70,
                evictions: 15,
                bytes_stored: 1024 * 1024,
                item_count: 100,
            };
            analytics.record_stats(stats).await;
        }

        let recommendations = analytics.generate_recommendations().await;
        assert!(!recommendations.is_empty());
    }

    #[tokio::test]
    async fn test_anomaly_detection() {
        let analytics = CacheAnalytics::new();

        // Normal stats
        for _ in 0..15 {
            let stats = CacheStats {
                hits: 80,
                misses: 20,
                evictions: 2,
                bytes_stored: 1024 * 1024,
                item_count: 100,
            };
            analytics.record_stats(stats).await;
        }

        // Anomalous stats (hit rate drop)
        for _ in 0..5 {
            let stats = CacheStats {
                hits: 30,
                misses: 70,
                evictions: 150,
                bytes_stored: 1024 * 1024,
                item_count: 100,
            };
            analytics.record_stats(stats).await;
        }

        let anomalies = analytics.detect_anomalies().await;
        assert!(!anomalies.is_empty());
    }
}
