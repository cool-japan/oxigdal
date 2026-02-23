//! Resource reservation system for advance booking and guaranteed allocation.

use crate::error::{ClusterError, Result};
use crate::task_graph::ResourceRequirements;
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Reservation identifier.
pub type ReservationId = uuid::Uuid;

/// Resource reservation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    /// Reservation ID
    pub id: ReservationId,
    /// Owner/tenant ID
    pub owner_id: String,
    /// Reserved resources
    pub resources: ResourceRequirements,
    /// Start time
    pub start_time: SystemTime,
    /// End time
    pub end_time: SystemTime,
    /// Reservation status
    pub status: ReservationStatus,
    /// Created at
    pub created_at: SystemTime,
    /// Priority (higher = more important)
    pub priority: i32,
}

/// Reservation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservationStatus {
    /// Pending confirmation
    Pending,
    /// Confirmed and active
    Active,
    /// Currently in use
    InUse,
    /// Completed
    Completed,
    /// Cancelled
    Cancelled,
    /// Expired
    Expired,
}

/// Reservation manager for resource booking.
pub struct ReservationManager {
    /// All reservations
    reservations: Arc<DashMap<ReservationId, Reservation>>,
    /// Time-ordered reservations for efficient lookup
    timeline: Arc<RwLock<BTreeMap<SystemTime, Vec<ReservationId>>>>,
    /// Owner to reservations mapping
    owner_reservations: Arc<DashMap<String, Vec<ReservationId>>>,
    /// Total available resources
    total_resources: Arc<RwLock<ResourceRequirements>>,
    /// Statistics
    stats: Arc<RwLock<ReservationStats>>,
}

/// Reservation statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReservationStats {
    /// Total number of reservations created
    pub total_reservations: u64,
    /// Number of currently active reservations
    pub active_reservations: usize,
    /// Number of completed reservations
    pub completed_reservations: u64,
    /// Number of cancelled reservations
    pub cancelled_reservations: u64,
    /// Number of expired reservations
    pub expired_reservations: u64,
    /// Average resource utilization
    pub average_utilization: f64,
    /// Number of reservation conflicts detected
    pub reservation_conflicts: u64,
}

impl ReservationManager {
    /// Create a new reservation manager.
    pub fn new(total_resources: ResourceRequirements) -> Self {
        Self {
            reservations: Arc::new(DashMap::new()),
            timeline: Arc::new(RwLock::new(BTreeMap::new())),
            owner_reservations: Arc::new(DashMap::new()),
            total_resources: Arc::new(RwLock::new(total_resources)),
            stats: Arc::new(RwLock::new(ReservationStats::default())),
        }
    }

    /// Create a new reservation.
    pub fn create_reservation(
        &self,
        owner_id: String,
        resources: ResourceRequirements,
        start_time: SystemTime,
        duration: Duration,
        priority: i32,
    ) -> Result<ReservationId> {
        let end_time = start_time + duration;
        let now = SystemTime::now();

        // Validate time window
        if start_time < now {
            return Err(ClusterError::InvalidConfiguration(
                "Reservation start time cannot be in the past".to_string(),
            ));
        }

        if duration < Duration::from_secs(60) {
            return Err(ClusterError::InvalidConfiguration(
                "Reservation duration must be at least 1 minute".to_string(),
            ));
        }

        // Check for resource availability
        if !self.check_availability(&resources, start_time, end_time)? {
            let mut stats = self.stats.write();
            stats.reservation_conflicts += 1;
            return Err(ClusterError::ResourceNotAvailable(
                "Insufficient resources for reservation".to_string(),
            ));
        }

        let reservation_id = uuid::Uuid::new_v4();
        let reservation = Reservation {
            id: reservation_id,
            owner_id: owner_id.clone(),
            resources,
            start_time,
            end_time,
            status: ReservationStatus::Pending,
            created_at: now,
            priority,
        };

        // Store reservation
        self.reservations
            .insert(reservation_id, reservation.clone());

        // Update timeline (scoped to drop write lock before stats update)
        {
            let mut timeline = self.timeline.write();
            timeline.entry(start_time).or_default().push(reservation_id);
            timeline.entry(end_time).or_default().push(reservation_id);
        }

        // Update owner mapping
        self.owner_reservations
            .entry(owner_id)
            .or_default()
            .push(reservation_id);

        // Update statistics
        let mut stats = self.stats.write();
        stats.total_reservations += 1;
        stats.active_reservations = self.count_active_reservations();

        Ok(reservation_id)
    }

