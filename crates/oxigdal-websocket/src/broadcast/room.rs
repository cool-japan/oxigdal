//! Room management for group communications

use crate::error::{Error, Result};
use crate::protocol::message::Message;
use crate::server::connection::ConnectionId;
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

/// Room for group communications
pub struct Room {
    name: String,
    members: Arc<RwLock<HashSet<ConnectionId>>>,
    max_members: usize,
    stats: Arc<RoomStatistics>,
}

/// Room statistics
struct RoomStatistics {
    messages_sent: AtomicU64,
    total_joins: AtomicU64,
    total_leaves: AtomicU64,
}

impl Room {
    /// Create a new room
    pub fn new(name: String, max_members: usize) -> Self {
        Self {
            name,
            members: Arc::new(RwLock::new(HashSet::new())),
            max_members,
            stats: Arc::new(RoomStatistics {
                messages_sent: AtomicU64::new(0),
                total_joins: AtomicU64::new(0),
                total_leaves: AtomicU64::new(0),
            }),
        }
    }

    /// Get room name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Add a member to the room
    pub async fn join(&self, member: ConnectionId) -> Result<()> {
        let mut members = self.members.write().await;

        if members.len() >= self.max_members {
            return Err(Error::Room(format!(
                "Room {} is full (max {} members)",
                self.name, self.max_members
            )));
        }

        if members.insert(member) {
            self.stats.total_joins.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("Member {} joined room {}", member, self.name);
        }

        Ok(())
    }

    /// Remove a member from the room
    pub async fn leave(&self, member: &ConnectionId) -> Result<()> {
        let mut members = self.members.write().await;

        if members.remove(member) {
            self.stats.total_leaves.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("Member {} left room {}", member, self.name);
        }

        Ok(())
    }

    /// Check if a member is in the room
    pub async fn has_member(&self, member: &ConnectionId) -> bool {
        let members = self.members.read().await;
        members.contains(member)
    }

    /// Get all members
    pub async fn members(&self) -> Vec<ConnectionId> {
        let members = self.members.read().await;
        members.iter().copied().collect()
    }

    /// Get member count
    pub async fn member_count(&self) -> usize {
        self.members.read().await.len()
    }

    /// Record a message sent to the room
    pub fn record_message(&self) {
        self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Get room statistics
    pub async fn stats(&self) -> RoomStats {
        RoomStats {
            name: self.name.clone(),
            member_count: self.member_count().await,
            messages_sent: self.stats.messages_sent.load(Ordering::Relaxed),
            total_joins: self.stats.total_joins.load(Ordering::Relaxed),
            total_leaves: self.stats.total_leaves.load(Ordering::Relaxed),
        }
    }
}

/// Room statistics snapshot
#[derive(Debug, Clone)]
pub struct RoomStats {
    /// Room name
    pub name: String,
    /// Current member count
    pub member_count: usize,
    /// Messages sent to the room
    pub messages_sent: u64,
    /// Total joins
    pub total_joins: u64,
    /// Total leaves
    pub total_leaves: u64,
}

/// Room manager
pub struct RoomManager {
    rooms: Arc<DashMap<String, Arc<Room>>>,
    max_rooms: usize,
    max_members_per_room: usize,
}

impl RoomManager {
    /// Create a new room manager
    pub fn new(max_rooms: usize, max_members_per_room: usize) -> Self {
        Self {
            rooms: Arc::new(DashMap::new()),
            max_rooms,
            max_members_per_room,
        }
    }

    /// Create a room
    pub fn create_room(&self, name: String) -> Result<Arc<Room>> {
        if self.rooms.len() >= self.max_rooms {
            return Err(Error::Room(format!(
                "Maximum number of rooms ({}) reached",
                self.max_rooms
            )));
        }

        let room = Arc::new(Room::new(name.clone(), self.max_members_per_room));
        self.rooms.insert(name, room.clone());

        tracing::info!("Room {} created", room.name());
        Ok(room)
    }

    /// Get or create a room
    pub fn get_or_create(&self, name: &str) -> Result<Arc<Room>> {
        if let Some(room) = self.rooms.get(name) {
            Ok(room.clone())
        } else {
            self.create_room(name.to_string())
        }
    }

    /// Get a room
    pub fn get(&self, name: &str) -> Option<Arc<Room>> {
        self.rooms.get(name).map(|r| r.clone())
    }

    /// Delete a room
    pub fn delete_room(&self, name: &str) -> Option<Arc<Room>> {
        self.rooms.remove(name).map(|(_, room)| {
            tracing::info!("Room {} deleted", name);
            room
        })
    }

