//! Property-based tests for synchronization primitives

#[cfg(test)]
mod tests {
    use crate::crdt::{Crdt, GCounter, PnCounter};
    use crate::vector_clock::VectorClock;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_g_counter_always_increases(increments in prop::collection::vec(1u64..100, 0..50)) {
            let mut counter = GCounter::new("device-1".to_string());
            let mut current_value = counter.value();

            for inc in increments {
                counter.increment(inc);
                let new_value = counter.value();
                prop_assert!(new_value >= current_value);
                current_value = new_value;
            }
        }

        #[test]
        fn test_g_counter_merge_commutative(
            inc1 in prop::collection::vec(1u64..100, 0..20),
            inc2 in prop::collection::vec(1u64..100, 0..20)
        ) {
            let mut counter1a = GCounter::new("device-1".to_string());
            let mut counter2a = GCounter::new("device-2".to_string());

            let mut counter1b = GCounter::new("device-1".to_string());
            let mut counter2b = GCounter::new("device-2".to_string());

            for &inc in &inc1 {
                counter1a.increment(inc);
                counter1b.increment(inc);
            }

            for &inc in &inc2 {
                counter2a.increment(inc);
                counter2b.increment(inc);
            }

            // Merge in different orders
            counter1a.merge(&counter2a).ok();
            counter2b.merge(&counter1b).ok();

            prop_assert_eq!(counter1a.value(), counter2b.value());
        }

        #[test]
        fn test_g_counter_merge_idempotent(increments in prop::collection::vec(1u64..100, 0..30)) {
            let mut counter1 = GCounter::new("device-1".to_string());
            let mut counter2 = GCounter::new("device-2".to_string());

            for &inc in &increments {
                counter2.increment(inc);
            }

            counter1.merge(&counter2).ok();
            let value1 = counter1.value();

            counter1.merge(&counter2).ok();
            let value2 = counter1.value();

            prop_assert_eq!(value1, value2);
        }

        #[test]
        fn test_pn_counter_operations(
            increments in prop::collection::vec(1u64..100, 0..30),
            decrements in prop::collection::vec(1u64..100, 0..30)
        ) {
            let mut counter = PnCounter::new("device-1".to_string());

            let total_inc: u64 = increments.iter().sum();
            let total_dec: u64 = decrements.iter().sum();

            for &inc in &increments {
                counter.increment(inc);
            }

            for &dec in &decrements {
                counter.decrement(dec);
            }

            let expected = (total_inc as i64) - (total_dec as i64);
            prop_assert_eq!(counter.value(), expected);
        }

        #[test]
        fn test_vector_clock_merge_monotonic(ticks in prop::collection::vec(1usize..10, 0..20)) {
            let mut clock1 = VectorClock::new("device-1".to_string());
            let mut clock2 = VectorClock::new("device-2".to_string());

            for _ in 0..ticks.len() {
                clock2.tick();
            }

            let initial_sum = clock1.sum();
            clock1.merge(&clock2);
            let final_sum = clock1.sum();

            prop_assert!(final_sum >= initial_sum);
        }

        #[test]
        fn test_vector_clock_compare_reflexive(ticks in 1usize..50) {
            let mut clock = VectorClock::new("device-1".to_string());

            for _ in 0..ticks {
                clock.tick();
            }

            use crate::vector_clock::ClockOrdering;
            prop_assert_eq!(clock.compare(&clock), ClockOrdering::Equal);
        }

        #[test]
        fn test_pn_counter_merge_associative(
            inc1 in prop::collection::vec(1u64..50, 0..10),
            inc2 in prop::collection::vec(1u64..50, 0..10),
            inc3 in prop::collection::vec(1u64..50, 0..10)
        ) {
            let mut counter1a = PnCounter::new("device-1".to_string());
            let mut counter2a = PnCounter::new("device-2".to_string());
            let counter3a = {
                let mut c = PnCounter::new("device-3".to_string());
                for &inc in &inc3 {
                    c.increment(inc);
                }
                c
            };

            let mut counter1b = PnCounter::new("device-1".to_string());
            let counter2b = {
                let mut c = PnCounter::new("device-2".to_string());
                for &inc in &inc2 {
                    c.increment(inc);
                }
                c
            };
            let mut counter3b = PnCounter::new("device-3".to_string());

            for &inc in &inc1 {
                counter1a.increment(inc);
                counter1b.increment(inc);
            }

            for &inc in &inc2 {
                counter2a.increment(inc);
            }

            for &inc in &inc3 {
                counter3b.increment(inc);
            }

            // (c1 merge c2) merge c3
            counter1a.merge(&counter2a).ok();
            counter1a.merge(&counter3a).ok();

            // c1 merge (c2 merge c3)
            counter3b.merge(&counter2b).ok();
            counter1b.merge(&counter3b).ok();

            prop_assert_eq!(counter1a.value(), counter1b.value());
        }
    }

    #[test]
    fn test_g_counter_zero_increments() {
        let mut counter = GCounter::new("device-1".to_string());
        assert_eq!(counter.value(), 0);

        counter.increment(0);
        assert_eq!(counter.value(), 0);
    }

    #[test]
    fn test_pn_counter_zero_operations() {
        let mut counter = PnCounter::new("device-1".to_string());
        assert_eq!(counter.value(), 0);

        counter.increment(0);
        counter.decrement(0);
        assert_eq!(counter.value(), 0);
    }

    #[test]
    fn test_vector_clock_empty() {
        let clock = VectorClock::new("device-1".to_string());
        assert!(!clock.is_empty());
        assert_eq!(clock.len(), 1);
    }

    #[test]
    fn test_crdt_convergence_property() {
        // Property: After all-to-all sync, all replicas should have the same value
        let mut counters = vec![
            GCounter::new("device-1".to_string()),
            GCounter::new("device-2".to_string()),
            GCounter::new("device-3".to_string()),
        ];

        counters[0].increment(5);
        counters[1].increment(10);
        counters[2].increment(15);

        // All-to-all sync
        for i in 0..counters.len() {
            for j in 0..counters.len() {
                if i != j {
                    let counter_j = counters[j].clone();
                    counters[i].merge(&counter_j).ok();
                }
            }
        }

        // All should have the same value
        let expected = 30;
        for counter in &counters {
            assert_eq!(counter.value(), expected);
        }
    }
}
