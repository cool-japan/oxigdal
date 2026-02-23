//! Hydrological analysis module.

pub mod flow_accumulation;
pub mod flow_direction;
pub mod sink_fill;
pub mod stream_network;
pub mod watershed;

pub use flow_accumulation::flow_accumulation;
pub use flow_direction::{FlowAlgorithm, flow_direction, flow_direction_d8, flow_direction_dinf};
pub use sink_fill::fill_sinks;
pub use stream_network::{extract_streams, strahler_order};
pub use watershed::watershed_from_point;
