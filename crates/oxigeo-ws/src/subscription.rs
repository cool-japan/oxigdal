//! Subscription management for WebSocket clients.

use crate::error::{Error, Result};
use crate::protocol::{EventType, SubscriptionFilter};
use dashmap::DashMap;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;
use uuid::Uuid;

/// Subscription type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionType {
    /// Tile subscription with bbox and zoom range
    Tiles {
        /// Bounding box [min_x, min_y, max_x, max_y]
        bbox: [i64; 4], // Using i64 for exact comparison
        /// Zoom level range
        zoom_range: Range<u8>,
    },
    /// Feature subscription with filters
    Features {
        /// Layer name
        layer: Option<String>,
    },
    /// Event subscription
    Events {
        /// Event types
        event_types: HashSet<EventType>,
    },
}

/// A subscription to WebSocket updates.
#[derive(Debug, Clone)]
pub struct Subscription {
    /// Unique subscription ID
    pub id: String,
    /// Client ID that owns this subscription
    pub client_id: String,
    /// Subscription type
    pub subscription_type: SubscriptionType,
    /// Optional filter
    pub filter: Option<SubscriptionFilter>,
}

impl Subscription {
    /// Create a new subscription.
    pub fn new(
        client_id: String,
        subscription_type: SubscriptionType,
        filter: Option<SubscriptionFilter>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            client_id,
            subscription_type,
            filter,
        }
    }

    /// Create a tile subscription.
    pub fn tiles(
        client_id: String,
        bbox: [f64; 4],
        zoom_range: Range<u8>,
        filter: Option<SubscriptionFilter>,
    ) -> Self {
        // Convert f64 bbox to i64 for exact comparison (multiply by 1e6)
        let bbox_i64 = [
            (bbox[0] * 1_000_000.0) as i64,
            (bbox[1] * 1_000_000.0) as i64,
            (bbox[2] * 1_000_000.0) as i64,
            (bbox[3] * 1_000_000.0) as i64,
        ];

        Self::new(
            client_id,
            SubscriptionType::Tiles {
                bbox: bbox_i64,
                zoom_range,
            },
            filter,
        )
    }

    /// Create a feature subscription.
    pub fn features(
        client_id: String,
        layer: Option<String>,
        filter: Option<SubscriptionFilter>,
    ) -> Self {
        Self::new(client_id, SubscriptionType::Features { layer }, filter)
    }

    /// Create an event subscription.
    pub fn events(
        client_id: String,
        event_types: HashSet<EventType>,
        filter: Option<SubscriptionFilter>,
    ) -> Self {
        Self::new(client_id, SubscriptionType::Events { event_types }, filter)
    }

    /// Check if a tile matches this subscription.
    pub fn matches_tile(&self, tile_x: u32, tile_y: u32, zoom: u8) -> bool {
        match &self.subscription_type {
            SubscriptionType::Tiles { bbox, zoom_range } => {
                if !zoom_range.contains(&zoom) {
                    return false;
                }

                // Convert tile coordinates to bbox
                let n = 2_u32.pow(zoom.into());
                let tile_bbox = Self::tile_to_bbox(tile_x, tile_y, n);

                // Convert back to i64 for comparison
                let tile_bbox_i64 = [
                    (tile_bbox[0] * 1_000_000.0) as i64,
                    (tile_bbox[1] * 1_000_000.0) as i64,
                    (tile_bbox[2] * 1_000_000.0) as i64,
                    (tile_bbox[3] * 1_000_000.0) as i64,
                ];

                // Check if bboxes overlap
                Self::bboxes_overlap(bbox, &tile_bbox_i64)
            }
            _ => false,
        }
    }

    /// Convert tile coordinates to geographic bbox.
    fn tile_to_bbox(x: u32, y: u32, n: u32) -> [f64; 4] {
        let n_f64 = n as f64;
        let min_lon = (x as f64 / n_f64) * 360.0 - 180.0;
        let max_lon = ((x + 1) as f64 / n_f64) * 360.0 - 180.0;

        let lat_rad = |y_val: f64| -> f64 {
            let n_rad = std::f64::consts::PI - 2.0 * std::f64::consts::PI * y_val / n_f64;
            n_rad.sinh().atan().to_degrees()
        };

        let max_lat = lat_rad(y as f64);
        let min_lat = lat_rad((y + 1) as f64);

        [min_lon, min_lat, max_lon, max_lat]
    }

    /// Check if two bboxes overlap.
    fn bboxes_overlap(bbox1: &[i64; 4], bbox2: &[i64; 4]) -> bool {
        bbox1[0] <= bbox2[2] && bbox1[2] >= bbox2[0] && bbox1[1] <= bbox2[3] && bbox1[3] >= bbox2[1]
    }

    /// Check if a feature matches this subscription.
    pub fn matches_feature(&self, layer: Option<&str>) -> bool {
        match &self.subscription_type {
            SubscriptionType::Features { layer: sub_layer } => {
                if let Some(sub_layer) = sub_layer {
                    layer == Some(sub_layer.as_str())
                } else {
                    true // Match all layers
                }
            }
            _ => false,
        }
    }

    /// Check if an event matches this subscription.
    pub fn matches_event(&self, event_type: EventType) -> bool {
        match &self.subscription_type {
            SubscriptionType::Events { event_types } => event_types.contains(&event_type),
            _ => false,
        }
    }
}

