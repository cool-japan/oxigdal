//! Hydrological analysis algorithms
//!
//! This module provides comprehensive hydrological analysis tools for
//! digital elevation models (DEMs), implementing well-known algorithms
//! from the literature:
//!
//! - **Flow direction**: D8 (Jenson & Domingue 1988), D-Infinity (Tarboton 1997),
//!   MFD (Freeman 1991), with flat-area resolution (Garbrecht & Martz 1997)
//! - **Flow accumulation**: D8, D-Infinity, and MFD accumulation with optional
//!   weight grids and stream threshold extraction
//! - **Sink filling**: Wang & Liu (2006) priority-flood, Planchon & Darboux (2001)
//!   iterative, breach depressions (Lindsay & Dhun 2015 style)
//! - **Stream network**: Strahler order, Shreve magnitude, stream heads,
//!   stream links, vectorisation
//! - **Watersheds**: Pour-point delineation, sub-basin delineation, watershed
//!   hierarchy, snap pour points

pub mod fill_sinks;
pub mod flow_accumulation;
pub mod flow_direction;
pub mod stream_network;
pub mod watersheds;

// -- Flow direction --
pub use flow_direction::{
    D8_DX, D8_DY, D8_FLAT, D8_PIT, D8Config, D8Direction, FlowDirectionResult, FlowMethod,
    MfdConfig, MfdResult, compute_d8_flow_direction, compute_d8_flow_direction_cfg,
    compute_dinf_flow_direction, compute_flow_direction, compute_mfd_flow_direction,
};

// -- Flow accumulation --
pub use flow_accumulation::{
    StreamThresholdConfig, compute_d8_accumulation_from_fdir, compute_dinf_flow_accumulation,
    compute_dinf_flow_accumulation_weighted, compute_flow_accumulation,
    compute_mfd_accumulation_from_result, compute_mfd_flow_accumulation,
    compute_mfd_flow_accumulation_weighted, compute_weighted_flow_accumulation,
    extract_streams_by_threshold,
};

// -- Sink filling --
pub use fill_sinks::{
    FillMethod, FillSinksConfig, breach_depressions, compute_fill_depth, fill_sinks,
    fill_sinks_cfg, identify_sinks,
};

// -- Stream network --
pub use stream_network::{
    StreamSegment, compute_shreve_magnitude, compute_strahler_order, compute_stream_links,
    compute_stream_order, extract_stream_network, identify_stream_heads, vectorize_streams,
};

// -- Watersheds --
pub use watersheds::{
    WatershedNode, build_watershed_hierarchy, delineate_sub_basins, delineate_watersheds,
    delineate_watersheds_from_fdir, label_all_watersheds, snap_pour_points,
};
