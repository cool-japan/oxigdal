//! Tile streaming handler.

use crate::error::{Error, Result};
use crate::protocol::Message;
use crate::stream::{BackpressureController, DeltaEncoder, TileData};
use crate::subscription::SubscriptionManager;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// A pluggable source of encoded map tiles.
///
/// This is the integration point between the WebSocket tile-streaming layer and
/// a concrete tile renderer (for example an `oxigeo-server` raster tile pipeline
/// or a vector-tile / MVT builder). Given a tile coordinate it returns the fully
/// encoded [`TileData`] (bytes + MIME type). Tile rendering is CPU-bound, so the
/// callback is synchronous and invoked from the async streaming path.
pub type TileGenerator = Arc<dyn Fn(u32, u32, u8) -> Result<TileData> + Send + Sync>;

/// Tile streaming handler.
pub struct TileHandler {
    /// Subscription manager
    subscriptions: Arc<SubscriptionManager>,
    /// Client message senders
    client_senders: Arc<DashMap<String, mpsc::UnboundedSender<Message>>>,
    /// Delta encoder for efficient updates
    delta_encoder: Arc<DeltaEncoder>,
    /// Backpressure controller
    backpressure: Arc<DashMap<String, BackpressureController>>,
    /// Enable delta encoding
    enable_delta: bool,
    /// Optional real tile source used for viewport/prefetch generation
    tile_source: Option<TileGenerator>,
}

impl TileHandler {
    /// Create a new tile handler.
    ///
    /// No tile source is configured by default; call [`TileHandler::with_tile_source`]
    /// or [`TileHandler::set_tile_source`] to wire a real tile renderer before using
    /// [`TileHandler::generate_viewport_tiles`] / [`TileHandler::prefetch_tiles`].
    pub fn new(subscriptions: Arc<SubscriptionManager>) -> Self {
        Self {
            subscriptions,
            client_senders: Arc::new(DashMap::new()),
            delta_encoder: Arc::new(DeltaEncoder::new()),
            backpressure: Arc::new(DashMap::new()),
            enable_delta: true,
            tile_source: None,
        }
    }

    /// Attach a real tile source (builder style).
    #[must_use]
    pub fn with_tile_source(mut self, source: TileGenerator) -> Self {
        self.tile_source = Some(source);
        self
    }

    /// Attach or replace the real tile source.
    pub fn set_tile_source(&mut self, source: TileGenerator) {
        self.tile_source = Some(source);
    }

    /// Whether a real tile source has been configured.
    #[must_use]
    pub fn has_tile_source(&self) -> bool {
        self.tile_source.is_some()
    }

    /// Register a client sender.
    pub fn register_client(&self, client_id: String, sender: mpsc::UnboundedSender<Message>) {
        self.client_senders.insert(client_id.clone(), sender);
        self.backpressure
            .insert(client_id, BackpressureController::new(1000));
    }

    /// Unregister a client.
    pub fn unregister_client(&self, client_id: &str) {
        self.client_senders.remove(client_id);
        self.backpressure.remove(client_id);
    }

    /// Stream a tile to subscribers.
    pub async fn stream_tile(&self, tile: TileData) -> Result<usize> {
        let (x, y, zoom) = tile.coords();
        debug!("Streaming tile: x={}, y={}, z={}", x, y, zoom);

        // Find matching subscriptions
        let subscriptions = self.subscriptions.find_tile_subscriptions(x, y, zoom);

        if subscriptions.is_empty() {
            return Ok(0);
        }

        let mut sent_count = 0;

        for subscription in subscriptions {
            let client_id = &subscription.client_id;

            // Check backpressure
            if let Some(controller) = self.backpressure.get(client_id)
                && controller.should_drop()
            {
                warn!("Dropping tile for client {} due to backpressure", client_id);
                continue;
            }

            // Get tile data (with optional delta encoding)
            let data = if self.enable_delta {
                self.delta_encoder.encode(&tile)?
            } else {
                tile.data.to_vec()
            };

            // Send tile to client
            if let Some(sender) = self.client_senders.get(client_id) {
                let message = Message::TileData {
                    subscription_id: subscription.id.clone(),
                    tile: (x, y, zoom),
                    data,
                    mime_type: tile.mime_type.clone(),
                };

                if sender.send(message).is_ok() {
                    sent_count += 1;
                } else {
                    warn!("Failed to send tile to client {}", client_id);
                }
            }
        }

        Ok(sent_count)
    }

