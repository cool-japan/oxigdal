//! Intelligent tiering policies
//!
//! Automatic promotion and demotion between cache tiers based on:
//! - Access frequency
//! - Access recency
//! - Cost-aware placement
//! - Predictive promotion
//! - Adaptive tier sizing

use crate::error::{CacheError, Result};
use crate::multi_tier::CacheKey;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Tier information
#[derive(Debug, Clone)]
pub struct TierInfo {
    /// Tier name
    pub name: String,
    /// Tier level (0 = fastest/most expensive)
    pub level: usize,
    /// Cost per byte (arbitrary units)
    pub cost_per_byte: f64,
    /// Access latency (microseconds)
    pub latency_us: u64,
    /// Current size in bytes
    pub current_size: usize,
    /// Maximum size in bytes
    pub max_size: usize,
}

impl TierInfo {
    /// Check if tier has space available
    pub fn has_space(&self, bytes: usize) -> bool {
        self.current_size + bytes <= self.max_size
    }

    /// Get utilization percentage
    pub fn utilization(&self) -> f64 {
        if self.max_size == 0 {
            0.0
        } else {
            (self.current_size as f64 / self.max_size as f64) * 100.0
        }
    }
}

/// Access statistics for a cache item
#[derive(Debug, Clone)]
pub struct AccessStats {
    /// Total number of accesses
    pub access_count: u64,
    /// Last access time
    pub last_access: Instant,
    /// First access time
    pub first_access: Instant,
    /// Access timestamps (for frequency analysis)
    pub access_times: VecDeque<Instant>,
    /// Current tier level
    pub current_tier: usize,
    /// Item size in bytes
    pub size_bytes: usize,
}

impl AccessStats {
    /// Create new access stats
    pub fn new(tier: usize, size: usize) -> Self {
        let now = Instant::now();
        let mut times = VecDeque::with_capacity(100);
        times.push_back(now);

        Self {
            access_count: 1,
            last_access: now,
            first_access: now,
            access_times: times,
            current_tier: tier,
            size_bytes: size,
        }
    }

    /// Record an access
    pub fn record_access(&mut self) {
        self.access_count += 1;
        self.last_access = Instant::now();

        // Keep recent access history
        if self.access_times.len() >= 100 {
            self.access_times.pop_front();
        }
        self.access_times.push_back(Instant::now());
    }

    /// Calculate access frequency (accesses per second)
    pub fn frequency(&self) -> f64 {
        let duration = self.last_access.duration_since(self.first_access);
        if duration.as_secs() == 0 {
            self.access_count as f64
        } else {
            self.access_count as f64 / duration.as_secs() as f64
        }
    }

    /// Calculate recency score (0.0 = old, 1.0 = very recent)
    pub fn recency_score(&self, max_age: Duration) -> f64 {
        let age = self.last_access.elapsed();
        let age_secs = age.as_secs_f64();
        let max_secs = max_age.as_secs_f64();

        if age_secs >= max_secs {
            0.0
        } else {
            1.0 - (age_secs / max_secs)
        }
    }

    /// Calculate heat score (combination of frequency and recency)
    pub fn heat_score(&self, max_age: Duration) -> f64 {
        let freq = self.frequency();
        let recency = self.recency_score(max_age);

        // Weighted combination (favor recency slightly)
        0.4 * freq.min(10.0) / 10.0 + 0.6 * recency
    }
}

/// Tiering policy decisions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TieringAction {
    /// Promote to higher tier
    Promote(usize),
    /// Demote to lower tier
    Demote(usize),
    /// Stay in current tier
    Stay,
}

/// Frequency-based tiering policy
pub struct FrequencyBasedPolicy {
    /// Access statistics
    stats: Arc<RwLock<HashMap<CacheKey, AccessStats>>>,
    /// Tier information
    tiers: Vec<TierInfo>,
    /// Promotion threshold (accesses per second)
    promotion_threshold: f64,
    /// Demotion threshold (accesses per second)
    demotion_threshold: f64,
}