/// Manager for all subscriptions.
pub struct SubscriptionManager {
    /// All subscriptions by ID
    subscriptions: Arc<DashMap<String, Subscription>>,
    /// Subscriptions by client ID
    client_subscriptions: Arc<DashMap<String, HashSet<String>>>,
}

impl SubscriptionManager {
    /// Create a new subscription manager.
    pub fn new() -> Self {
        Self {
            subscriptions: Arc::new(DashMap::new()),
            client_subscriptions: Arc::new(DashMap::new()),
        }
    }

    /// Add a subscription.
    pub fn add(&self, subscription: Subscription) -> Result<String> {
        let sub_id = subscription.id.clone();
        let client_id = subscription.client_id.clone();

        self.subscriptions.insert(sub_id.clone(), subscription);

        self.client_subscriptions
            .entry(client_id)
            .or_default()
            .insert(sub_id.clone());

        Ok(sub_id)
    }

    /// Remove a subscription.
    pub fn remove(&self, subscription_id: &str) -> Result<()> {
        if let Some((_, subscription)) = self.subscriptions.remove(subscription_id) {
            if let Some(mut client_subs) =
                self.client_subscriptions.get_mut(&subscription.client_id)
            {
                client_subs.remove(subscription_id);
            }
            Ok(())
        } else {
            Err(Error::NotFound(format!(
                "Subscription not found: {}",
                subscription_id
            )))
        }
    }

    /// Remove all subscriptions for a client.
    pub fn remove_client(&self, client_id: &str) -> Result<()> {
        if let Some((_, sub_ids)) = self.client_subscriptions.remove(client_id) {
            for sub_id in sub_ids {
                self.subscriptions.remove(&sub_id);
            }
        }
        Ok(())
    }

    /// Get a subscription by ID.
    pub fn get(&self, subscription_id: &str) -> Option<Subscription> {
        self.subscriptions.get(subscription_id).map(|s| s.clone())
    }

    /// Get all subscriptions for a client.
    pub fn get_client_subscriptions(&self, client_id: &str) -> Vec<Subscription> {
        if let Some(sub_ids) = self.client_subscriptions.get(client_id) {
            sub_ids.iter().filter_map(|id| self.get(id)).collect()
        } else {
            Vec::new()
        }
    }