    /// Check if resources are available during the time window.
    fn check_availability(
        &self,
        required: &ResourceRequirements,
        start: SystemTime,
        end: SystemTime,
    ) -> Result<bool> {
        let total = self.total_resources.read();

        // Get all overlapping reservations
        let overlapping = self.get_overlapping_reservations(start, end);

        // Calculate peak resource usage during the window
        let mut peak_cpu = 0.0;
        let mut peak_memory = 0;
        let mut peak_gpu = 0;
        let mut peak_disk = 0;

        for reservation_id in overlapping {
            if let Some(reservation) = self.reservations.get(&reservation_id) {
                if reservation.status == ReservationStatus::Active
                    || reservation.status == ReservationStatus::InUse
                {
                    peak_cpu += reservation.resources.cpu_cores;
                    peak_memory += reservation.resources.memory_bytes;
                    peak_gpu += if reservation.resources.gpu { 1 } else { 0 };
                    peak_disk += reservation.resources.storage_bytes;
                }
            }
        }

        // Check if adding this reservation would exceed capacity
        Ok(peak_cpu + required.cpu_cores <= total.cpu_cores
            && peak_memory + required.memory_bytes <= total.memory_bytes
            && (if required.gpu { peak_gpu + 1 } else { peak_gpu })
                <= (if total.gpu { 1 } else { 0 })
            && peak_disk + required.storage_bytes <= total.storage_bytes)
    }

    /// Get reservations that overlap with the given time window.
    fn get_overlapping_reservations(
        &self,
        start: SystemTime,
        end: SystemTime,
    ) -> Vec<ReservationId> {
        let mut overlapping = Vec::new();

        for entry in self.reservations.iter() {
            let reservation = entry.value();

            // Check for overlap: (start1 < end2) && (start2 < end1)
            if reservation.start_time < end && start < reservation.end_time {
                overlapping.push(reservation.id);
            }
        }

        overlapping
    }

    /// Activate a pending reservation.
    pub fn activate_reservation(&self, id: ReservationId) -> Result<()> {
        // Scope the DashMap mutable ref so it's dropped before we iterate
        // the same DashMap in count_active_reservations(). Holding get_mut
        // while calling iter() on the same DashMap causes a deadlock.
        {
            let mut reservation = self
                .reservations
                .get_mut(&id)
                .ok_or_else(|| ClusterError::ReservationNotFound(id.to_string()))?;

            if reservation.status != ReservationStatus::Pending {
                return Err(ClusterError::InvalidOperation(format!(
                    "Cannot activate reservation in status {:?}",
                    reservation.status
                )));
            }

            reservation.status = ReservationStatus::Active;
        }

        let mut stats = self.stats.write();
        stats.active_reservations = self.count_active_reservations();

        Ok(())
    }

    /// Mark reservation as in use.
    pub fn use_reservation(&self, id: ReservationId) -> Result<()> {
        let mut reservation = self
            .reservations
            .get_mut(&id)
            .ok_or_else(|| ClusterError::ReservationNotFound(id.to_string()))?;

        if reservation.status != ReservationStatus::Active {
            return Err(ClusterError::InvalidOperation(format!(
                "Cannot use reservation in status {:?}",
                reservation.status
            )));
        }

        let now = SystemTime::now();
        if now < reservation.start_time || now > reservation.end_time {
            return Err(ClusterError::InvalidOperation(
                "Reservation is not in valid time window".to_string(),
            ));
        }

        reservation.status = ReservationStatus::InUse;

        Ok(())
    }

    /// Complete a reservation.
    pub fn complete_reservation(&self, id: ReservationId) -> Result<()> {
        // Scope the DashMap mutable ref so it's dropped before we iterate
        // the same DashMap in count_active_reservations().
        {
            let mut reservation = self
                .reservations
                .get_mut(&id)
                .ok_or_else(|| ClusterError::ReservationNotFound(id.to_string()))?;

            reservation.status = ReservationStatus::Completed;
        }

        let mut stats = self.stats.write();
        stats.completed_reservations += 1;
        stats.active_reservations = self.count_active_reservations();

        Ok(())
    }

    /// Cancel a reservation.
    pub fn cancel_reservation(&self, id: ReservationId) -> Result<()> {
        // Scope the DashMap mutable ref so it's dropped before we iterate
        // the same DashMap in count_active_reservations().
        {
            let mut reservation = self
                .reservations
                .get_mut(&id)
                .ok_or_else(|| ClusterError::ReservationNotFound(id.to_string()))?;

            if reservation.status == ReservationStatus::Completed {
                return Err(ClusterError::InvalidOperation(
                    "Cannot cancel completed reservation".to_string(),
                ));
            }

            reservation.status = ReservationStatus::Cancelled;
        }

        let mut stats = self.stats.write();
        stats.cancelled_reservations += 1;
        stats.active_reservations = self.count_active_reservations();

        Ok(())
    }

    /// Expire old reservations.
    pub fn expire_old_reservations(&self) -> Result<Vec<ReservationId>> {
        let now = SystemTime::now();
        let mut expired = Vec::new();

        for mut entry in self.reservations.iter_mut() {
            let reservation = entry.value_mut();

            if reservation.end_time < now
                && (reservation.status == ReservationStatus::Active
                    || reservation.status == ReservationStatus::Pending)
            {
                reservation.status = ReservationStatus::Expired;
                expired.push(reservation.id);
            }
        }

        if !expired.is_empty() {
            let mut stats = self.stats.write();
            stats.expired_reservations += expired.len() as u64;
            stats.active_reservations = self.count_active_reservations();
        }

        Ok(expired)
    }

