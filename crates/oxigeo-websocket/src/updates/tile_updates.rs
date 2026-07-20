//! Tile update management and notifications

use crate::error::{Error, Result};
use crate::protocol::message::{Message, MessageType, Payload, TilePayload};
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Tile update type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileUpdateType {
    /// Full tile replacement
    Full,
    /// Incremental delta update
    Delta,
    /// Tile invalidation (needs refresh)
    Invalidate,
}

/// Tile coordinates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    /// Zoom level
    pub z: u8,
    /// X coordinate
    pub x: u32,
    /// Y coordinate
    pub y: u32,
}

impl TileCoord {
    /// Create new tile coordinates
    pub fn new(z: u8, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Parse from string
    pub fn from_string(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 3 {
            return Err(Error::Protocol(format!("Invalid tile coord format: {}", s)));
        }

        let z = parts[0]
            .parse()
            .map_err(|_| Error::Protocol("Invalid z coordinate".to_string()))?;
        let x = parts[1]
            .parse()
            .map_err(|_| Error::Protocol("Invalid x coordinate".to_string()))?;
        let y = parts[2]
            .parse()
            .map_err(|_| Error::Protocol("Invalid y coordinate".to_string()))?;

        Ok(Self { z, x, y })
    }
}

impl fmt::Display for TileCoord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.z, self.x, self.y)
    }
}

/// Tile update
pub struct TileUpdate {
    /// Tile coordinates
    pub coord: TileCoord,
    /// Update type
    pub update_type: TileUpdateType,
    /// Tile data
    pub data: Vec<u8>,
    /// Tile format (e.g., "png", "webp", "mvt")
    pub format: String,
    /// Optional delta data
    pub delta: Option<Vec<u8>>,
    /// Timestamp
    pub timestamp: i64,
}

