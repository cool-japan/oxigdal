//! Aggregation support for STAC API.
//!
//! This module provides aggregation functions for STAC search results.

use crate::error::{Result, StacError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Aggregation request for STAC API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationRequest {
    /// Aggregations to compute.
    pub aggregations: Vec<Aggregation>,
}

/// Single aggregation specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Aggregation {
    /// Count aggregation.
    #[serde(rename = "count")]
    Count {
        /// Name of the aggregation.
        name: String,
        /// Field to count (optional, counts all if not specified).
        #[serde(skip_serializing_if = "Option::is_none")]
        field: Option<String>,
    },

    /// Sum aggregation.
    #[serde(rename = "sum")]
    Sum {
        /// Name of the aggregation.
        name: String,
        /// Field to sum.
        field: String,
    },

    /// Average aggregation.
    #[serde(rename = "avg")]
    Avg {
        /// Name of the aggregation.
        name: String,
        /// Field to average.
        field: String,
    },

    /// Min aggregation.
    #[serde(rename = "min")]
    Min {
        /// Name of the aggregation.
        name: String,
        /// Field to find minimum.
        field: String,
    },

    /// Max aggregation.
    #[serde(rename = "max")]
    Max {
        /// Name of the aggregation.
        name: String,
        /// Field to find maximum.
        field: String,
    },

    /// Stats aggregation (count, sum, avg, min, max).
    #[serde(rename = "stats")]
    Stats {
        /// Name of the aggregation.
        name: String,
        /// Field to compute statistics.
        field: String,
    },

    /// Terms aggregation (frequency count by value).
    #[serde(rename = "terms")]
    Terms {
        /// Name of the aggregation.
        name: String,
        /// Field to group by.
        field: String,
        /// Maximum number of buckets to return.
        #[serde(skip_serializing_if = "Option::is_none")]
        size: Option<u32>,
    },

    /// Histogram aggregation (bucketed numeric values).
    #[serde(rename = "histogram")]
    Histogram {
        /// Name of the aggregation.
        name: String,
        /// Field to histogram.
        field: String,
        /// Interval for buckets.
        interval: f64,
        /// Minimum value.
        #[serde(skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        /// Maximum value.
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
    },

    /// Date histogram aggregation (bucketed temporal values).
    #[serde(rename = "date_histogram")]
    DateHistogram {
        /// Name of the aggregation.
        name: String,
        /// Field to histogram.
        field: String,
        /// Calendar interval (e.g., "1d", "1M", "1y").
        interval: String,
        /// Time zone (e.g., "UTC", "America/New_York").
        #[serde(skip_serializing_if = "Option::is_none")]
        time_zone: Option<String>,
    },

    /// Geohash grid aggregation (spatial bucketing).
    #[serde(rename = "geohash_grid")]
    GeohashGrid {
        /// Name of the aggregation.
        name: String,
        /// Field containing geometries.
        field: String,
        /// Geohash precision (1-12).
        precision: u8,
    },
}

/// Aggregation response from STAC API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResponse {
    /// Aggregation results keyed by aggregation name.
    pub aggregations: HashMap<String, AggregationResult>,
}

/// Result of a single aggregation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AggregationResult {
    /// Simple numeric value.
    Value(f64),

    /// Statistics result.
    Stats {
        /// Count of values.
        count: u64,
        /// Sum of values.
        sum: f64,
        /// Average of values.
        avg: f64,
        /// Minimum value.
        min: f64,
        /// Maximum value.
        max: f64,
    },

    /// Bucketed results.
    Buckets {
        /// List of buckets.
        buckets: Vec<Bucket>,
    },
}

/// Bucket in a terms or histogram aggregation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    /// Bucket key (value or range).
    pub key: Value,
    /// Count of items in this bucket.
    pub doc_count: u64,
    /// Sub-aggregations (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregations: Option<HashMap<String, AggregationResult>>,
}

impl AggregationRequest {
    /// Creates a new aggregation request.
    pub fn new() -> Self {
        Self {
            aggregations: Vec::new(),
        }
    }

    /// Adds an aggregation to the request.
    pub fn add(mut self, aggregation: Aggregation) -> Self {
        self.aggregations.push(aggregation);
        self
    }

    /// Creates a count aggregation.
    pub fn count(name: impl Into<String>) -> Aggregation {
        Aggregation::Count {
            name: name.into(),
            field: None,
        }
    }

    /// Creates a sum aggregation.
    pub fn sum(name: impl Into<String>, field: impl Into<String>) -> Aggregation {
        Aggregation::Sum {
            name: name.into(),
            field: field.into(),
        }
    }

    /// Creates an average aggregation.
    pub fn avg(name: impl Into<String>, field: impl Into<String>) -> Aggregation {
        Aggregation::Avg {
            name: name.into(),
            field: field.into(),
        }
    }