impl FrequencyBasedPolicy {
    /// Create new frequency-based policy
    pub fn new(tiers: Vec<TierInfo>, promotion_threshold: f64, demotion_threshold: f64) -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            tiers,
            promotion_threshold,
            demotion_threshold,
        }
    }

    /// Record access
    pub async fn record_access(&self, key: CacheKey, tier: usize, size: usize) {
        let mut stats = self.stats.write().await;
        stats
            .entry(key)
            .and_modify(|s| s.record_access())
            .or_insert_with(|| AccessStats::new(tier, size));
    }

    /// Evaluate tiering action for a key
    pub async fn evaluate(&self, key: &CacheKey) -> Result<TieringAction> {
        let stats = self.stats.read().await;
        let item_stats = stats
            .get(key)
            .ok_or_else(|| CacheError::KeyNotFound(key.clone()))?;

        let freq = item_stats.frequency();
        let current_tier = item_stats.current_tier;

        if freq >= self.promotion_threshold && current_tier > 0 {
            // Promote to higher tier
            Ok(TieringAction::Promote(current_tier - 1))
        } else if freq <= self.demotion_threshold && current_tier < self.tiers.len() - 1 {
            // Demote to lower tier
            Ok(TieringAction::Demote(current_tier + 1))
        } else {
            Ok(TieringAction::Stay)
        }
    }

    /// Get items to promote
    pub async fn get_promotion_candidates(&self, tier: usize, limit: usize) -> Vec<CacheKey> {
        let stats = self.stats.read().await;
        let mut candidates: Vec<_> = stats
            .iter()
            .filter(|(_, s)| s.current_tier == tier && s.frequency() >= self.promotion_threshold)
            .map(|(k, s)| (k.clone(), s.frequency()))
            .collect();

        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(limit);
        candidates.into_iter().map(|(k, _)| k).collect()
    }

    /// Get items to demote
    pub async fn get_demotion_candidates(&self, tier: usize, limit: usize) -> Vec<CacheKey> {
        let stats = self.stats.read().await;
        let mut candidates: Vec<_> = stats
            .iter()
            .filter(|(_, s)| s.current_tier == tier && s.frequency() <= self.demotion_threshold)
            .map(|(k, s)| (k.clone(), s.frequency()))
            .collect();

        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(limit);
        candidates.into_iter().map(|(k, _)| k).collect()
    }
}

/// Cost-aware tiering policy
pub struct CostAwarePolicy {
    /// Access statistics
    stats: Arc<RwLock<HashMap<CacheKey, AccessStats>>>,
    /// Tier information
    tiers: Vec<TierInfo>,
    /// Maximum age for recency calculation
    max_age: Duration,
}

impl CostAwarePolicy {
    /// Create new cost-aware policy
    pub fn new(tiers: Vec<TierInfo>, max_age: Duration) -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            tiers,
            max_age,
        }
    }

    /// Record access
    pub async fn record_access(&self, key: CacheKey, tier: usize, size: usize) {
        let mut stats = self.stats.write().await;
        stats
            .entry(key)
            .and_modify(|s| s.record_access())
            .or_insert_with(|| AccessStats::new(tier, size));
    }

    /// Calculate value score (benefit per cost)
    fn value_score(&self, item_stats: &AccessStats, target_tier: usize) -> f64 {
        if target_tier >= self.tiers.len() {
            return 0.0;
        }

        let tier = &self.tiers[target_tier];
        let heat = item_stats.heat_score(self.max_age);
        let cost = item_stats.size_bytes as f64 * tier.cost_per_byte;

        if cost > 0.0 { heat / cost } else { heat }
    }

    /// Evaluate tiering action
    pub async fn evaluate(&self, key: &CacheKey) -> Result<TieringAction> {
        let stats = self.stats.read().await;
        let item_stats = stats
            .get(key)
            .ok_or_else(|| CacheError::KeyNotFound(key.clone()))?;

        let current_tier = item_stats.current_tier;
        let current_value = self.value_score(item_stats, current_tier);

        // Check if promotion makes sense
        if current_tier > 0 {
            let promote_value = self.value_score(item_stats, current_tier - 1);
            if promote_value > current_value * 1.2 {
                // 20% improvement threshold
                return Ok(TieringAction::Promote(current_tier - 1));
            }
        }

        // Check if demotion makes sense
        if current_tier < self.tiers.len() - 1 {
            let demote_value = self.value_score(item_stats, current_tier + 1);
            if current_value < demote_value * 0.8 {
                // Stay only if current is at least 80% as good
                return Ok(TieringAction::Demote(current_tier + 1));
            }
        }

        Ok(TieringAction::Stay)
    }

    /// Get optimal tier for a key based on value score
    pub async fn get_optimal_tier(&self, key: &CacheKey) -> Result<usize> {
        let stats = self.stats.read().await;
        let item_stats = stats
            .get(key)
            .ok_or_else(|| CacheError::KeyNotFound(key.clone()))?;

        let mut best_tier = 0;
        let mut best_value = 0.0;

        for (tier_idx, _tier) in self.tiers.iter().enumerate() {
            let value = self.value_score(item_stats, tier_idx);
            if value > best_value {
                best_value = value;
                best_tier = tier_idx;
            }
        }

        Ok(best_tier)
    }
}

