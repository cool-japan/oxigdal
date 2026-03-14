//! Collection-level aggregation statistics for STAC collections.
//!
//! This module provides a streaming aggregation builder that processes STAC
//! items one by one and accumulates statistics without buffering all items in
//! memory.  The final [`CollectionStats`] object is produced by calling
//! [`CollectionAggregator::build`].

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── Descriptive statistics ─────────────────────────────────────────────────

/// Descriptive statistics for a numeric property across a set of items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NumericStats {
    /// Minimum observed value.
    pub min: f64,
    /// Maximum observed value.
    pub max: f64,
    /// Arithmetic mean.
    pub mean: f64,
    /// Population standard deviation.
    pub std_dev: f64,
    /// Number of observations.
    pub count: u64,
}

impl NumericStats {
    /// Computes statistics from a slice of values.
    ///
    /// Returns `None` when the slice is empty.
    pub fn from_values(values: &[f64]) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        let n = values.len() as f64;
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let mean = values.iter().sum::<f64>() / n;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        Some(Self {
            min,
            max,
            mean,
            std_dev: variance.sqrt(),
            count: values.len() as u64,
        })
    }
}

// ── Collection stats ───────────────────────────────────────────────────────

/// Aggregated statistics for a STAC collection.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CollectionStats {
    /// Collection identifier.
    pub collection_id: String,
    /// Total number of items ingested.
    pub item_count: u64,
    /// Temporal extent `[earliest_datetime, latest_datetime]` in RFC 3339.
    pub temporal_extent: Option<[String; 2]>,
    /// Spatial extent `[west, south, east, north]` in WGS 84.
    pub spatial_extent: Option<[f64; 4]>,
    /// Cloud cover statistics (populated only when EO items are ingested).
    pub cloud_cover: Option<NumericStats>,
    /// Value frequencies for categorical string properties.
    pub property_frequencies: HashMap<String, HashMap<String, u64>>,
    /// Count of items per platform identifier.
    pub platforms: HashMap<String, u64>,
}

// ── Aggregation builder ─────────────────────────────────────────────────────

/// Streaming aggregation builder for a STAC collection.
///
/// Create one, call [`ingest`] for each item, then call [`build`] to obtain
/// the final [`CollectionStats`].
///
/// [`ingest`]: CollectionAggregator::ingest
/// [`build`]: CollectionAggregator::build
pub struct CollectionAggregator {
    collection_id: String,
    item_count: u64,
    cloud_covers: Vec<f64>,
    platforms: HashMap<String, u64>,
    /// Running counts for categorical string properties.
    property_counts: HashMap<String, HashMap<String, u64>>,
    // Bounding-box accumulators.
    min_lon: f64,
    max_lon: f64,
    min_lat: f64,
    max_lat: f64,
    has_bbox: bool,
}

impl CollectionAggregator {
    /// Creates a new aggregator for the named collection.
    pub fn new(collection_id: impl Into<String>) -> Self {
        Self {
            collection_id: collection_id.into(),
            item_count: 0,
            cloud_covers: Vec::new(),
            platforms: HashMap::new(),
            property_counts: HashMap::new(),
            min_lon: f64::INFINITY,
            max_lon: f64::NEG_INFINITY,
            min_lat: f64::INFINITY,
            max_lat: f64::NEG_INFINITY,
            has_bbox: false,
        }
    }

    /// Ingests a single STAC item (as a JSON value) into the aggregator.
    ///
    /// Only fields that are present and parseable contribute to the statistics;
    /// missing or invalid fields are silently ignored.
    pub fn ingest(&mut self, item: &serde_json::Value) {
        self.item_count += 1;

        // ── Spatial extent from `bbox` ─────────────────────────────────────
        if let Some(bbox_arr) = item.get("bbox").and_then(|b| b.as_array()) {
            if bbox_arr.len() >= 4 {
                if let (Some(w), Some(s), Some(e), Some(n)) = (
                    bbox_arr[0].as_f64(),
                    bbox_arr[1].as_f64(),
                    bbox_arr[2].as_f64(),
                    bbox_arr[3].as_f64(),
                ) {
                    self.min_lon = self.min_lon.min(w);
                    self.max_lon = self.max_lon.max(e);
                    self.min_lat = self.min_lat.min(s);
                    self.max_lat = self.max_lat.max(n);
                    self.has_bbox = true;
                }
            }
        }

        let props = item.get("properties").and_then(|p| p.as_object());

        // ── EO cloud cover ─────────────────────────────────────────────────
        if let Some(cc) = props
            .and_then(|p| p.get("eo:cloud_cover"))
            .and_then(|v| v.as_f64())
        {
            self.cloud_covers.push(cc);
        }

        // ── Platform ───────────────────────────────────────────────────────
        if let Some(platform) = props
            .and_then(|p| p.get("platform"))
            .and_then(|v| v.as_str())
        {
            *self.platforms.entry(platform.to_string()).or_insert(0) += 1;
        }

        // ── Generic categorical string properties ──────────────────────────
        if let Some(props_map) = props {
            for (key, val) in props_map {
                if let Some(s) = val.as_str() {
                    *self
                        .property_counts
                        .entry(key.clone())
                        .or_default()
                        .entry(s.to_string())
                        .or_insert(0) += 1;
                }
            }
        }
    }