impl TileUpdate {
    /// Create a new full tile update
    pub fn full(coord: TileCoord, data: Vec<u8>, format: String) -> Self {
        Self {
            coord,
            update_type: TileUpdateType::Full,
            data,
            format,
            delta: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Create a new delta tile update
    pub fn delta(coord: TileCoord, data: Vec<u8>, delta: Vec<u8>, format: String) -> Self {
        Self {
            coord,
            update_type: TileUpdateType::Delta,
            data,
            format,
            delta: Some(delta),
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Create an invalidation update
    pub fn invalidate(coord: TileCoord) -> Self {
        Self {
            coord,
            update_type: TileUpdateType::Invalidate,
            data: Vec::new(),
            format: String::new(),
            delta: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Convert to message
    pub fn to_message(&self) -> Message {
        let payload = Payload::TileData(TilePayload {
            z: self.coord.z,
            x: self.coord.x,
            y: self.coord.y,
            data: self.data.clone(),
            format: self.format.clone(),
            delta: self.delta.clone(),
        });

        Message::new(MessageType::TileUpdate, payload)
    }
}

/// Tile update manager
pub struct TileUpdateManager {
    /// Pending updates by tile coordinate
    updates: Arc<RwLock<HashMap<TileCoord, VecDeque<TileUpdate>>>>,
    /// Maximum queue size per tile
    max_queue_size: usize,
    /// Statistics
    stats: Arc<TileUpdateStats>,
}

/// Tile update statistics
struct TileUpdateStats {
    total_updates: AtomicU64,
    full_updates: AtomicU64,
    delta_updates: AtomicU64,
    invalidations: AtomicU64,
    dropped_updates: AtomicU64,
}

impl TileUpdateManager {
    /// Create a new tile update manager
    pub fn new(max_queue_size: usize) -> Self {
        Self {
            updates: Arc::new(RwLock::new(HashMap::new())),
            max_queue_size,
            stats: Arc::new(TileUpdateStats {
                total_updates: AtomicU64::new(0),
                full_updates: AtomicU64::new(0),
                delta_updates: AtomicU64::new(0),
                invalidations: AtomicU64::new(0),
                dropped_updates: AtomicU64::new(0),
            }),
        }
    }

    /// Add a tile update
    pub fn add_update(&self, update: TileUpdate) -> Result<()> {
        self.stats.total_updates.fetch_add(1, Ordering::Relaxed);

        match update.update_type {
            TileUpdateType::Full => {
                self.stats.full_updates.fetch_add(1, Ordering::Relaxed);
            }
            TileUpdateType::Delta => {
                self.stats.delta_updates.fetch_add(1, Ordering::Relaxed);
            }
            TileUpdateType::Invalidate => {
                self.stats.invalidations.fetch_add(1, Ordering::Relaxed);
            }
        }

        let mut updates = self.updates.write();
        let queue = updates.entry(update.coord).or_default();

        if queue.len() >= self.max_queue_size {
            // Drop oldest update
            queue.pop_front();
            self.stats.dropped_updates.fetch_add(1, Ordering::Relaxed);
        }

        queue.push_back(update);
        Ok(())
    }

    /// Get pending updates for a tile
    pub fn get_updates(&self, coord: &TileCoord) -> Vec<TileUpdate> {
        let mut updates = self.updates.write();

        if let Some(queue) = updates.get_mut(coord) {
            queue.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    /// Get all pending updates
    pub fn get_all_updates(&self) -> HashMap<TileCoord, Vec<TileUpdate>> {
        let mut updates = self.updates.write();
        let mut result = HashMap::new();

        for (coord, queue) in updates.iter_mut() {
            result.insert(*coord, queue.drain(..).collect());
        }

        result
    }

    /// Clear updates for a tile
    pub fn clear_tile(&self, coord: &TileCoord) {
        let mut updates = self.updates.write();
        updates.remove(coord);
    }

    /// Clear all updates
    pub fn clear_all(&self) {
        let mut updates = self.updates.write();
        updates.clear();
    }

    /// Get pending update count
    pub fn pending_count(&self) -> usize {
        let updates = self.updates.read();
        updates.values().map(|q| q.len()).sum()
    }

    /// Get statistics
    pub async fn stats(&self) -> TileUpdateManagerStats {
        TileUpdateManagerStats {
            total_updates: self.stats.total_updates.load(Ordering::Relaxed),
            full_updates: self.stats.full_updates.load(Ordering::Relaxed),
            delta_updates: self.stats.delta_updates.load(Ordering::Relaxed),
            invalidations: self.stats.invalidations.load(Ordering::Relaxed),
            dropped_updates: self.stats.dropped_updates.load(Ordering::Relaxed),
            pending_updates: self.pending_count(),
        }
    }
}

/// Tile update manager statistics
#[derive(Debug, Clone)]
pub struct TileUpdateManagerStats {
    /// Total updates
    pub total_updates: u64,
    /// Full updates
    pub full_updates: u64,
    /// Delta updates
    pub delta_updates: u64,
    /// Invalidations
    pub invalidations: u64,
    /// Dropped updates
    pub dropped_updates: u64,
    /// Pending updates
    pub pending_updates: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_coord() {
        let coord = TileCoord::new(10, 512, 384);
        assert_eq!(coord.z, 10);
        assert_eq!(coord.x, 512);
        assert_eq!(coord.y, 384);
    }

    #[test]
    fn test_tile_coord_string() -> Result<()> {
        let coord = TileCoord::new(10, 512, 384);
        let s = coord.to_string();
        assert_eq!(s, "10/512/384");

        let parsed = TileCoord::from_string(&s)?;
        assert_eq!(parsed, coord);
        Ok(())
    }

    #[test]
    fn test_tile_update_full() {
        let coord = TileCoord::new(10, 512, 384);
        let data = vec![1, 2, 3, 4];
        let update = TileUpdate::full(coord, data.clone(), "png".to_string());

        assert_eq!(update.coord, coord);
        assert_eq!(update.update_type, TileUpdateType::Full);
        assert_eq!(update.data, data);
        assert_eq!(update.format, "png");
    }

    #[test]
    fn test_tile_update_delta() {
        let coord = TileCoord::new(10, 512, 384);
        let data = vec![1, 2, 3, 4];
        let delta = vec![5, 6, 7, 8];
        let update = TileUpdate::delta(coord, data.clone(), delta.clone(), "png".to_string());

        assert_eq!(update.update_type, TileUpdateType::Delta);
        assert_eq!(update.delta, Some(delta));
    }

    #[test]
    fn test_tile_update_manager() -> Result<()> {
        let manager = TileUpdateManager::new(10);
        let coord = TileCoord::new(10, 512, 384);
        let update = TileUpdate::full(coord, vec![1, 2, 3, 4], "png".to_string());

        manager.add_update(update)?;
        assert_eq!(manager.pending_count(), 1);

        let updates = manager.get_updates(&coord);
        assert_eq!(updates.len(), 1);
        assert_eq!(manager.pending_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_tile_update_stats() -> Result<()> {
        let manager = TileUpdateManager::new(10);
        let coord = TileCoord::new(10, 512, 384);

        let full = TileUpdate::full(coord, vec![1, 2, 3], "png".to_string());
        let delta = TileUpdate::delta(coord, vec![1, 2], vec![3, 4], "png".to_string());
        let inv = TileUpdate::invalidate(coord);

        manager.add_update(full)?;
        manager.add_update(delta)?;
        manager.add_update(inv)?;

        let stats = manager.stats().await;
        assert_eq!(stats.total_updates, 3);
        assert_eq!(stats.full_updates, 1);
        assert_eq!(stats.delta_updates, 1);
        assert_eq!(stats.invalidations, 1);
        Ok(())
    }
}