    /// Join a room
    pub async fn join(&self, room_name: &str, member: ConnectionId) -> Result<()> {
        let room = self.get_or_create(room_name)?;
        room.join(member).await
    }

    /// Leave a room
    pub async fn leave(&self, room_name: &str, member: &ConnectionId) -> Result<()> {
        if let Some(room) = self.get(room_name) {
            room.leave(member).await?;

            // Delete room if empty
            if room.member_count().await == 0 {
                self.delete_room(room_name);
            }
        }
        Ok(())
    }

    /// Broadcast a message to a room
    pub async fn broadcast(&self, room_name: &str, _message: Message) -> Result<Vec<ConnectionId>> {
        if let Some(room) = self.get(room_name) {
            room.record_message();
            Ok(room.members().await)
        } else {
            Ok(Vec::new())
        }
    }

    /// Get all rooms
    pub fn rooms(&self) -> Vec<String> {
        self.rooms.iter().map(|r| r.key().clone()).collect()
    }

    /// Get room count
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    /// Get member count across all rooms
    pub async fn total_member_count(&self) -> usize {
        let mut total = 0;
        for room in self.rooms.iter() {
            total += room.member_count().await;
        }
        total
    }

    /// Get statistics
    pub async fn stats(&self) -> RoomManagerStats {
        let mut total_members = 0;
        let mut total_messages = 0;

        for room in self.rooms.iter() {
            let stats = room.stats().await;
            total_members += stats.member_count;
            total_messages += stats.messages_sent;
        }

        RoomManagerStats {
            total_rooms: self.room_count(),
            total_members,
            total_messages,
        }
    }

    /// Remove member from all rooms
    pub async fn remove_member_from_all(&self, member: &ConnectionId) -> Result<()> {
        let room_names = self.rooms().to_vec();

        for room_name in room_names {
            self.leave(&room_name, member).await?;
        }

        Ok(())
    }
}

/// Room manager statistics
#[derive(Debug, Clone)]
pub struct RoomManagerStats {
    /// Total number of rooms
    pub total_rooms: usize,
    /// Total members across all rooms
    pub total_members: usize,
    /// Total messages sent to all rooms
    pub total_messages: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_room_creation() {
        let room = Room::new("test".to_string(), 100);
        assert_eq!(room.name(), "test");
        assert_eq!(room.member_count().await, 0);
    }

    #[tokio::test]
    async fn test_room_join() -> Result<()> {
        let room = Room::new("test".to_string(), 100);
        let member = Uuid::new_v4();

        room.join(member).await?;
        assert_eq!(room.member_count().await, 1);
        assert!(room.has_member(&member).await);
        Ok(())
    }

    #[tokio::test]
    async fn test_room_leave() -> Result<()> {
        let room = Room::new("test".to_string(), 100);
        let member = Uuid::new_v4();

        room.join(member).await?;
        room.leave(&member).await?;

        assert_eq!(room.member_count().await, 0);
        assert!(!room.has_member(&member).await);
        Ok(())
    }

    #[tokio::test]
    async fn test_room_max_members() {
        let room = Room::new("test".to_string(), 2);
        let member1 = Uuid::new_v4();
        let member2 = Uuid::new_v4();
        let member3 = Uuid::new_v4();

        assert!(room.join(member1).await.is_ok());
        assert!(room.join(member2).await.is_ok());
        assert!(room.join(member3).await.is_err());
    }

    #[tokio::test]
    async fn test_room_manager() -> Result<()> {
        let manager = RoomManager::new(10, 100);
        assert_eq!(manager.room_count(), 0);

        let room = manager.create_room("test".to_string())?;
        assert_eq!(manager.room_count(), 1);
        assert_eq!(room.name(), "test");
        Ok(())
    }

    #[tokio::test]
    async fn test_room_manager_join_leave() -> Result<()> {
        let manager = RoomManager::new(10, 100);
        let member = Uuid::new_v4();

        manager.join("test", member).await?;
        assert_eq!(manager.room_count(), 1);

        manager.leave("test", &member).await?;
        // Room should be deleted when empty
        assert_eq!(manager.room_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_room_manager_broadcast() -> Result<()> {
        let manager = RoomManager::new(10, 100);
        let member1 = Uuid::new_v4();
        let member2 = Uuid::new_v4();

        manager.join("test", member1).await?;
        manager.join("test", member2).await?;

        let message = Message::ping();
        let recipients = manager.broadcast("test", message).await?;

        assert_eq!(recipients.len(), 2);
        Ok(())
    }
}
