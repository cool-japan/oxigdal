//! Cluster health and distributed system metrics.

use crate::error::Result;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter, UpDownCounter};

/// Metrics for cluster operations.
pub struct ClusterMetrics {
    // Cluster health
    /// Total number of nodes in the cluster.
    pub cluster_nodes_total: UpDownCounter<i64>,
    /// Number of healthy nodes in the cluster.
    pub cluster_nodes_healthy: UpDownCounter<i64>,
    /// Number of unhealthy nodes in the cluster.
    pub cluster_nodes_unhealthy: UpDownCounter<i64>,

    // Node operations
    /// Counter for node join events.
    pub node_join_count: Counter<u64>,
    /// Counter for node leave events.
    pub node_leave_count: Counter<u64>,
    /// Counter for node heartbeat messages.
    pub node_heartbeat_count: Counter<u64>,
    /// Histogram of heartbeat durations in milliseconds.
    pub node_heartbeat_duration: Histogram<f64>,

    // Data distribution
    /// Counter for data transfer operations.
    pub data_transfer_count: Counter<u64>,
    /// Histogram of data transfer durations.
    pub data_transfer_duration: Histogram<f64>,
    /// Total bytes transferred between nodes.
    pub data_transfer_bytes: Counter<u64>,
    /// Counter for data replication operations.
    pub data_replication_count: Counter<u64>,
    /// Counter for data rebalance operations.
    pub data_rebalance_count: Counter<u64>,

    // Leader election
    /// Counter for leader election events.
    pub leader_election_count: Counter<u64>,
    /// Histogram of leader election durations.
    pub leader_election_duration: Histogram<f64>,
    /// Current leader term number.
    pub leader_term: UpDownCounter<i64>,

    // Consensus
    /// Counter for consensus proposals.
    pub consensus_proposals: Counter<u64>,
    /// Counter for committed proposals.
    pub consensus_commits: Counter<u64>,
    /// Counter for rejected proposals.
    pub consensus_rejections: Counter<u64>,
    /// Histogram of consensus operation durations.
    pub consensus_duration: Histogram<f64>,

    // Partition operations
    /// Current number of partitions.
    pub partition_count: UpDownCounter<i64>,
    /// Counter for partition reassignments.
    pub partition_reassignments: Counter<u64>,

    // Errors
    /// Counter for cluster errors.
    pub cluster_errors: Counter<u64>,
    /// Counter for split-brain detections.
    pub split_brain_detected: Counter<u64>,
}

impl ClusterMetrics {
    /// Create new cluster metrics.
    pub fn new(meter: Meter) -> Result<Self> {
        Ok(Self {
            // Cluster health
            cluster_nodes_total: meter
                .i64_up_down_counter("oxigdal.cluster.nodes.total")
                .with_description("Total number of cluster nodes")
                .init(),
            cluster_nodes_healthy: meter
                .i64_up_down_counter("oxigdal.cluster.nodes.healthy")
                .with_description("Number of healthy cluster nodes")
                .init(),
            cluster_nodes_unhealthy: meter
                .i64_up_down_counter("oxigdal.cluster.nodes.unhealthy")
                .with_description("Number of unhealthy cluster nodes")
                .init(),

            // Node operations
            node_join_count: meter
                .u64_counter("oxigdal.cluster.node.join")
                .with_description("Number of node joins")
                .init(),
            node_leave_count: meter
                .u64_counter("oxigdal.cluster.node.leave")
                .with_description("Number of node leaves")
                .init(),
            node_heartbeat_count: meter
                .u64_counter("oxigdal.cluster.node.heartbeat")
                .with_description("Number of node heartbeats")
                .init(),
            node_heartbeat_duration: meter
                .f64_histogram("oxigdal.cluster.node.heartbeat.duration")
                .with_description("Node heartbeat duration in milliseconds")
                .init(),

            // Data distribution
            data_transfer_count: meter
                .u64_counter("oxigdal.cluster.data_transfer.count")
                .with_description("Number of data transfers between nodes")
                .init(),
            data_transfer_duration: meter
                .f64_histogram("oxigdal.cluster.data_transfer.duration")
                .with_description("Data transfer duration in milliseconds")
                .init(),
            data_transfer_bytes: meter
                .u64_counter("oxigdal.cluster.data_transfer.bytes")
                .with_description("Bytes transferred between nodes")
                .init(),
            data_replication_count: meter
                .u64_counter("oxigdal.cluster.replication.count")
                .with_description("Number of data replication operations")
                .init(),
            data_rebalance_count: meter
                .u64_counter("oxigdal.cluster.rebalance.count")
                .with_description("Number of data rebalance operations")
                .init(),

            // Leader election
            leader_election_count: meter
                .u64_counter("oxigdal.cluster.leader_election.count")
                .with_description("Number of leader elections")
                .init(),
            leader_election_duration: meter
                .f64_histogram("oxigdal.cluster.leader_election.duration")
                .with_description("Leader election duration in milliseconds")
                .init(),
            leader_term: meter
                .i64_up_down_counter("oxigdal.cluster.leader.term")
                .with_description("Current leader term")
                .init(),

            // Consensus
            consensus_proposals: meter
                .u64_counter("oxigdal.cluster.consensus.proposals")
                .with_description("Number of consensus proposals")
                .init(),
            consensus_commits: meter
                .u64_counter("oxigdal.cluster.consensus.commits")
                .with_description("Number of consensus commits")
                .init(),
            consensus_rejections: meter
                .u64_counter("oxigdal.cluster.consensus.rejections")
                .with_description("Number of consensus rejections")
                .init(),
            consensus_duration: meter
                .f64_histogram("oxigdal.cluster.consensus.duration")
                .with_description("Consensus duration in milliseconds")
                .init(),

            // Partition operations
            partition_count: meter
                .i64_up_down_counter("oxigdal.cluster.partition.count")
                .with_description("Number of partitions")
                .init(),
            partition_reassignments: meter
                .u64_counter("oxigdal.cluster.partition.reassignments")
                .with_description("Number of partition reassignments")
                .init(),

            // Errors
            cluster_errors: meter
                .u64_counter("oxigdal.cluster.errors")
                .with_description("Number of cluster errors")
                .init(),
            split_brain_detected: meter
                .u64_counter("oxigdal.cluster.split_brain")
                .with_description("Number of split-brain detections")
                .init(),
        })
    }

