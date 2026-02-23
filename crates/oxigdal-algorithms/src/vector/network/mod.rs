//! Network analysis for geospatial routing and accessibility
//!
//! This module provides comprehensive network analysis capabilities:
//!
//! - **Graph structures**: Build network graphs from vector data with directed/undirected support
//! - **Edge weights**: Multi-criteria weights (distance, time, monetary cost)
//! - **Graph validation**: Structural integrity checks, self-loop/parallel edge detection
//! - **Topology cleaning**: Snap nodes, remove duplicates, contract degree-2 chains
//! - **Shortest path**: Dijkstra, A*, bidirectional search, Floyd-Warshall
//! - **Turn restrictions**: Edge-based pathfinding with turn penalties and prohibitions
//! - **Time-dependent routing**: Variable edge costs based on time of day
//! - **Service areas**: Calculate isochrones and accessibility zones
//! - **Multi-facility**: Service areas from multiple origins with overlap detection
//! - **Routing**: Advanced routing with turn penalties, alternatives, waypoint optimization
//!
//! All algorithms are optimized for large-scale geospatial networks.

mod graph;
mod graph_ops;
mod routing;
mod service_area;
mod shortest_path;

pub use graph::{
    Edge, EdgeId, EdgeWeight, Graph, GraphBuilder, GraphMetrics, GraphType, Node, NodeId, RoadClass,
};
pub use graph_ops::{
    ConnectedComponent, NetworkEdge, NetworkNode, TimeDependentWeight, TopologyCleanResult,
    TurnPenalties, TurnPenalty, ValidationIssue, ValidationResult, ValidationSeverity,
    haversine_distance,
};
pub use routing::{
    Route, RouteOptions, RouteResult, RouteSegment, RoutingAlgorithm, RoutingCriteria,
    TurnRestriction, WaypointOptimizationResult, calculate_route,
    calculate_route_with_alternatives, calculate_routes_batch, od_matrix, optimize_waypoint_order,
};
pub use service_area::{
    AccessibilityResult, Isochrone, IsochroneOptions, IsochronePolygonMethod, MultiFacilityResult,
    OverlapZone, ServiceArea, ServiceAreaCostType, ServiceAreaInterval, ServiceAreaOptions,
    accessibility_score, calculate_drive_time_polygons, calculate_isochrones,
    calculate_multi_facility_service_area, calculate_multi_isochrones, calculate_service_area,
};
pub use shortest_path::{
    AllPairsResult, PathFindingAlgorithm, ShortestPath, ShortestPathOptions, astar_search,
    astar_turn_restricted, bidirectional_search, dijkstra_search, dijkstra_single_source,
    dijkstra_turn_restricted, floyd_warshall, time_dependent_dijkstra,
};