    /// Creates a min aggregation.
    pub fn min(name: impl Into<String>, field: impl Into<String>) -> Aggregation {
        Aggregation::Min {
            name: name.into(),
            field: field.into(),
        }
    }

    /// Creates a max aggregation.
    pub fn max(name: impl Into<String>, field: impl Into<String>) -> Aggregation {
        Aggregation::Max {
            name: name.into(),
            field: field.into(),
        }
    }

    /// Creates a stats aggregation.
    pub fn stats(name: impl Into<String>, field: impl Into<String>) -> Aggregation {
        Aggregation::Stats {
            name: name.into(),
            field: field.into(),
        }
    }

    /// Creates a terms aggregation.
    pub fn terms(name: impl Into<String>, field: impl Into<String>) -> Aggregation {
        Aggregation::Terms {
            name: name.into(),
            field: field.into(),
            size: None,
        }
    }

    /// Creates a histogram aggregation.
    pub fn histogram(
        name: impl Into<String>,
        field: impl Into<String>,
        interval: f64,
    ) -> Aggregation {
        Aggregation::Histogram {
            name: name.into(),
            field: field.into(),
            interval,
            min: None,
            max: None,
        }
    }

    /// Creates a date histogram aggregation.
    pub fn date_histogram(
        name: impl Into<String>,
        field: impl Into<String>,
        interval: impl Into<String>,
    ) -> Aggregation {
        Aggregation::DateHistogram {
            name: name.into(),
            field: field.into(),
            interval: interval.into(),
            time_zone: None,
        }
    }

    /// Creates a geohash grid aggregation.
    pub fn geohash_grid(
        name: impl Into<String>,
        field: impl Into<String>,
        precision: u8,
    ) -> Result<Aggregation> {
        if !(1..=12).contains(&precision) {
            return Err(StacError::InvalidFieldValue {
                field: "precision".to_string(),
                reason: "must be between 1 and 12".to_string(),
            });
        }

        Ok(Aggregation::GeohashGrid {
            name: name.into(),
            field: field.into(),
            precision,
        })
    }
}

impl Default for AggregationRequest {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregationResponse {
    /// Gets a single value result by name.
    pub fn get_value(&self, name: &str) -> Option<f64> {
        self.aggregations.get(name).and_then(|result| {
            if let AggregationResult::Value(v) = result {
                Some(*v)
            } else {
                None
            }
        })
    }

    /// Gets stats result by name.
    pub fn get_stats(&self, name: &str) -> Option<(u64, f64, f64, f64, f64)> {
        self.aggregations.get(name).and_then(|result| {
            if let AggregationResult::Stats {
                count,
                sum,
                avg,
                min,
                max,
            } = result
            {
                Some((*count, *sum, *avg, *min, *max))
            } else {
                None
            }
        })
    }

    /// Gets buckets result by name.
    pub fn get_buckets(&self, name: &str) -> Option<&Vec<Bucket>> {
        self.aggregations.get(name).and_then(|result| {
            if let AggregationResult::Buckets { buckets } = result {
                Some(buckets)
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregation_request_builder() {
        let request = AggregationRequest::new()
            .add(AggregationRequest::count("total"))
            .add(AggregationRequest::avg("avg_cloud_cover", "eo:cloud_cover"))
            .add(AggregationRequest::terms("platforms", "platform"));

        assert_eq!(request.aggregations.len(), 3);
    }

    #[test]
    fn test_aggregation_serialization() {
        let request = AggregationRequest::new()
            .add(AggregationRequest::count("total"))
            .add(AggregationRequest::stats(
                "cloud_cover_stats",
                "eo:cloud_cover",
            ));

        let json = serde_json::to_string(&request).expect("Failed to serialize");
        assert!(json.contains("count"));
        assert!(json.contains("stats"));
    }

    #[test]
    fn test_geohash_grid_validation() {
        let valid = AggregationRequest::geohash_grid("geo_grid", "geometry", 5);
        assert!(valid.is_ok());

        let invalid = AggregationRequest::geohash_grid("geo_grid", "geometry", 15);
        assert!(invalid.is_err());
    }

    #[test]
    fn test_aggregation_response() {
        let mut aggregations = HashMap::new();
        aggregations.insert("total".to_string(), AggregationResult::Value(1000.0));
        aggregations.insert(
            "stats".to_string(),
            AggregationResult::Stats {
                count: 100,
                sum: 1500.0,
                avg: 15.0,
                min: 0.0,
                max: 100.0,
            },
        );

        let response = AggregationResponse { aggregations };

        assert_eq!(response.get_value("total"), Some(1000.0));
        assert_eq!(
            response.get_stats("stats"),
            Some((100, 1500.0, 15.0, 0.0, 100.0))
        );
    }
}
