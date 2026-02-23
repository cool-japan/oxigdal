//! Watermark generation for event-time processing.

use crate::core::stream::StreamElement;
use crate::error::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A watermark representing event-time progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Watermark {
    /// Watermark timestamp
    pub timestamp: DateTime<Utc>,
}

impl Watermark {
    /// Create a new watermark.
    pub fn new(timestamp: DateTime<Utc>) -> Self {
        Self { timestamp }
    }

    /// Get the minimum watermark (beginning of time).
    pub fn min() -> Self {
        Self {
            timestamp: DateTime::from_timestamp(0, 0).unwrap_or_else(Utc::now),
        }
    }

    /// Get the maximum watermark (end of time).
    pub fn max() -> Self {
        Self {
            timestamp: DateTime::from_timestamp(i64::MAX / 1000, 0).unwrap_or_else(Utc::now),
        }
    }
}

/// Strategy for generating watermarks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatermarkStrategy {
    /// Ascending timestamps (watermark = max observed timestamp)
    Ascending,

    /// Bounded out-of-orderness (watermark = max timestamp - max delay)
    BoundedOutOfOrderness,

    /// Periodic watermarks
    Periodic,

    /// Punctuated watermarks (based on special markers)
    Punctuated,
}

/// Configuration for watermark generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatermarkConfig {
    /// Watermark strategy
    pub strategy: WatermarkStrategy,

    /// Maximum out-of-orderness
    pub max_out_of_orderness: Duration,

    /// Watermark interval (for periodic strategy)
    pub interval: Duration,

    /// Idle timeout (emit watermark if no data for this duration)
    pub idle_timeout: Option<Duration>,
}

impl Default for WatermarkConfig {
    fn default() -> Self {
        Self {
            strategy: WatermarkStrategy::BoundedOutOfOrderness,
            max_out_of_orderness: Duration::seconds(5),
            interval: Duration::seconds(1),
            idle_timeout: Some(Duration::seconds(10)),
        }
    }
}

/// Generates watermarks for a stream.
pub trait WatermarkGenerator: Send + Sync {
    /// Process an element and potentially generate a watermark.
    fn on_event(&mut self, element: &StreamElement) -> Option<Watermark>;

    /// Generate a periodic watermark.
    fn on_periodic_emit(&mut self) -> Option<Watermark>;

    /// Get the current watermark.
    fn current_watermark(&self) -> Watermark;
}

/// Periodic watermark generator.
pub struct PeriodicWatermarkGenerator {
    config: WatermarkConfig,
    max_timestamp: Option<DateTime<Utc>>,
    current_watermark: Watermark,
    last_emit: Option<DateTime<Utc>>,
}

impl PeriodicWatermarkGenerator {
    /// Create a new periodic watermark generator.
    pub fn new(config: WatermarkConfig) -> Self {
        Self {
            config,
            max_timestamp: None,
            current_watermark: Watermark::min(),
            last_emit: None,
        }
    }
}

impl WatermarkGenerator for PeriodicWatermarkGenerator {
    fn on_event(&mut self, element: &StreamElement) -> Option<Watermark> {
        if let Some(max_ts) = self.max_timestamp {
            if element.event_time > max_ts {
                self.max_timestamp = Some(element.event_time);
            }
        } else {
            self.max_timestamp = Some(element.event_time);
        }

        None
    }

    fn on_periodic_emit(&mut self) -> Option<Watermark> {
        let now = Utc::now();
        let should_emit = if let Some(last) = self.last_emit {
            now - last >= self.config.interval
        } else {
            true
        };

        if should_emit {
            if let Some(max_ts) = self.max_timestamp {
                let new_watermark = match self.config.strategy {
                    WatermarkStrategy::Ascending => Watermark::new(max_ts),
                    WatermarkStrategy::BoundedOutOfOrderness => {
                        Watermark::new(max_ts - self.config.max_out_of_orderness)
                    }
                    _ => self.current_watermark,
                };

                if new_watermark > self.current_watermark {
                    self.current_watermark = new_watermark;
                    self.last_emit = Some(now);
                    return Some(new_watermark);
                }
            }
        }

        None
    }

    fn current_watermark(&self) -> Watermark {
        self.current_watermark
    }
}

/// Punctuated watermark generator.
pub struct PunctuatedWatermarkGenerator {
    config: WatermarkConfig,
    current_watermark: Watermark,
    max_timestamp: Option<DateTime<Utc>>,
}

impl PunctuatedWatermarkGenerator {
    /// Create a new punctuated watermark generator.
    pub fn new(config: WatermarkConfig) -> Self {
        Self {
            config,
            current_watermark: Watermark::min(),
            max_timestamp: None,
        }
    }

    /// Check if an element should trigger a watermark.
    fn should_emit_watermark(&self, element: &StreamElement) -> bool {
        if let Some(marker) = element.metadata.attributes.get("watermark_marker") {
            marker == "true"
        } else {
            false
        }
    }
}