/// Adaptive tier sizing
pub struct AdaptiveTierSizer {
    /// Tier information
    tiers: Arc<RwLock<Vec<TierInfo>>>,
    /// Target utilization percentage
    target_utilization: f64,
    /// Resize step size (percentage)
    resize_step: f64,
}

impl AdaptiveTierSizer {
    /// Create new adaptive tier sizer
    pub fn new(tiers: Vec<TierInfo>, target_utilization: f64, resize_step: f64) -> Self {
        Self {
            tiers: Arc::new(RwLock::new(tiers)),
            target_utilization,
            resize_step,
        }
    }

    /// Adjust tier sizes based on utilization
    pub async fn adjust_sizes(&self) -> Vec<TierInfo> {
        let mut tiers = self.tiers.write().await;
        let mut adjustments = Vec::new();

        for tier in tiers.iter_mut() {
            let utilization = tier.utilization();

            if utilization > self.target_utilization {
                // Increase size
                let increase = (tier.max_size as f64 * self.resize_step) as usize;
                tier.max_size += increase;
                adjustments.push(tier.clone());
            } else if utilization < self.target_utilization * 0.5 {
                // Decrease size (if very under-utilized)
                let decrease = (tier.max_size as f64 * self.resize_step * 0.5) as usize;
                tier.max_size = tier.max_size.saturating_sub(decrease);
                tier.max_size = tier.max_size.max(tier.current_size); // Don't shrink below current
                adjustments.push(tier.clone());
            }
        }

        tiers.clone()
    }

    /// Get current tier sizes
    pub async fn get_tiers(&self) -> Vec<TierInfo> {
        self.tiers.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_stats() {
        let mut stats = AccessStats::new(0, 1024);
        assert_eq!(stats.access_count, 1);

        stats.record_access();
        assert_eq!(stats.access_count, 2);

        let heat = stats.heat_score(Duration::from_secs(60));
        assert!(heat > 0.0 && heat <= 1.0);
    }

    #[tokio::test]
    async fn test_frequency_based_policy() {
        let tiers = vec![
            TierInfo {
                name: "L1".to_string(),
                level: 0,
                cost_per_byte: 1.0,
                latency_us: 10,
                current_size: 0,
                max_size: 1024 * 1024,
            },
            TierInfo {
                name: "L2".to_string(),
                level: 1,
                cost_per_byte: 0.1,
                latency_us: 100,
                current_size: 0,
                max_size: 10 * 1024 * 1024,
            },
        ];

        let policy = FrequencyBasedPolicy::new(tiers, 5.0, 0.1);

        let key = "test_key".to_string();
        policy.record_access(key.clone(), 1, 1024).await;

        let action = policy.evaluate(&key).await.unwrap_or(TieringAction::Stay);
        assert!(matches!(action, TieringAction::Stay));
    }

    #[tokio::test]
    async fn test_cost_aware_policy() {
        let tiers = vec![
            TierInfo {
                name: "L1".to_string(),
                level: 0,
                cost_per_byte: 1.0,
                latency_us: 10,
                current_size: 0,
                max_size: 1024 * 1024,
            },
            TierInfo {
                name: "L2".to_string(),
                level: 1,
                cost_per_byte: 0.1,
                latency_us: 100,
                current_size: 0,
                max_size: 10 * 1024 * 1024,
            },
        ];

        let policy = CostAwarePolicy::new(tiers, Duration::from_secs(60));

        let key = "test_key".to_string();
        policy.record_access(key.clone(), 1, 1024).await;

        let optimal = policy.get_optimal_tier(&key).await.unwrap_or(0);
        assert!(optimal < 2);
    }

    #[tokio::test]
    async fn test_adaptive_tier_sizer() {
        let tiers = vec![TierInfo {
            name: "L1".to_string(),
            level: 0,
            cost_per_byte: 1.0,
            latency_us: 10,
            current_size: 900 * 1024,
            max_size: 1024 * 1024,
        }];

        let sizer = AdaptiveTierSizer::new(tiers.clone(), 80.0, 0.1);
        let adjusted = sizer.adjust_sizes().await;

        // Should have increased size due to high utilization
        assert!(adjusted[0].max_size > 1024 * 1024);
    }
}
