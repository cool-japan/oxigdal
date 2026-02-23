//! Rebalance listener for handling partition assignments

use rdkafka::TopicPartitionList;
use std::sync::Arc;
use tracing::{info, warn};

/// Type alias for rebalance callback function
pub type RebalanceCallbackFn = Arc<dyn Fn(&TopicPartitionList) + Send + Sync>;

/// Callback for rebalance events
pub trait RebalanceCallback: Send + Sync {
    /// Called when partitions are assigned
    fn on_partitions_assigned(&self, partitions: &TopicPartitionList);

    /// Called when partitions are revoked
    fn on_partitions_revoked(&self, partitions: &TopicPartitionList);
}

/// Default rebalance listener that logs events
#[derive(Debug, Default)]
pub struct DefaultRebalanceListener;

impl RebalanceCallback for DefaultRebalanceListener {
    fn on_partitions_assigned(&self, partitions: &TopicPartitionList) {
        info!("Partitions assigned:");
        for elem in partitions.elements() {
            info!(
                "  - Topic: {}, Partition: {}, Offset: {:?}",
                elem.topic(),
                elem.partition(),
                elem.offset()
            );
        }
    }

    fn on_partitions_revoked(&self, partitions: &TopicPartitionList) {
        warn!("Partitions revoked:");
        for elem in partitions.elements() {
            warn!(
                "  - Topic: {}, Partition: {}",
                elem.topic(),
                elem.partition()
            );
        }
    }
}

/// Rebalance listener with custom callbacks
pub struct CustomRebalanceListener<F1, F2>
where
    F1: Fn(&TopicPartitionList) + Send + Sync,
    F2: Fn(&TopicPartitionList) + Send + Sync,
{
    on_assign: F1,
    on_revoke: F2,
}

impl<F1, F2> CustomRebalanceListener<F1, F2>
where
    F1: Fn(&TopicPartitionList) + Send + Sync,
    F2: Fn(&TopicPartitionList) + Send + Sync,
{
    /// Create a new custom rebalance listener
    pub fn new(on_assign: F1, on_revoke: F2) -> Self {
        Self {
            on_assign,
            on_revoke,
        }
    }
}

impl<F1, F2> RebalanceCallback for CustomRebalanceListener<F1, F2>
where
    F1: Fn(&TopicPartitionList) + Send + Sync,
    F2: Fn(&TopicPartitionList) + Send + Sync,
{
    fn on_partitions_assigned(&self, partitions: &TopicPartitionList) {
        (self.on_assign)(partitions);
    }

    fn on_partitions_revoked(&self, partitions: &TopicPartitionList) {
        (self.on_revoke)(partitions);
    }
}

/// Rebalance listener that saves/restores offsets
pub struct OffsetSavingRebalanceListener {
    on_assign: RebalanceCallbackFn,
    on_revoke: RebalanceCallbackFn,
}

impl OffsetSavingRebalanceListener {
    /// Create a new offset-saving rebalance listener
    pub fn new(on_assign: RebalanceCallbackFn, on_revoke: RebalanceCallbackFn) -> Self {
        Self {
            on_assign,
            on_revoke,
        }
    }
}

impl RebalanceCallback for OffsetSavingRebalanceListener {
    fn on_partitions_assigned(&self, partitions: &TopicPartitionList) {
        (self.on_assign)(partitions);
    }

    fn on_partitions_revoked(&self, partitions: &TopicPartitionList) {
        (self.on_revoke)(partitions);
    }
}

/// Rebalance listener builder
pub struct RebalanceListenerBuilder {
    on_assign: Option<RebalanceCallbackFn>,
    on_revoke: Option<RebalanceCallbackFn>,
}

impl RebalanceListenerBuilder {
    /// Create a new rebalance listener builder
    pub fn new() -> Self {
        Self {
            on_assign: None,
            on_revoke: None,
        }
    }

    /// Set callback for partition assignment
    pub fn on_assign<F>(mut self, callback: F) -> Self
    where
        F: Fn(&TopicPartitionList) + Send + Sync + 'static,
    {
        self.on_assign = Some(Arc::new(callback));
        self
    }

    /// Set callback for partition revocation
    pub fn on_revoke<F>(mut self, callback: F) -> Self
    where
        F: Fn(&TopicPartitionList) + Send + Sync + 'static,
    {
        self.on_revoke = Some(Arc::new(callback));
        self
    }