impl WatermarkGenerator for PunctuatedWatermarkGenerator {
    fn on_event(&mut self, element: &StreamElement) -> Option<Watermark> {
        if let Some(max_ts) = self.max_timestamp {
            if element.event_time > max_ts {
                self.max_timestamp = Some(element.event_time);
            }
        } else {
            self.max_timestamp = Some(element.event_time);
        }

        if self.should_emit_watermark(element) {
            if let Some(max_ts) = self.max_timestamp {
                let new_watermark = Watermark::new(max_ts - self.config.max_out_of_orderness);

                if new_watermark > self.current_watermark {
                    self.current_watermark = new_watermark;
                    return Some(new_watermark);
                }
            }
        }

        None
    }

    fn on_periodic_emit(&mut self) -> Option<Watermark> {
        None
    }

    fn current_watermark(&self) -> Watermark {
        self.current_watermark
    }
}

/// Multi-source watermark manager.
pub struct MultiSourceWatermarkManager {
    source_watermarks: Arc<RwLock<BTreeMap<String, Watermark>>>,
    global_watermark: Arc<RwLock<Watermark>>,
}

impl MultiSourceWatermarkManager {
    /// Create a new multi-source watermark manager.
    pub fn new() -> Self {
        Self {
            source_watermarks: Arc::new(RwLock::new(BTreeMap::new())),
            global_watermark: Arc::new(RwLock::new(Watermark::min())),
        }
    }

    /// Update watermark for a source.
    pub async fn update_source_watermark(
        &self,
        source_id: String,
        watermark: Watermark,
    ) -> Result<()> {
        let mut watermarks = self.source_watermarks.write().await;
        watermarks.insert(source_id, watermark);

        let min_watermark = watermarks
            .values()
            .min()
            .copied()
            .unwrap_or(Watermark::min());

        let mut global = self.global_watermark.write().await;
        if min_watermark > *global {
            *global = min_watermark;
        }

        Ok(())
    }

    /// Get the global watermark (minimum of all source watermarks).
    pub async fn global_watermark(&self) -> Watermark {
        *self.global_watermark.read().await
    }

    /// Get watermark for a specific source.
    pub async fn source_watermark(&self, source_id: &str) -> Option<Watermark> {
        self.source_watermarks.read().await.get(source_id).copied()
    }

    /// Remove a source.
    pub async fn remove_source(&self, source_id: &str) -> Result<()> {
        let mut watermarks = self.source_watermarks.write().await;
        watermarks.remove(source_id);

        let min_watermark = watermarks
            .values()
            .min()
            .copied()
            .unwrap_or(Watermark::max());

        let mut global = self.global_watermark.write().await;
        *global = min_watermark;

        Ok(())
    }
}

impl Default for MultiSourceWatermarkManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watermark_creation() {
        let now = Utc::now();
        let wm = Watermark::new(now);
        assert_eq!(wm.timestamp, now);
    }

    #[test]
    fn test_watermark_ordering() {
        let wm1 = Watermark::new(Utc::now());
        let wm2 = Watermark::new(Utc::now() + Duration::seconds(10));

        assert!(wm1 < wm2);
        assert!(wm2 > wm1);
    }

    #[tokio::test]
    async fn test_periodic_watermark_generator() {
        let config = WatermarkConfig::default();
        let mut generator = PeriodicWatermarkGenerator::new(config);

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());
        generator.on_event(&elem);

        let wm = generator.on_periodic_emit();
        assert!(wm.is_some());
    }

    #[tokio::test]
    async fn test_multi_source_watermark_manager() {
        let manager = MultiSourceWatermarkManager::new();

        let wm1 = Watermark::new(Utc::now());
        let wm2 = Watermark::new(Utc::now() + Duration::seconds(10));

        manager
            .update_source_watermark("source1".to_string(), wm1)
            .await
            .expect("Test watermark update for source1 should succeed");
        manager
            .update_source_watermark("source2".to_string(), wm2)
            .await
            .expect("Test watermark update for source2 should succeed");

        let global = manager.global_watermark().await;
        assert_eq!(global, wm1);
    }

    #[tokio::test]
    async fn test_remove_source_watermark() {
        let manager = MultiSourceWatermarkManager::new();

        let wm1 = Watermark::new(Utc::now());
        let wm2 = Watermark::new(Utc::now() + Duration::seconds(10));

        manager
            .update_source_watermark("source1".to_string(), wm1)
            .await
            .expect("Test watermark update for source1 should succeed");
        manager
            .update_source_watermark("source2".to_string(), wm2)
            .await
            .expect("Test watermark update for source2 should succeed");

        manager
            .remove_source("source1")
            .await
            .expect("Test source removal should succeed");

        let global = manager.global_watermark().await;
        assert_eq!(global, wm2);
    }
}
