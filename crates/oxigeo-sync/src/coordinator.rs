//! Multi-device coordination and state management
//!
//! This module provides coordination capabilities for synchronizing
//! state across multiple devices in a distributed system.

use crate::crdt::Crdt;
use crate::delta::{Delta, DeltaEncoder};
use crate::vector_clock::{ClockOrdering, VectorClock};
use crate::{DeviceId, SyncError, SyncResult, Timestamp};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Device status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus {
    /// Device is online and active
    Online,
    /// Device is offline
    Offline,
    /// Device is syncing
    Syncing,
    /// Device encountered an error
    Error,
}

/// Device metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetadata {
    /// Device identifier
    pub device_id: DeviceId,
    /// Current status
    pub status: DeviceStatus,
    /// Last seen timestamp
    pub last_seen: Timestamp,
    /// Vector clock for this device
    pub clock: VectorClock,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl DeviceMetadata {
    /// Creates new device metadata
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device identifier
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id: device_id.clone(),
            status: DeviceStatus::Offline,
            last_seen: Self::current_timestamp(),
            clock: VectorClock::new(device_id),
            metadata: HashMap::new(),
        }
    }

    /// Gets the current Unix timestamp
    fn current_timestamp() -> Timestamp {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Updates the last seen timestamp
    pub fn update_last_seen(&mut self) {
        self.last_seen = Self::current_timestamp();
    }

    /// Sets device status
    pub fn set_status(&mut self, status: DeviceStatus) {
        self.status = status;
        self.update_last_seen();
    }

    /// Checks if device is online
    pub fn is_online(&self) -> bool {
        matches!(self.status, DeviceStatus::Online | DeviceStatus::Syncing)
    }

    /// Checks if device is stale (hasn't been seen recently)
    ///
    /// # Arguments
    ///
    /// * `timeout_secs` - Timeout in seconds
    pub fn is_stale(&self, timeout_secs: u64) -> bool {
        let current = Self::current_timestamp();
        current.saturating_sub(self.last_seen) > timeout_secs
    }
}

/// Synchronization session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSession {
    /// Session ID
    pub session_id: String,
    /// Source device
    pub source_device: DeviceId,
    /// Target device
    pub target_device: DeviceId,
    /// Start timestamp
    pub started_at: Timestamp,
    /// End timestamp (if completed)
    pub completed_at: Option<Timestamp>,
    /// Number of items synced
    pub items_synced: usize,
    /// Bytes transferred
    pub bytes_transferred: usize,
}

impl SyncSession {
    /// Creates a new sync session
    pub fn new(source_device: DeviceId, target_device: DeviceId) -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            source_device,
            target_device,
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            completed_at: None,
            items_synced: 0,
            bytes_transferred: 0,
        }
    }

    /// Marks the session as completed
    pub fn complete(&mut self) {
        self.completed_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
    }

    /// Gets the duration of the session in seconds
    pub fn duration(&self) -> Option<u64> {
        self.completed_at
            .map(|end| end.saturating_sub(self.started_at))
    }

    /// Checks if the session is completed
    pub fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }
}

/// Sync coordinator for managing multi-device state
pub struct SyncCoordinator {
    /// This device's ID
    device_id: DeviceId,
    /// Device registry
    devices: Arc<DashMap<DeviceId, DeviceMetadata>>,
    /// Active sync sessions
    sessions: Arc<RwLock<Vec<SyncSession>>>,
    /// Delta encoder
    delta_encoder: DeltaEncoder,
}

impl SyncCoordinator {
    /// Creates a new sync coordinator
    ///
    /// # Arguments
    ///
    /// * `device_id` - This device's identifier
    pub fn new(device_id: DeviceId) -> Self {
        let devices = Arc::new(DashMap::new());

        // Register this device
        let mut metadata = DeviceMetadata::new(device_id.clone());
        metadata.set_status(DeviceStatus::Online);
        devices.insert(device_id.clone(), metadata);

        Self {
            device_id,
            devices,
            sessions: Arc::new(RwLock::new(Vec::new())),
            delta_encoder: DeltaEncoder::default_encoder(),
        }
    }

    /// Gets this device's ID
    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    /// Registers a new device
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device to register
    pub fn register_device(&self, device_id: DeviceId) -> SyncResult<()> {
        let metadata = DeviceMetadata::new(device_id.clone());
        self.devices.insert(device_id, metadata);
        Ok(())
    }