    /// Find subscriptions matching a tile.
    pub fn find_tile_subscriptions(&self, tile_x: u32, tile_y: u32, zoom: u8) -> Vec<Subscription> {
        self.subscriptions
            .iter()
            .filter(|entry| entry.value().matches_tile(tile_x, tile_y, zoom))
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Find subscriptions matching a feature.
    pub fn find_feature_subscriptions(&self, layer: Option<&str>) -> Vec<Subscription> {
        self.subscriptions
            .iter()
            .filter(|entry| entry.value().matches_feature(layer))
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Find subscriptions matching an event.
    pub fn find_event_subscriptions(&self, event_type: EventType) -> Vec<Subscription> {
        self.subscriptions
            .iter()
            .filter(|entry| entry.value().matches_event(event_type))
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get total subscription count.
    pub fn count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Get client count.
    pub fn client_count(&self) -> usize {
        self.client_subscriptions.len()
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_creation() {
        let sub = Subscription::tiles(
            "client-1".to_string(),
            [-180.0, -90.0, 180.0, 90.0],
            0..14,
            None,
        );

        assert_eq!(sub.client_id, "client-1");
        assert!(!sub.id.is_empty());
    }

    #[test]
    fn test_tile_matching() {
        let sub = Subscription::tiles(
            "client-1".to_string(),
            [-180.0, -90.0, 0.0, 0.0],
            5..10,
            None,
        );

        // Should match tiles in the southwest quadrant at zoom 5-9
        // At zoom 5 (n=32), tile (0,0) is northwest, tile (0,31) is southwest
        // At zoom 5, southwest quadrant uses x=[0,15], y=[16,31]
        assert!(sub.matches_tile(0, 31, 5)); // Southwest corner tile
        assert!(sub.matches_tile(15, 16, 5)); // Near equator/prime meridian boundary
        // At zoom 8 (n=256), southwest quadrant uses x=[0,127], y=[128,255]
        assert!(sub.matches_tile(100, 200, 8)); // Deep in southwest quadrant

        // Should not match tiles outside zoom range
        assert!(!sub.matches_tile(0, 31, 4)); // Correct tile but wrong zoom
        assert!(!sub.matches_tile(0, 31, 10)); // Correct tile but wrong zoom

        // Should not match tiles outside bbox
        assert!(!sub.matches_tile(0, 0, 5)); // Northwest, not southwest
        assert!(!sub.matches_tile(16, 16, 8)); // Northeast, not southwest
    }

    #[test]
    fn test_subscription_manager() {
        let manager = SubscriptionManager::new();

        let sub1 = Subscription::tiles(
            "client-1".to_string(),
            [-180.0, -90.0, 0.0, 0.0],
            0..14,
            None,
        );
        let sub_id1 = sub1.id.clone();

        assert!(manager.add(sub1).is_ok());

        assert_eq!(manager.count(), 1);
        assert_eq!(manager.client_count(), 1);

        let retrieved = manager.get(&sub_id1);
        assert!(retrieved.is_some());
        if let Some(ref sub) = retrieved {
            assert_eq!(sub.id, sub_id1);
        }

        assert!(manager.remove(&sub_id1).is_ok());
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_find_tile_subscriptions() {
        let manager = SubscriptionManager::new();

        // client-1: southwest quadrant
        let sub1 = Subscription::tiles(
            "client-1".to_string(),
            [-180.0, -90.0, 0.0, 0.0],
            5..10,
            None,
        );

        // client-2: northeast quadrant
        let sub2 =
            Subscription::tiles("client-2".to_string(), [0.0, 0.0, 180.0, 90.0], 5..10, None);

        assert!(manager.add(sub1).is_ok());
        assert!(manager.add(sub2).is_ok());

        // Find subscriptions for a tile in the southwest quadrant
        // At zoom 5, tile (0,31) is in the southwest corner
        let matches = manager.find_tile_subscriptions(0, 31, 5);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].client_id, "client-1");

        // Find subscriptions for a tile in the northeast quadrant
        // At zoom 5, tile (31,0) is in the northeast corner
        let matches2 = manager.find_tile_subscriptions(31, 0, 5);
        assert_eq!(matches2.len(), 1);
        assert_eq!(matches2[0].client_id, "client-2");
    }

    #[test]
    fn test_remove_client() {
        let manager = SubscriptionManager::new();

        let sub1 = Subscription::tiles(
            "client-1".to_string(),
            [-180.0, -90.0, 0.0, 0.0],
            0..14,
            None,
        );

        let sub2 = Subscription::features("client-1".to_string(), Some("layer1".to_string()), None);

        assert!(manager.add(sub1).is_ok());
        assert!(manager.add(sub2).is_ok());

        assert_eq!(manager.count(), 2);

        assert!(manager.remove_client("client-1").is_ok());
        assert_eq!(manager.count(), 0);
    }
}