    /// Stream multiple tiles.
    pub async fn stream_tiles(&self, tiles: Vec<TileData>) -> Result<usize> {
        let mut total_sent = 0;

        for tile in tiles {
            total_sent += self.stream_tile(tile).await?;
        }

        Ok(total_sent)
    }

    /// Generate tiles for a viewport using the configured tile source.
    ///
    /// Returns [`Error::NotFound`] when no tile source has been wired, rather
    /// than emitting fabricated placeholder bytes. Individual tile failures are
    /// logged and skipped so a single bad tile does not abort the whole viewport.
    pub async fn generate_viewport_tiles(&self, bbox: [f64; 4], zoom: u8) -> Result<Vec<TileData>> {
        // Convert bbox to tile coordinates
        let tiles = Self::bbox_to_tiles(bbox, zoom);

        let source = self.tile_source.as_ref().ok_or_else(|| {
            Error::NotFound(
                "no tile source configured; call TileHandler::set_tile_source to \
                 wire a real tile renderer before generating viewport tiles"
                    .to_string(),
            )
        })?;

        let mut tile_data = Vec::with_capacity(tiles.len());

        for (x, y) in tiles {
            match source(x, y, zoom) {
                Ok(tile) => tile_data.push(tile),
                Err(e) => {
                    warn!("Tile source failed for ({}, {}, z={}): {}", x, y, zoom, e);
                }
            }
        }

        Ok(tile_data)
    }

    /// Convert bbox to tile coordinates.
    fn bbox_to_tiles(bbox: [f64; 4], zoom: u8) -> Vec<(u32, u32)> {
        let n = 2_u32.pow(zoom.into());

        let min_x = Self::lon_to_tile_x(bbox[0], n);
        let max_x = Self::lon_to_tile_x(bbox[2], n);
        let min_y = Self::lat_to_tile_y(bbox[3], n); // Note: inverted
        let max_y = Self::lat_to_tile_y(bbox[1], n);

        let mut tiles = Vec::new();
        for x in min_x..=max_x {
            for y in min_y..=max_y {
                tiles.push((x, y));
            }
        }

        tiles
    }

    /// Convert longitude to tile X coordinate.
    fn lon_to_tile_x(lon: f64, n: u32) -> u32 {
        let x = ((lon + 180.0) / 360.0 * n as f64).floor() as u32;
        // Clamp to valid range [0, n-1] - longitude 180.0 wraps to -180.0
        x.min(n.saturating_sub(1))
    }

    /// Convert latitude to tile Y coordinate.
    fn lat_to_tile_y(lat: f64, n: u32) -> u32 {
        let lat_rad = lat.to_radians();
        ((1.0 - lat_rad.tan().asinh() / std::f64::consts::PI) / 2.0 * n as f64).floor() as u32
    }

    /// Prefetch tiles based on movement patterns.
    pub async fn prefetch_tiles(
        &self,
        current_bbox: [f64; 4],
        zoom: u8,
        direction: Direction,
    ) -> Result<Vec<TileData>> {
        // Calculate prefetch bbox based on direction
        let prefetch_bbox = Self::expand_bbox(current_bbox, direction, 0.5);

        // Generate tiles for prefetch area
        self.generate_viewport_tiles(prefetch_bbox, zoom).await
    }

