//! Real-world synchronization scenarios

#[cfg(test)]
mod tests {
    use crate::SyncResult;
    use crate::coordinator::{DeviceStatus, SyncCoordinator};
    use crate::crdt::{Crdt, GCounter, OrSet};

    use crate::merkle::MerkleTree;
    use crate::vector_clock::VectorClock;

    #[test]
    fn test_basic_two_device_sync() -> SyncResult<()> {
        let coord1 = SyncCoordinator::new("device-1".to_string());
        let coord2_id = "device-2".to_string();

        // Register second device
        coord1.register_device(coord2_id.clone())?;

        // Start sync session
        let session = coord1.start_sync_session(coord2_id.clone())?;

        assert!(!session.is_completed());

        // Complete sync
        coord1.complete_sync_session(&session.session_id)?;

        // Verify devices are back online
        let device1 = coord1
            .get_device(&"device-1".to_string())
            .ok_or_else(|| crate::SyncError::InvalidDeviceId("device-1".to_string()))?;
        assert_eq!(device1.status, DeviceStatus::Online);

        Ok(())
    }

    #[test]
    fn test_multi_device_coordination() -> SyncResult<()> {
        let coordinator = SyncCoordinator::new("central".to_string());

        // Register multiple devices
        for i in 1..=5 {
            coordinator.register_device(format!("device-{}", i))?;
        }

        assert_eq!(coordinator.list_devices().len(), 6); // Central + 5 devices

        // Update statuses
        for i in 1..=5 {
            coordinator.update_device_status(&format!("device-{}", i), DeviceStatus::Online)?;
        }

        let online = coordinator.list_online_devices();
        assert_eq!(online.len(), 6);

        Ok(())
    }

    #[test]
    fn test_sync_with_delta_encoding() -> SyncResult<()> {
        let coordinator = SyncCoordinator::new("device-1".to_string());

        // Create two versions of data
        let base_data = b"Hello, world! This is a test document with some content.";
        let target_data = b"Hello, world! This is a modified test document with updated content.";

        // Create delta
        let delta = coordinator.create_delta(base_data, target_data)?;

        // Apply delta
        let result = coordinator.apply_delta(base_data, &delta)?;

        assert_eq!(result, target_data);

        Ok(())
    }

    #[test]
    fn test_merkle_tree_sync_protocol() -> SyncResult<()> {
        // Simulate sync using Merkle trees for efficient change detection

        // Device 1 data blocks
        let device1_blocks: Vec<Vec<u8>> = vec![
            b"block-1".to_vec(),
            b"block-2".to_vec(),
            b"block-3".to_vec(),
            b"block-4".to_vec(),
        ];

        // Device 2 has modified one block
        let mut device2_blocks = device1_blocks.clone();
        device2_blocks[2] = b"block-3-modified".to_vec();

        // Build Merkle trees
        let tree1 = MerkleTree::from_data(device1_blocks.clone())?;
        let tree2 = MerkleTree::from_data(device2_blocks)?;

        // Compare trees to find differences
        let differences = tree1.diff(&tree2);

        // Should detect at least one difference
        assert!(!differences.is_empty());

        Ok(())
    }

    #[test]
    fn test_concurrent_edit_scenario() -> SyncResult<()> {
        // Simulate concurrent edits on a shared document

        // Initial state
        let mut doc1 = OrSet::new("device-1".to_string());
        let mut doc2 = OrSet::new("device-2".to_string());

        // Both devices start with the same paragraphs
        doc1.insert("para-1".to_string());
        doc1.insert("para-2".to_string());
        doc2.insert("para-1".to_string());
        doc2.insert("para-2".to_string());

        // Device 1 adds and removes
        doc1.insert("para-3".to_string());
        doc1.remove(&"para-2".to_string());

        // Device 2 makes different changes
        doc2.insert("para-4".to_string());
        doc2.remove(&"para-1".to_string());

        // Sync the documents
        doc1.merge(&doc2)?;

        // Should contain elements from both edits (except removed ones)
        assert!(doc1.contains(&"para-3".to_string()));
        assert!(doc1.contains(&"para-4".to_string()));

        Ok(())
    }

    #[test]
    fn test_offline_online_sync() -> SyncResult<()> {
        let coordinator = SyncCoordinator::new("server".to_string());

        // Register devices
        coordinator.register_device("mobile".to_string())?;
        coordinator.register_device("desktop".to_string())?;

        // Mobile goes online
        coordinator.update_device_status(&"mobile".to_string(), DeviceStatus::Online)?;

        // Desktop is offline
        coordinator.update_device_status(&"desktop".to_string(), DeviceStatus::Offline)?;

        // Check online devices
        let online = coordinator.list_online_devices();
        assert_eq!(online.len(), 2); // Server + mobile

        // Desktop comes online
        coordinator.update_device_status(&"desktop".to_string(), DeviceStatus::Online)?;

        let online = coordinator.list_online_devices();
        assert_eq!(online.len(), 3); // All devices

        Ok(())
    }

    #[test]
    fn test_vector_clock_sync_ordering() {
        let mut clock1 = VectorClock::new("device-1".to_string());
        let mut clock2 = VectorClock::new("device-2".to_string());

        // Simulate event sequence
        clock1.tick(); // Event 1 on device-1
        clock2.merge(&clock1);
        clock2.tick(); // Event 2 on device-2
        clock1.merge(&clock2);
        clock1.tick(); // Event 3 on device-1

        // clock1 should have happened after clock2's last event
        assert!(clock1.happened_after(&clock2));
    }

    #[test]
    fn test_conflict_free_counter_updates() -> SyncResult<()> {
        // Simulate distributed counter across multiple devices
        let devices = ["dev1", "dev2", "dev3", "dev4", "dev5"];
        let mut counters: Vec<GCounter> = devices
            .iter()
            .map(|d| GCounter::new(d.to_string()))
            .collect();

        // Each device increments independently
        for (i, counter) in counters.iter_mut().enumerate() {
            for _ in 0..(i + 1) * 5 {
                counter.increment(1);
            }
        }

        // Sync all devices with first device
        for i in 1..counters.len() {
            let counter_i = counters[i].clone();
            counters[0].merge(&counter_i)?;
        }

        // First device should have all counts
        let expected = 5 + 10 + 15 + 20 + 25; // Sum of all increments
        assert_eq!(counters[0].value(), expected);

        Ok(())
    }

    #[test]
    fn test_session_tracking() -> SyncResult<()> {
        let coordinator = SyncCoordinator::new("device-1".to_string());
        coordinator.register_device("device-2".to_string())?;

        // Start multiple sessions
        let session1 = coordinator.start_sync_session("device-2".to_string())?;
        assert_eq!(coordinator.active_sessions().len(), 1);

        // Complete first session
        coordinator.complete_sync_session(&session1.session_id)?;
        assert_eq!(coordinator.active_sessions().len(), 0);
        assert_eq!(coordinator.completed_sessions().len(), 1);

        Ok(())
    }

    #[test]
    fn test_large_scale_sync() -> SyncResult<()> {
        // Test synchronization with many devices
        let coordinator = SyncCoordinator::new("master".to_string());

        // Register 100 devices
        for i in 0..100 {
            coordinator.register_device(format!("device-{}", i))?;
            coordinator.update_device_status(&format!("device-{}", i), DeviceStatus::Online)?;
        }

        assert_eq!(coordinator.list_online_devices().len(), 101); // Master + 100 devices

        Ok(())
    }
}