    /// Unregisters a device
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device to unregister
    pub fn unregister_device(&self, device_id: &DeviceId) -> SyncResult<()> {
        self.devices.remove(device_id);
        Ok(())
    }

    /// Updates device status
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device to update
    /// * `status` - The new status
    pub fn update_device_status(
        &self,
        device_id: &DeviceId,
        status: DeviceStatus,
    ) -> SyncResult<()> {
        if let Some(mut metadata) = self.devices.get_mut(device_id) {
            metadata.set_status(status);
            Ok(())
        } else {
            Err(SyncError::InvalidDeviceId(device_id.clone()))
        }
    }

    /// Gets device metadata
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device to query
    pub fn get_device(&self, device_id: &DeviceId) -> Option<DeviceMetadata> {
        self.devices
            .get(device_id)
            .map(|entry| entry.value().clone())
    }

    /// Lists all registered devices
    pub fn list_devices(&self) -> Vec<DeviceMetadata> {
        self.devices
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Lists all online devices
    pub fn list_online_devices(&self) -> Vec<DeviceMetadata> {
        self.devices
            .iter()
            .filter(|entry| entry.value().is_online())
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Checks for stale devices and marks them offline
    ///
    /// # Arguments
    ///
    /// * `timeout_secs` - Timeout in seconds
    pub fn cleanup_stale_devices(&self, timeout_secs: u64) -> usize {
        let mut count = 0;

        for mut entry in self.devices.iter_mut() {
            if entry.value().is_stale(timeout_secs) && entry.value().is_online() {
                entry.value_mut().set_status(DeviceStatus::Offline);
                count += 1;
            }
        }

        count
    }

    /// Starts a sync session between two devices
    ///
    /// # Arguments
    ///
    /// * `target_device` - The device to sync with
    pub fn start_sync_session(&self, target_device: DeviceId) -> SyncResult<SyncSession> {
        // Verify target device exists
        if !self.devices.contains_key(&target_device) {
            return Err(SyncError::InvalidDeviceId(target_device));
        }

        let session = SyncSession::new(self.device_id.clone(), target_device.clone());

        // Update device statuses
        self.update_device_status(&self.device_id, DeviceStatus::Syncing)?;
        self.update_device_status(&target_device, DeviceStatus::Syncing)?;

        // Record session
        self.sessions.write().push(session.clone());

        Ok(session)
    }

    /// Completes a sync session
    ///
    /// # Arguments
    ///
    /// * `session_id` - The session to complete
    pub fn complete_sync_session(&self, session_id: &str) -> SyncResult<()> {
        let mut sessions = self.sessions.write();

        if let Some(session) = sessions.iter_mut().find(|s| s.session_id == session_id) {
            session.complete();

            // Update device statuses back to online
            self.update_device_status(&session.source_device, DeviceStatus::Online)?;
            self.update_device_status(&session.target_device, DeviceStatus::Online)?;

            Ok(())
        } else {
            Err(SyncError::CoordinationError(format!(
                "Session not found: {}",
                session_id
            )))
        }
    }

    /// Gets active sync sessions
    pub fn active_sessions(&self) -> Vec<SyncSession> {
        self.sessions
            .read()
            .iter()
            .filter(|s| !s.is_completed())
            .cloned()
            .collect()
    }

    /// Gets completed sync sessions
    pub fn completed_sessions(&self) -> Vec<SyncSession> {
        self.sessions
            .read()
            .iter()
            .filter(|s| s.is_completed())
            .cloned()
            .collect()
    }

    /// Synchronizes CRDT state with another device
    ///
    /// # Arguments
    ///
    /// * `local_crdt` - Local CRDT state
    /// * `remote_crdt` - Remote CRDT state
    pub fn sync_crdt<T: Crdt>(&self, local_crdt: &mut T, remote_crdt: &T) -> SyncResult<()> {
        local_crdt.merge(remote_crdt)
    }

    /// Creates a delta between two data versions
    ///
    /// # Arguments
    ///
    /// * `base` - Base data
    /// * `target` - Target data
    pub fn create_delta(&self, base: &[u8], target: &[u8]) -> SyncResult<Delta> {
        self.delta_encoder.encode(base, target)
    }

    /// Applies a delta to base data
    ///
    /// # Arguments
    ///
    /// * `base` - Base data
    /// * `delta` - Delta to apply
    pub fn apply_delta(&self, base: &[u8], delta: &Delta) -> SyncResult<Vec<u8>> {
        delta.apply(base)
    }

    /// Updates device clock
    ///
    /// # Arguments
    ///
    /// * `device_id` - Device to update
    /// * `clock` - New clock value
    pub fn update_device_clock(&self, device_id: &DeviceId, clock: VectorClock) -> SyncResult<()> {
        if let Some(mut metadata) = self.devices.get_mut(device_id) {
            metadata.clock.merge(&clock);
            metadata.update_last_seen();
            Ok(())
        } else {
            Err(SyncError::InvalidDeviceId(device_id.clone()))
        }
    }

    /// Compares clocks between two devices
    ///
    /// # Arguments
    ///
    /// * `device1` - First device
    /// * `device2` - Second device
    pub fn compare_device_clocks(
        &self,
        device1: &DeviceId,
        device2: &DeviceId,
    ) -> SyncResult<ClockOrdering> {
        let clock1 = self
            .devices
            .get(device1)
            .map(|m| m.clock.clone())
            .ok_or_else(|| SyncError::InvalidDeviceId(device1.clone()))?;

        let clock2 = self
            .devices
            .get(device2)
            .map(|m| m.clock.clone())
            .ok_or_else(|| SyncError::InvalidDeviceId(device2.clone()))?;

        Ok(clock1.compare(&clock2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_metadata_creation() {
        let metadata = DeviceMetadata::new("device-1".to_string());
        assert_eq!(metadata.device_id, "device-1");
        assert_eq!(metadata.status, DeviceStatus::Offline);
    }

    #[test]
    fn test_device_metadata_status() {
        let mut metadata = DeviceMetadata::new("device-1".to_string());
        metadata.set_status(DeviceStatus::Online);
        assert_eq!(metadata.status, DeviceStatus::Online);
        assert!(metadata.is_online());
    }

    #[test]
    fn test_sync_session_creation() {
        let session = SyncSession::new("device-1".to_string(), "device-2".to_string());
        assert_eq!(session.source_device, "device-1");
        assert_eq!(session.target_device, "device-2");
        assert!(!session.is_completed());
    }

    #[test]
    fn test_sync_session_complete() {
        let mut session = SyncSession::new("device-1".to_string(), "device-2".to_string());
        session.complete();
        assert!(session.is_completed());
        assert!(session.duration().is_some());
    }

    #[test]
    fn test_coordinator_creation() {
        let coordinator = SyncCoordinator::new("device-1".to_string());
        assert_eq!(coordinator.device_id(), "device-1");
        assert_eq!(coordinator.list_devices().len(), 1);
    }

    #[test]
    fn test_coordinator_register_device() -> SyncResult<()> {
        let coordinator = SyncCoordinator::new("device-1".to_string());
        coordinator.register_device("device-2".to_string())?;
        assert_eq!(coordinator.list_devices().len(), 2);
        Ok(())
    }

    #[test]
    fn test_coordinator_unregister_device() -> SyncResult<()> {
        let coordinator = SyncCoordinator::new("device-1".to_string());
        coordinator.register_device("device-2".to_string())?;
        coordinator.unregister_device(&"device-2".to_string())?;
        assert_eq!(coordinator.list_devices().len(), 1);
        Ok(())
    }

    #[test]
    fn test_coordinator_update_status() -> SyncResult<()> {
        let coordinator = SyncCoordinator::new("device-1".to_string());
        coordinator.update_device_status(&"device-1".to_string(), DeviceStatus::Syncing)?;

        let metadata = coordinator
            .get_device(&"device-1".to_string())
            .ok_or_else(|| SyncError::InvalidDeviceId("device-1".to_string()))?;
        assert_eq!(metadata.status, DeviceStatus::Syncing);

        Ok(())
    }

    #[test]
    fn test_coordinator_sync_session() -> SyncResult<()> {
        let coordinator = SyncCoordinator::new("device-1".to_string());
        coordinator.register_device("device-2".to_string())?;

        let session = coordinator.start_sync_session("device-2".to_string())?;
        assert_eq!(coordinator.active_sessions().len(), 1);

        coordinator.complete_sync_session(&session.session_id)?;
        assert_eq!(coordinator.active_sessions().len(), 0);
        assert_eq!(coordinator.completed_sessions().len(), 1);

        Ok(())
    }
}
