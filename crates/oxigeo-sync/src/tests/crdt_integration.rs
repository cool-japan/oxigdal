//! Integration tests for CRDT operations

#[cfg(test)]
mod tests {
    use crate::crdt::{Crdt, GCounter, LwwRegister, OrSet, PnCounter};

    #[test]
    fn test_multi_device_g_counter_sync() {
        // Simulate 3 devices incrementing a shared counter
        let mut device1 = GCounter::new("device-1".to_string());
        let mut device2 = GCounter::new("device-2".to_string());
        let mut device3 = GCounter::new("device-3".to_string());

        // Each device increments independently
        for _ in 0..10 {
            device1.increment(1);
        }
        for _ in 0..15 {
            device2.increment(1);
        }
        for _ in 0..20 {
            device3.increment(1);
        }

        // Sync device1 with device2
        device1.merge(&device2).ok();
        assert_eq!(device1.value(), 25); // 10 + 15

        // Sync device1 with device3
        device1.merge(&device3).ok();
        assert_eq!(device1.value(), 45); // 10 + 15 + 20

        // Sync device2 with device3
        device2.merge(&device3).ok();
        assert_eq!(device2.value(), 35); // 15 + 20

        // Final sync device2 with device1
        device2.merge(&device1).ok();
        assert_eq!(device2.value(), 45); // All synchronized
    }

    #[test]
    fn test_pn_counter_concurrent_operations() {
        let mut counter1 = PnCounter::new("device-1".to_string());
        let mut counter2 = PnCounter::new("device-2".to_string());

        // Concurrent increments and decrements
        counter1.increment(50);
        counter1.decrement(10);

        counter2.increment(30);
        counter2.decrement(5);

        // Before merge
        assert_eq!(counter1.value(), 40); // 50 - 10
        assert_eq!(counter2.value(), 25); // 30 - 5

        // Merge
        counter1.merge(&counter2).ok();

        // After merge: (50 + 30) - (10 + 5) = 65
        assert_eq!(counter1.value(), 65);
    }

    #[test]
    fn test_lww_register_conflicts() {
        let mut reg1 = LwwRegister::new("device-1".to_string(), "initial".to_string());
        let mut reg2 = LwwRegister::new("device-2".to_string(), "initial".to_string());

        // Both devices update concurrently
        reg1.set("update-from-device-1".to_string());
        reg2.set("update-from-device-2".to_string());

        // Merge - should use deterministic tie-breaking (device-2 > device-1)
        reg1.merge(&reg2).ok();
        assert_eq!(reg1.get(), "update-from-device-2");
    }

    #[test]
    fn test_or_set_concurrent_add_remove_scenarios() {
        let mut set1 = OrSet::new("device-1".to_string());
        let mut set2 = OrSet::new("device-2".to_string());

        // Both devices add elements
        set1.insert("a".to_string());
        set1.insert("b".to_string());

        set2.insert("b".to_string());
        set2.insert("c".to_string());

        // Device1 removes "b"
        set1.remove(&"b".to_string());

        // Merge - concurrent add from device2 should win
        set1.merge(&set2).ok();

        // Should contain a, b (from device2), c
        assert!(set1.contains(&"a".to_string()));
        assert!(set1.contains(&"b".to_string())); // Concurrent add wins
        assert!(set1.contains(&"c".to_string()));
    }

    #[test]
    fn test_mixed_crdt_workflow() {
        // Simulate a collaborative document editor using multiple CRDTs

        // Use LWW-Register for document title
        let mut title1 = LwwRegister::new("device-1".to_string(), "Untitled".to_string());
        let mut title2 = LwwRegister::new("device-2".to_string(), "Untitled".to_string());

        title1.set("My Document".to_string());
        title2.set("Our Document".to_string());

        title1.merge(&title2).ok();

        // Use OR-Set for tags
        let mut tags1 = OrSet::new("device-1".to_string());
        let mut tags2 = OrSet::new("device-2".to_string());

        tags1.insert("rust".to_string());
        tags1.insert("sync".to_string());

        tags2.insert("crdt".to_string());
        tags2.insert("distributed".to_string());

        tags1.merge(&tags2).ok();

        // Use G-Counter for view count
        let mut views1 = GCounter::new("device-1".to_string());
        let mut views2 = GCounter::new("device-2".to_string());

        for _ in 0..5 {
            views1.increment(1);
        }
        for _ in 0..3 {
            views2.increment(1);
        }

        views1.merge(&views2).ok();

        // Verify final state
        assert_eq!(tags1.len(), 4);
        assert_eq!(views1.value(), 8);
    }

    #[test]
    fn test_crdt_convergence() {
        // Test that all replicas converge to the same state
        let mut counters: Vec<GCounter> = (0..5)
            .map(|i| GCounter::new(format!("device-{}", i)))
            .collect();

        // Each device increments a different amount
        for (i, counter) in counters.iter_mut().enumerate() {
            for _ in 0..(i + 1) * 10 {
                counter.increment(1);
            }
        }

        // Perform all-to-all sync
        let mut final_values = Vec::new();
        for i in 0..5 {
            let mut merged = counters[i].clone();
            for (j, counter) in counters.iter().enumerate().take(5) {
                if i != j {
                    merged.merge(counter).ok();
                }
            }
            final_values.push(merged.value());
        }

        // All devices should converge to the same value
        let expected = 10 + 20 + 30 + 40 + 50; // Sum of all increments
        for value in final_values {
            assert_eq!(value, expected);
        }
    }

    #[test]
    fn test_crdt_idempotence() {
        // Test that merging the same state multiple times is idempotent
        let mut counter1 = GCounter::new("device-1".to_string());
        let mut counter2 = GCounter::new("device-2".to_string());

        counter1.increment(10);
        counter2.increment(20);

        counter1.merge(&counter2).ok();
        let value1 = counter1.value();

        // Merge again - should not change
        counter1.merge(&counter2).ok();
        let value2 = counter1.value();

        assert_eq!(value1, value2);
    }

    #[test]
    fn test_crdt_commutativity() {
        // Test that merge order doesn't matter
        let mut counter1a = GCounter::new("device-1".to_string());
        let mut counter1b = GCounter::new("device-1".to_string());
        let counter2 = {
            let mut c = GCounter::new("device-2".to_string());
            c.increment(10);
            c
        };
        let counter3 = {
            let mut c = GCounter::new("device-3".to_string());
            c.increment(20);
            c
        };

        // Merge in different orders
        counter1a.merge(&counter2).ok();
        counter1a.merge(&counter3).ok();

        counter1b.merge(&counter3).ok();
        counter1b.merge(&counter2).ok();

        assert_eq!(counter1a.value(), counter1b.value());
    }

    #[test]
    fn test_or_set_element_resurrection() {
        // Test that concurrent adds can resurrect removed elements
        let mut set1 = OrSet::new("device-1".to_string());
        let mut set2 = OrSet::new("device-2".to_string());

        // Both add "item"
        set1.insert("item".to_string());
        set2.insert("item".to_string());

        // Device 1 removes it
        set1.remove(&"item".to_string());

        // Device 2 adds it again (concurrent with removal)
        let mut set3 = set2.clone();
        set3.insert("item".to_string());

        // Merge: the new add should keep the item
        set1.merge(&set3).ok();

        // Item should still be present due to concurrent add
        assert!(set1.contains(&"item".to_string()));
    }
}