    /// Record node joining cluster.
    pub fn record_node_join(&self, node_id: &str) {
        let attrs = vec![KeyValue::new("node_id", node_id.to_string())];
        self.node_join_count.add(1, &attrs);
        self.cluster_nodes_total.add(1, &attrs);
        self.cluster_nodes_healthy.add(1, &attrs);
    }

    /// Record node leaving cluster.
    pub fn record_node_leave(&self, node_id: &str, reason: &str) {
        let attrs = vec![
            KeyValue::new("node_id", node_id.to_string()),
            KeyValue::new("reason", reason.to_string()),
        ];
        self.node_leave_count.add(1, &attrs);
        self.cluster_nodes_total.add(-1, &attrs);
        self.cluster_nodes_healthy.add(-1, &attrs);
    }

    /// Record node heartbeat.
    pub fn record_heartbeat(&self, node_id: &str, duration_ms: f64, healthy: bool) {
        let attrs = vec![
            KeyValue::new("node_id", node_id.to_string()),
            KeyValue::new("healthy", healthy),
        ];

        self.node_heartbeat_count.add(1, &attrs);
        self.node_heartbeat_duration.record(duration_ms, &attrs);
    }

    /// Record data transfer between nodes.
    pub fn record_data_transfer(
        &self,
        duration_ms: f64,
        bytes: u64,
        from_node: &str,
        to_node: &str,
        success: bool,
    ) {
        let attrs = vec![
            KeyValue::new("from_node", from_node.to_string()),
            KeyValue::new("to_node", to_node.to_string()),
            KeyValue::new("success", success),
        ];

        self.data_transfer_count.add(1, &attrs);
        self.data_transfer_duration.record(duration_ms, &attrs);
        if success {
            self.data_transfer_bytes.add(bytes, &attrs);
        }
    }

    /// Record leader election.
    pub fn record_leader_election(&self, duration_ms: f64, new_leader: &str, term: i64) {
        let attrs = vec![
            KeyValue::new("new_leader", new_leader.to_string()),
            KeyValue::new("term", term),
        ];

        self.leader_election_count.add(1, &attrs);
        self.leader_election_duration.record(duration_ms, &attrs);
        self.leader_term.add(1, &attrs);
    }

    /// Record consensus operation.
    pub fn record_consensus(&self, duration_ms: f64, committed: bool) {
        let attrs = vec![KeyValue::new("committed", committed)];

        self.consensus_proposals.add(1, &attrs);
        self.consensus_duration.record(duration_ms, &attrs);

        if committed {
            self.consensus_commits.add(1, &attrs);
        } else {
            self.consensus_rejections.add(1, &attrs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::global;

    #[test]
    fn test_cluster_metrics_creation() {
        let meter = global::meter("test");
        let metrics = ClusterMetrics::new(meter);
        assert!(metrics.is_ok());
    }
}