    /// Build the rebalance listener
    pub fn build(self) -> Arc<dyn RebalanceCallback> {
        let on_assign = self.on_assign.unwrap_or_else(|| {
            Arc::new(|partitions: &TopicPartitionList| {
                info!("Partitions assigned: {} partitions", partitions.count());
            })
        });

        let on_revoke = self.on_revoke.unwrap_or_else(|| {
            Arc::new(|partitions: &TopicPartitionList| {
                warn!("Partitions revoked: {} partitions", partitions.count());
            })
        });

        Arc::new(OffsetSavingRebalanceListener::new(on_assign, on_revoke))
    }
}

impl Default for RebalanceListenerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Rebalance listener that tracks partition state
#[derive(Debug, Default)]
pub struct StatefulRebalanceListener {
    assigned_partitions: Arc<parking_lot::RwLock<Vec<(String, i32)>>>,
}

impl StatefulRebalanceListener {
    /// Create a new stateful rebalance listener
    pub fn new() -> Self {
        Self::default()
    }

    /// Get currently assigned partitions
    pub fn assigned_partitions(&self) -> Vec<(String, i32)> {
        self.assigned_partitions.read().clone()
    }
}

impl RebalanceCallback for StatefulRebalanceListener {
    fn on_partitions_assigned(&self, partitions: &TopicPartitionList) {
        let mut assigned = self.assigned_partitions.write();
        assigned.clear();

        for elem in partitions.elements() {
            assigned.push((elem.topic().to_string(), elem.partition()));
            info!(
                "Partition assigned: {} [{}]",
                elem.topic(),
                elem.partition()
            );
        }
    }

    fn on_partitions_revoked(&self, partitions: &TopicPartitionList) {
        let mut assigned = self.assigned_partitions.write();

        for elem in partitions.elements() {
            let key = (elem.topic().to_string(), elem.partition());
            if let Some(pos) = assigned.iter().position(|x| *x == key) {
                assigned.remove(pos);
            }
            warn!("Partition revoked: {} [{}]", elem.topic(), elem.partition());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rebalance_listener() {
        let listener = DefaultRebalanceListener;
        let tpl = TopicPartitionList::new();

        // Just test that it doesn't crash
        listener.on_partitions_assigned(&tpl);
        listener.on_partitions_revoked(&tpl);
    }

    #[test]
    fn test_custom_rebalance_listener() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let assign_count = Arc::new(AtomicUsize::new(0));
        let revoke_count = Arc::new(AtomicUsize::new(0));

        let assign_count_clone = Arc::clone(&assign_count);
        let revoke_count_clone = Arc::clone(&revoke_count);

        let listener = CustomRebalanceListener::new(
            move |_| {
                assign_count_clone.fetch_add(1, Ordering::SeqCst);
            },
            move |_| {
                revoke_count_clone.fetch_add(1, Ordering::SeqCst);
            },
        );

        let tpl = TopicPartitionList::new();

        listener.on_partitions_assigned(&tpl);
        assert_eq!(assign_count.load(Ordering::SeqCst), 1);

        listener.on_partitions_revoked(&tpl);
        assert_eq!(revoke_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_stateful_rebalance_listener() {
        let listener = StatefulRebalanceListener::new();
        assert_eq!(listener.assigned_partitions().len(), 0);

        let mut tpl = TopicPartitionList::new();
        tpl.add_partition("topic1", 0);
        tpl.add_partition("topic1", 1);

        listener.on_partitions_assigned(&tpl);
        let assigned = listener.assigned_partitions();
        assert_eq!(assigned.len(), 2);
        assert!(assigned.contains(&("topic1".to_string(), 0)));
        assert!(assigned.contains(&("topic1".to_string(), 1)));

        let mut revoke_tpl = TopicPartitionList::new();
        revoke_tpl.add_partition("topic1", 0);

        listener.on_partitions_revoked(&revoke_tpl);
        let assigned = listener.assigned_partitions();
        assert_eq!(assigned.len(), 1);
        assert!(assigned.contains(&("topic1".to_string(), 1)));
    }

    #[test]
    fn test_rebalance_listener_builder() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let listener = RebalanceListenerBuilder::new()
            .on_assign(move |_| {
                called_clone.store(true, Ordering::SeqCst);
            })
            .build();

        let tpl = TopicPartitionList::new();
        listener.on_partitions_assigned(&tpl);

        assert!(called.load(Ordering::SeqCst));
    }
}