    /// Get reservation details.
    pub fn get_reservation(&self, id: ReservationId) -> Option<Reservation> {
        self.reservations.get(&id).map(|r| r.clone())
    }

    /// List all reservations for an owner.
    pub fn list_owner_reservations(&self, owner_id: &str) -> Vec<Reservation> {
        self.owner_reservations
            .get(owner_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.get_reservation(*id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all active reservations.
    pub fn list_active_reservations(&self) -> Vec<Reservation> {
        self.reservations
            .iter()
            .filter(|entry| {
                matches!(
                    entry.value().status,
                    ReservationStatus::Active | ReservationStatus::InUse
                )
            })
            .map(|entry| entry.value().clone())
            .collect()
    }

    fn count_active_reservations(&self) -> usize {
        self.reservations
            .iter()
            .filter(|entry| {
                matches!(
                    entry.value().status,
                    ReservationStatus::Active | ReservationStatus::InUse
                )
            })
            .count()
    }

    /// Get reservation statistics.
    pub fn get_stats(&self) -> ReservationStats {
        self.stats.read().clone()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_create_reservation() {
        let total_resources = ResourceRequirements {
            cpu_cores: 16.0,
            memory_bytes: 32768 * 1024 * 1024,
            gpu: true,
            storage_bytes: 102400 * 1024 * 1024,
        };

        let manager = ReservationManager::new(total_resources);

        let start = SystemTime::now() + Duration::from_secs(60);
        let duration = Duration::from_secs(3600);

        let resources = ResourceRequirements {
            cpu_cores: 4.0,
            memory_bytes: 8192 * 1024 * 1024,
            gpu: true,
            storage_bytes: 10240 * 1024 * 1024,
        };

        let result =
            manager.create_reservation("user1".to_string(), resources, start, duration, 10);

        assert!(result.is_ok());
        let id = result.expect("reservation should be created");

        let reservation = manager.get_reservation(id);
        assert!(reservation.is_some());
        assert_eq!(
            reservation.expect("reservation should exist").status,
            ReservationStatus::Pending
        );
    }

    #[test]
    fn test_reservation_lifecycle() {
        let total_resources = ResourceRequirements {
            cpu_cores: 16.0,
            memory_bytes: 32768 * 1024 * 1024,
            gpu: true,
            storage_bytes: 102400 * 1024 * 1024,
        };

        let manager = ReservationManager::new(total_resources);

        // Use a future start time to avoid "start time in the past" validation
        let start = SystemTime::now() + Duration::from_secs(3600);
        let duration = Duration::from_secs(7200);

        let resources = ResourceRequirements {
            cpu_cores: 4.0,
            memory_bytes: 8192 * 1024 * 1024,
            gpu: true,
            storage_bytes: 10240 * 1024 * 1024,
        };

        let id = manager
            .create_reservation("user1".to_string(), resources, start, duration, 10)
            .expect("reservation should be created");

        // Activate
        manager
            .activate_reservation(id)
            .expect("activation should succeed");
        let reservation = manager
            .get_reservation(id)
            .expect("reservation should exist");
        assert_eq!(reservation.status, ReservationStatus::Active);

        // use_reservation requires now to be within [start_time, end_time],
        // which is in the future, so we skip that transition and test the
        // remaining lifecycle: Active -> Completed
        // Complete
        manager
            .complete_reservation(id)
            .expect("completion should succeed");
        let reservation = manager
            .get_reservation(id)
            .expect("reservation should exist");
        assert_eq!(reservation.status, ReservationStatus::Completed);
    }

    #[test]
    fn test_reservation_conflict() {
        let total_resources = ResourceRequirements {
            cpu_cores: 8.0,
            memory_bytes: 16384 * 1024 * 1024,
            gpu: true,
            storage_bytes: 51200 * 1024 * 1024,
        };

        let manager = ReservationManager::new(total_resources);

        let start = SystemTime::now() + Duration::from_secs(60);
        let duration = Duration::from_secs(3600);

        let resources = ResourceRequirements {
            cpu_cores: 6.0,
            memory_bytes: 12288 * 1024 * 1024,
            gpu: true,
            storage_bytes: 30720 * 1024 * 1024,
        };

        // First reservation should succeed
        let result1 =
            manager.create_reservation("user1".to_string(), resources.clone(), start, duration, 10);
        assert!(result1.is_ok());

        let id = result1.expect("first reservation should succeed");
        let _ = manager.activate_reservation(id);

        // Second overlapping reservation should fail
        let result2 =
            manager.create_reservation("user2".to_string(), resources, start, duration, 10);
        assert!(result2.is_err());
    }
}
