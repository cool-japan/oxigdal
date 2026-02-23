//! Feature streaming handler.

use crate::error::{Error, Result};
use crate::protocol::{ChangeType, Message};
use crate::stream::FeatureData;
use crate::subscription::SubscriptionManager;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Feature streaming handler.
pub struct FeatureHandler {
    /// Subscription manager
    subscriptions: Arc<SubscriptionManager>,
    /// Client message senders
    client_senders: Arc<DashMap<String, mpsc::UnboundedSender<Message>>>,
    /// Feature cache for change detection
    feature_cache: Arc<DashMap<String, String>>,
}

impl FeatureHandler {
    /// Create a new feature handler.
    pub fn new(subscriptions: Arc<SubscriptionManager>) -> Self {
        Self {
            subscriptions,
            client_senders: Arc::new(DashMap::new()),
            feature_cache: Arc::new(DashMap::new()),
        }
    }

    /// Register a client sender.
    pub fn register_client(&self, client_id: String, sender: mpsc::UnboundedSender<Message>) {
        self.client_senders.insert(client_id, sender);
    }

    /// Unregister a client.
    pub fn unregister_client(&self, client_id: &str) {
        self.client_senders.remove(client_id);
    }

    /// Stream a feature to subscribers.
    pub async fn stream_feature(&self, feature: FeatureData) -> Result<usize> {
        debug!("Streaming feature: layer={:?}", feature.layer);

        // Detect change type if not explicitly set
        let change_type = self.detect_change_type(&feature);

        // Find matching subscriptions
        let subscriptions = self
            .subscriptions
            .find_feature_subscriptions(feature.layer.as_deref());

        if subscriptions.is_empty() {
            return Ok(0);
        }

        let mut sent_count = 0;

        for subscription in subscriptions {
            let client_id = &subscription.client_id;

            // Apply attribute filters if present
            if let Some(ref filter) = subscription.filter {
                if let Some(ref attributes) = filter.attributes {
                    if !self.matches_attribute_filters(&feature, attributes)? {
                        continue;
                    }
                }
            }

            // Send feature to client
            if let Some(sender) = self.client_senders.get(client_id) {
                let message = Message::FeatureData {
                    subscription_id: subscription.id.clone(),
                    geojson: feature.geojson.clone(),
                    change_type,
                };

                if sender.send(message).is_ok() {
                    sent_count += 1;
                } else {
                    warn!("Failed to send feature to client {}", client_id);
                }
            }
        }

        // Update feature cache
        if let Ok(parsed) = feature.parse_json() {
            if let Some(id) = parsed.get("id") {
                let id_str = id.to_string();
                match change_type {
                    ChangeType::Deleted => {
                        self.feature_cache.remove(&id_str);
                    }
                    _ => {
                        self.feature_cache.insert(id_str, feature.geojson.clone());
                    }
                }
            }
        }

        Ok(sent_count)
    }

    /// Detect change type by comparing with cached version.
    fn detect_change_type(&self, feature: &FeatureData) -> ChangeType {
        // If explicitly set, use that
        if feature.change_type != ChangeType::Added {
            return feature.change_type;
        }

        // Try to parse feature ID
        if let Ok(parsed) = feature.parse_json() {
            if let Some(id) = parsed.get("id") {
                let id_str = id.to_string();

                if let Some(cached) = self.feature_cache.get(&id_str) {
                    // Feature exists - check if modified
                    if cached.value() != &feature.geojson {
                        return ChangeType::Updated;
                    }
                    return ChangeType::Added; // No change
                }
            }
        }

        // New feature
        ChangeType::Added
    }

