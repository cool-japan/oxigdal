//! Hydrological analysis module.

pub mod catchment;
pub mod channel_network;
pub mod flow_accumulation;
pub mod flow_direction;
pub mod sink_fill;
pub mod stream_network;
pub mod watershed;

pub use catchment::{CatchmentInfo, SnapPolicy, delineate_catchments};
pub use channel_network::{ChannelSegment, ThresholdMode, extract_channel_network};
pub use flow_accumulation::{flow_accumulation, flow_accumulation_dinf};
pub use flow_direction::{FlowAlgorithm, flow_direction, flow_direction_d8, flow_direction_dinf};
pub use sink_fill::{fill_sinks, fill_sinks_iterative, fill_sinks_priority_flood};
pub use stream_network::{extract_streams, strahler_order, strahler_order_from_d8};
pub use watershed::watershed_from_point;