    /// Finalises the aggregation and returns the [`CollectionStats`].
    pub fn build(self) -> CollectionStats {
        let spatial_extent = if self.has_bbox {
            Some([self.min_lon, self.min_lat, self.max_lon, self.max_lat])
        } else {
            None
        };

        CollectionStats {
            collection_id: self.collection_id,
            item_count: self.item_count,
            temporal_extent: None, // Populated externally from item datetimes if required
            spatial_extent,
            cloud_cover: NumericStats::from_values(&self.cloud_covers),
            property_frequencies: self.property_counts,
            platforms: self.platforms,
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn eo_item(id: &str, cloud: f64, platform: &str, bbox: [f64; 4]) -> serde_json::Value {
        let [w, s, e, n] = bbox;
        json!({
            "id": id,
            "type": "Feature",
            "bbox": [w, s, e, n],
            "properties": {
                "eo:cloud_cover": cloud,
                "platform": platform,
                "constellation": "sentinel"
            }
        })
    }

    #[test]
    fn test_empty_aggregator() {
        let agg = CollectionAggregator::new("empty-col");
        let stats = agg.build();
        assert_eq!(stats.collection_id, "empty-col");
        assert_eq!(stats.item_count, 0);
        assert!(stats.spatial_extent.is_none());
        assert!(stats.cloud_cover.is_none());
    }

    #[test]
    fn test_ingest_bbox_spatial_extent() {
        let mut agg = CollectionAggregator::new("col-a");
        agg.ingest(&eo_item("i1", 0.0, "s2a", [-10.0, -5.0, 10.0, 5.0]));
        agg.ingest(&eo_item("i2", 0.0, "s2a", [-20.0, -15.0, 5.0, 15.0]));
        let stats = agg.build();
        let ext = stats.spatial_extent.expect("spatial_extent");
        assert!((ext[0] - (-20.0)).abs() < 1e-9); // west
        assert!((ext[1] - (-15.0)).abs() < 1e-9); // south
        assert!((ext[2] - 10.0).abs() < 1e-9); // east
        assert!((ext[3] - 15.0).abs() < 1e-9); // north
    }

    #[test]
    fn test_cloud_cover_stats() {
        let mut agg = CollectionAggregator::new("col-b");
        for cc in [0.0_f64, 25.0, 50.0, 75.0, 100.0] {
            agg.ingest(&eo_item("x", cc, "plat", [0.0, 0.0, 1.0, 1.0]));
        }
        let stats = agg.build();
        let cc = stats.cloud_cover.expect("cloud_cover");
        assert!((cc.min - 0.0).abs() < 1e-9);
        assert!((cc.max - 100.0).abs() < 1e-9);
        assert!((cc.mean - 50.0).abs() < 1e-9);
        assert_eq!(cc.count, 5);
    }

    #[test]
    fn test_platform_counts() {
        let mut agg = CollectionAggregator::new("col-c");
        agg.ingest(&eo_item("a", 10.0, "s2a", [0.0, 0.0, 1.0, 1.0]));
        agg.ingest(&eo_item("b", 20.0, "s2a", [0.0, 0.0, 1.0, 1.0]));
        agg.ingest(&eo_item("c", 30.0, "s2b", [0.0, 0.0, 1.0, 1.0]));
        let stats = agg.build();
        assert_eq!(stats.platforms["s2a"], 2);
        assert_eq!(stats.platforms["s2b"], 1);
    }

    #[test]
    fn test_numeric_stats_from_values() {
        let vals = vec![2.0_f64, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let ns = NumericStats::from_values(&vals).expect("stats");
        assert!((ns.mean - 5.0).abs() < 1e-9);
        assert!((ns.min - 2.0).abs() < 1e-9);
        assert!((ns.max - 9.0).abs() < 1e-9);
        // Population std dev = 2.0
        assert!((ns.std_dev - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_numeric_stats_empty() {
        assert!(NumericStats::from_values(&[]).is_none());
    }

    #[test]
    fn test_multiple_items_aggregated() {
        let mut agg = CollectionAggregator::new("multi");
        for i in 0..10 {
            agg.ingest(&eo_item(
                &format!("item-{}", i),
                (i * 10) as f64,
                "plat-x",
                [0.0, 0.0, 1.0, 1.0],
            ));
        }
        let stats = agg.build();
        assert_eq!(stats.item_count, 10);
        let cc = stats.cloud_cover.expect("cloud");
        assert!((cc.mean - 45.0).abs() < 1e-9);
        assert_eq!(stats.platforms["plat-x"], 10);
    }

    #[test]
    fn test_property_frequencies() {
        let mut agg = CollectionAggregator::new("freq-col");
        agg.ingest(&eo_item("i1", 10.0, "s2a", [0.0, 0.0, 1.0, 1.0]));
        agg.ingest(&eo_item("i2", 20.0, "s2a", [0.0, 0.0, 1.0, 1.0]));
        agg.ingest(&eo_item("i3", 30.0, "s2b", [0.0, 0.0, 1.0, 1.0]));
        let stats = agg.build();
        // "platform" is captured as a categorical property
        let platform_freqs = &stats.property_frequencies["platform"];
        assert_eq!(platform_freqs["s2a"], 2);
        assert_eq!(platform_freqs["s2b"], 1);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut agg = CollectionAggregator::new("rt-col");
        agg.ingest(&eo_item("rt-1", 15.0, "s2a", [-5.0, -5.0, 5.0, 5.0]));
        let stats = agg.build();
        let json = serde_json::to_string(&stats).expect("serialize");
        let back: CollectionStats = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(stats, back);
    }
}
