//! # OxiGDAL High Availability (HA)
//!
//! High availability, disaster recovery, and automatic failover for OxiGDAL.
//!
//! ## Features
//!
//! - **Active-Active Replication**: Bi-directional replication with conflict resolution
//! - **Automatic Failover**: Sub-second failover with leader election
//! - **Point-in-Time Recovery**: WAL-based recovery with snapshots
//! - **Incremental Backups**: Efficient backup strategies
//! - **Disaster Recovery**: Cross-region DR with automated runbooks
//! - **Health Monitoring**: Comprehensive health checks
//!
//! ## Target Availability
//!
//! - **Uptime**: 99.99% (52 minutes downtime/year)
//! - **Failover Time**: < 1 second
//! - **RTO**: < 5 minutes
//! - **RPO**: < 1 minute
//!
//! ## Example
//!
//! ```no_run
//! use oxigdal_ha::replication::active_active::ActiveActiveReplication;
//! use oxigdal_ha::replication::{ReplicationConfig, ReplicationManager};
//! use uuid::Uuid;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let node_id = Uuid::new_v4();
//!     let config = ReplicationConfig::default();
//!     let replication = ActiveActiveReplication::new(node_id, config);
//!
//!     replication.start().await?;
//!
//!     Ok(())
//! }
//! ```

pub mod backup;
pub mod conflict;
pub mod dr;
pub mod error;
pub mod failover;
pub mod healthcheck;
pub mod recovery;
pub mod replication;

pub use error::{HaError, HaResult};

/// Version information.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