    /// Check if feature matches attribute filters.
    fn matches_attribute_filters(
        &self,
        feature: &FeatureData,
        filters: &[(String, String)],
    ) -> Result<bool> {
        let parsed = feature.parse_json()?;

        let properties = parsed
            .get("properties")
            .ok_or_else(|| Error::InvalidMessage("Missing properties".to_string()))?;

        for (key, value) in filters {
            let prop_value = properties
                .get(key)
                .ok_or_else(|| Error::InvalidMessage(format!("Missing property: {}", key)))?;

            let prop_str = prop_value
                .as_str()
                .ok_or_else(|| Error::InvalidMessage(format!("Property not a string: {}", key)))?;

            if prop_str != value {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Stream multiple features.
    pub async fn stream_features(&self, features: Vec<FeatureData>) -> Result<usize> {
        let mut total_sent = 0;

        for feature in features {
            total_sent += self.stream_feature(feature).await?;
        }

        Ok(total_sent)
    }

    /// Stream a GeoJSON FeatureCollection.
    pub async fn stream_feature_collection(
        &self,
        geojson: &str,
        layer: Option<String>,
    ) -> Result<usize> {
        let parsed: serde_json::Value = serde_json::from_str(geojson)?;

        let features = parsed
            .get("features")
            .and_then(|f| f.as_array())
            .ok_or_else(|| Error::InvalidMessage("Invalid FeatureCollection".to_string()))?;

        let mut total_sent = 0;

        for feature in features {
            let feature_str = serde_json::to_string(feature)?;
            let feature_data = FeatureData::new(feature_str, ChangeType::Added, layer.clone());

            total_sent += self.stream_feature(feature_data).await?;
        }

        Ok(total_sent)
    }

    /// Notify feature deletion.
    pub async fn notify_deletion(&self, feature_id: &str, layer: Option<String>) -> Result<usize> {
        // Create a minimal GeoJSON feature for deletion
        let geojson = serde_json::json!({
            "type": "Feature",
            "id": feature_id,
            "geometry": null,
            "properties": {}
        });

        let feature = FeatureData::new(geojson.to_string(), ChangeType::Deleted, layer);

        self.stream_feature(feature).await
    }

    /// Apply spatial filter to features.
    pub fn apply_spatial_filter(
        &self,
        features: Vec<FeatureData>,
        bbox: [f64; 4],
    ) -> Result<Vec<FeatureData>> {
        let mut filtered = Vec::new();

        for feature in features {
            if self.feature_in_bbox(&feature, bbox)? {
                filtered.push(feature);
            }
        }

        Ok(filtered)
    }

    /// Check if a feature is within a bounding box.
    fn feature_in_bbox(&self, feature: &FeatureData, bbox: [f64; 4]) -> Result<bool> {
        let parsed = feature.parse_json()?;

        let geometry = parsed
            .get("geometry")
            .ok_or_else(|| Error::InvalidMessage("Missing geometry".to_string()))?;

        let geom_type = geometry
            .get("type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| Error::InvalidMessage("Missing geometry type".to_string()))?;

        match geom_type {
            "Point" => {
                let coords = geometry
                    .get("coordinates")
                    .and_then(|c| c.as_array())
                    .ok_or_else(|| Error::InvalidMessage("Invalid coordinates".to_string()))?;

                let lon = coords[0]
                    .as_f64()
                    .ok_or_else(|| Error::InvalidMessage("Invalid longitude".to_string()))?;
                let lat = coords[1]
                    .as_f64()
                    .ok_or_else(|| Error::InvalidMessage("Invalid latitude".to_string()))?;

                Ok(lon >= bbox[0] && lon <= bbox[2] && lat >= bbox[1] && lat <= bbox[3])
            }
            _ => {
                // For other geometry types, could implement proper bbox calculation
                // For now, include all
                Ok(true)
            }
        }
    }

    /// Clear feature cache.
    pub fn clear_cache(&self) {
        self.feature_cache.clear();
    }

    /// Get cache size.
    pub fn cache_size(&self) -> usize {
        self.feature_cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_feature_handler_creation() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = FeatureHandler::new(subscriptions);

        assert_eq!(handler.cache_size(), 0);
    }

    #[tokio::test]
    async fn test_stream_feature_collection() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = FeatureHandler::new(subscriptions);

        let geojson = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "id": "1",
                    "geometry": {
                        "type": "Point",
                        "coordinates": [0.0, 0.0]
                    },
                    "properties": {
                        "name": "Test"
                    }
                }
            ]
        }"#;

        // Should process without error (won't send to anyone as no subscriptions)
        let result = handler.stream_feature_collection(geojson, None).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_feature_in_bbox() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = FeatureHandler::new(subscriptions);

        let feature = FeatureData::new(
            r#"{
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [10.0, 20.0]
                },
                "properties": {}
            }"#
            .to_string(),
            ChangeType::Added,
            None,
        );

        // Point at (10, 20) should be in this bbox
        let result = handler.feature_in_bbox(&feature, [0.0, 0.0, 20.0, 30.0]);
        assert!(result.is_ok());
        if let Ok(in_bbox) = result {
            assert!(in_bbox);
        }

        // Point should not be in this bbox
        let result = handler.feature_in_bbox(&feature, [0.0, 0.0, 5.0, 5.0]);
        assert!(result.is_ok());
        if let Ok(in_bbox) = result {
            assert!(!in_bbox);
        }
    }

    #[tokio::test]
    async fn test_notify_deletion() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = FeatureHandler::new(subscriptions);

        let result = handler.notify_deletion("feature-123", None).await;
        assert!(result.is_ok());
    }
}
