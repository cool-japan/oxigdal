//! Vector quality control modules.

pub mod attribution;
pub mod topology;
pub mod violations;

pub use attribution::{AttributionChecker, AttributionConfig, AttributionResult};
pub use topology::{
    TopologyChecker, TopologyConfig, TopologyResult, check_topology_rules, has_self_intersection,
};
pub use violations::{TopologyOptions, TopologyViolation};