    /// Expand bbox in a direction.
    fn expand_bbox(bbox: [f64; 4], direction: Direction, factor: f64) -> [f64; 4] {
        let width = bbox[2] - bbox[0];
        let height = bbox[3] - bbox[1];

        match direction {
            Direction::North => [bbox[0], bbox[1], bbox[2], bbox[3] + height * factor],
            Direction::South => [bbox[0], bbox[1] - height * factor, bbox[2], bbox[3]],
            Direction::East => [bbox[0], bbox[1], bbox[2] + width * factor, bbox[3]],
            Direction::West => [bbox[0] - width * factor, bbox[1], bbox[2], bbox[3]],
        }
    }

    /// Clear delta encoder cache.
    pub fn clear_delta_cache(&self) {
        self.delta_encoder.clear();
    }

    /// Get delta cache size.
    pub fn delta_cache_size(&self) -> usize {
        self.delta_encoder.cache_size()
    }

    /// Enable or disable delta encoding.
    pub fn set_delta_enabled(&mut self, enabled: bool) {
        self.enable_delta = enabled;
    }
}

/// Movement direction for prefetching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Moving north
    North,
    /// Moving south
    South,
    /// Moving east
    East,
    /// Moving west
    West,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bbox_to_tiles() {
        let bbox = [-180.0, -85.0, 180.0, 85.0];
        let tiles = TileHandler::bbox_to_tiles(bbox, 0);

        // At zoom 0, should have 1 tile
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0], (0, 0));
    }

    #[test]
    fn test_lon_to_tile_x() {
        let n = 2_u32.pow(1); // zoom 1 = 2 tiles (valid indices: 0, 1)

        assert_eq!(TileHandler::lon_to_tile_x(-180.0, n), 0);
        assert_eq!(TileHandler::lon_to_tile_x(0.0, n), 1);
        // Longitude 180.0 wraps to -180.0, should clamp to max valid tile (n-1)
        assert_eq!(TileHandler::lon_to_tile_x(180.0, n), 1);
    }

    #[test]
    fn test_expand_bbox() {
        let bbox = [0.0, 0.0, 10.0, 10.0];

        let north = TileHandler::expand_bbox(bbox, Direction::North, 0.5);
        assert_eq!(north, [0.0, 0.0, 10.0, 15.0]);

        let east = TileHandler::expand_bbox(bbox, Direction::East, 0.5);
        assert_eq!(east, [0.0, 0.0, 15.0, 10.0]);
    }

    #[tokio::test]
    async fn test_tile_handler_creation() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = TileHandler::new(subscriptions);

        assert_eq!(handler.delta_cache_size(), 0);
        assert!(handler.enable_delta);
        assert!(!handler.has_tile_source());
    }

    #[tokio::test]
    async fn test_generate_viewport_tiles_without_source_errors() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = TileHandler::new(subscriptions);

        // Without a configured source we must surface an honest error, never
        // fabricated placeholder tiles.
        let result = handler
            .generate_viewport_tiles([-180.0, -85.0, 180.0, 85.0], 0)
            .await;
        assert!(matches!(result, Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn test_generate_viewport_tiles_with_source() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let source: TileGenerator = Arc::new(|x, y, z| {
            // A trivial but real generator: encode the coordinate into the bytes
            // so we can assert the returned data is genuinely per-tile content.
            let data = vec![x as u8, y as u8, z, 0xAB];
            Ok(TileData::new(x, y, z, data, "image/png".to_string()))
        });
        let handler = TileHandler::new(subscriptions).with_tile_source(source);
        assert!(handler.has_tile_source());

        let tiles = handler
            .generate_viewport_tiles([-180.0, -85.0, 180.0, 85.0], 0)
            .await
            .expect("generation should succeed with a source");

        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].coords(), (0, 0, 0));
        assert_eq!(tiles[0].mime_type, "image/png");
        assert_eq!(tiles[0].data.to_vec(), vec![0u8, 0u8, 0u8, 0xABu8]);
    }
}
