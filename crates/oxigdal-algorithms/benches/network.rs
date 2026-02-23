//! Comprehensive benchmarks for network analysis algorithms
//!
//! This benchmark suite tests shortest path, service area, and routing algorithms
//! on various network topologies and sizes.
//!
//! ## Performance Characteristics
//!
//! - **Dijkstra**: O((V + E) log V) - guaranteed shortest path
//! - **A***: O((V + E) log V) - faster with good heuristic, same worst case
//! - **Bidirectional**: O((V + E) log V) - often 2x faster than Dijkstra
//! - **Service Area**: O((V + E) log V) - similar to Dijkstra
//! - **Batch Routing**: O(n * (V + E) log V) - n queries
//!
//! ## Network Topologies
//!
//! - **Grid**: Regular street grid (urban areas)
//! - **Random**: Random connections (rural networks)
//! - **Scale-Free**: Hub-and-spoke (highway systems)
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast,
    clippy::needless_range_loop
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::vector::network::{
    Graph, IsochroneOptions, IsochronePolygonMethod, RouteOptions, ServiceAreaCostType,
    ServiceAreaOptions, ShortestPathOptions, astar_search, bidirectional_search,
    calculate_isochrones, calculate_routes_batch, calculate_service_area, dijkstra_search,
};
use oxigdal_core::vector::Coordinate;
use std::hint::black_box;

/// Network topology type for test graph generation
#[derive(Debug, Clone, Copy)]
enum NetworkTopology {
    /// Regular grid (like city streets)
    Grid,
    /// Random connections
    Random,
    /// Hub-and-spoke pattern
    HubSpoke,
}

/// Creates a test network graph
///
/// # Arguments
///
/// * `node_count` - Number of nodes to create
/// * `avg_degree` - Average number of connections per node
/// * `topology` - Network topology pattern
fn create_test_graph(node_count: usize, avg_degree: usize, topology: NetworkTopology) -> Graph {
    let mut graph = Graph::new();
    let mut nodes = Vec::new();

    match topology {
        NetworkTopology::Grid => {
            // Create grid topology
            let grid_size = (node_count as f64).sqrt() as usize;

            for y in 0..grid_size {
                for x in 0..grid_size {
                    let coord = Coordinate {
                        x: x as f64 * 100.0,
                        y: y as f64 * 100.0,
                        z: None,
                        m: None,
                    };
                    let node = graph.add_node(coord);
                    nodes.push(node);
                }
            }

            // Connect grid nodes (4-connected)
            for y in 0..grid_size {
                for x in 0..grid_size {
                    let idx = y * grid_size + x;

                    // Connect to right neighbor
                    if x + 1 < grid_size {
                        let neighbor = y * grid_size + (x + 1);
                        let weight = 100.0; // Edge weight
                        let _ = graph.add_edge(nodes[idx], nodes[neighbor], weight);
                        let _ = graph.add_edge(nodes[neighbor], nodes[idx], weight); // Bidirectional
                    }

                    // Connect to bottom neighbor
                    if y + 1 < grid_size {
                        let neighbor = (y + 1) * grid_size + x;
                        let weight = 100.0;
                        let _ = graph.add_edge(nodes[idx], nodes[neighbor], weight);
                        let _ = graph.add_edge(nodes[neighbor], nodes[idx], weight);
                    }
                }
            }
        }

        NetworkTopology::Random => {
            // Create nodes at random positions
            for i in 0..node_count {
                let coord = Coordinate {
                    x: (i * 173 % 1000) as f64,
                    y: (i * 271 % 1000) as f64,
                    z: None,
                    m: None,
                };
                let node = graph.add_node(coord);
                nodes.push(node);
            }

            // Create random connections
            let edge_count = (node_count * avg_degree) / 2;
            for i in 0..edge_count {
                let src = (i * 17) % node_count;
                let dst = (i * 31 + 7) % node_count;
                if src != dst {
                    let weight = 50.0 + ((i * 13) % 100) as f64;
                    let _ = graph.add_edge(nodes[src], nodes[dst], weight);
                    let _ = graph.add_edge(nodes[dst], nodes[src], weight);
                }
            }
        }

        NetworkTopology::HubSpoke => {
            // Create hub-and-spoke topology
            let hub_count = (node_count as f64).sqrt() as usize;
            let spokes_per_hub = (node_count - hub_count) / hub_count.max(1);

            // Create hub nodes
            for i in 0..hub_count {
                let angle = (i as f64 / hub_count as f64) * 2.0 * std::f64::consts::PI;
                let coord = Coordinate {
                    x: 500.0 + 400.0 * angle.cos(),
                    y: 500.0 + 400.0 * angle.sin(),
                    z: None,
                    m: None,
                };
                let node = graph.add_node(coord);
                nodes.push(node);
            }

            // Connect hubs in a ring
            for i in 0..hub_count {
                let next = (i + 1) % hub_count;
                let weight = 200.0;
                let _ = graph.add_edge(nodes[i], nodes[next], weight);
                let _ = graph.add_edge(nodes[next], nodes[i], weight);
            }

            // Create spoke nodes
            for hub_idx in 0..hub_count {
                for spoke in 0..spokes_per_hub {
                    let angle = (spoke as f64 / spokes_per_hub as f64) * 2.0 * std::f64::consts::PI
                        + (hub_idx as f64);
                    let radius = 100.0 + (spoke * 20) as f64;
                    let hub_coord = &graph
                        .get_node(nodes[hub_idx])
                        .expect("hub node not found in benchmark")
                        .coordinate;

                    let coord = Coordinate {
                        x: hub_coord.x + radius * angle.cos(),
                        y: hub_coord.y + radius * angle.sin(),
                        z: None,
                        m: None,
                    };
                    let spoke_node = graph.add_node(coord);

                    // Connect spoke to hub
                    let weight = 100.0;
                    let _ = graph.add_edge(spoke_node, nodes[hub_idx], weight);
                    let _ = graph.add_edge(nodes[hub_idx], spoke_node, weight);
                }
            }
        }
    }

    graph
}

/// Benchmark Dijkstra's algorithm with varying network sizes
///
/// Time complexity: O((V + E) log V)
fn bench_dijkstra(c: &mut Criterion) {
    let mut group = c.benchmark_group("dijkstra");

    for &node_count in &[100, 500, 1000, 5000, 10000] {
        let graph = create_test_graph(node_count, 4, NetworkTopology::Grid);
        let nodes = graph.node_ids();

        if nodes.len() < 2 {
            continue;
        }

        let start = nodes[0];
        let end = nodes[nodes.len() - 1];

        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(node_count),
            &node_count,
            |b, _| {
                b.iter(|| {
                    dijkstra_search(
                        black_box(&graph),
                        black_box(start),
                        black_box(end),
                        black_box(&ShortestPathOptions::default()),
                    )
                    .expect("Dijkstra failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark A* algorithm
///
/// Time complexity: O((V + E) log V) worst case, often much faster
fn bench_astar(c: &mut Criterion) {
    let mut group = c.benchmark_group("astar");

    for &node_count in &[100, 500, 1000, 5000, 10000] {
        let graph = create_test_graph(node_count, 4, NetworkTopology::Grid);
        let nodes = graph.node_ids();

        if nodes.len() < 2 {
            continue;
        }

        let start = nodes[0];
        let end = nodes[nodes.len() - 1];

        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(node_count),
            &node_count,
            |b, _| {
                b.iter(|| {
                    astar_search(
                        black_box(&graph),
                        black_box(start),
                        black_box(end),
                        black_box(&ShortestPathOptions::default()),
                    )
                    .expect("A* failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark bidirectional search
///
/// Time complexity: O((V + E) log V), often 2x faster than unidirectional
fn bench_bidirectional(c: &mut Criterion) {
    let mut group = c.benchmark_group("bidirectional");

    for &node_count in &[100, 500, 1000, 5000, 10000] {
        let graph = create_test_graph(node_count, 4, NetworkTopology::Grid);
        let nodes = graph.node_ids();

        if nodes.len() < 2 {
            continue;
        }

        let start = nodes[0];
        let end = nodes[nodes.len() - 1];

        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(node_count),
            &node_count,
            |b, _| {
                b.iter(|| {
                    bidirectional_search(
                        black_box(&graph),
                        black_box(start),
                        black_box(end),
                        black_box(&ShortestPathOptions::default()),
                    )
                    .expect("Bidirectional search failed")
                });
            },
        );
    }

    group.finish();
}

/// Compare pathfinding algorithms on the same graph
fn bench_algorithm_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("pathfinding_comparison");

    let node_count = 5000;
    let graph = create_test_graph(node_count, 4, NetworkTopology::Grid);
    let nodes = graph.node_ids();

    if nodes.len() < 2 {
        return;
    }

    let start = nodes[0];
    let end = nodes[nodes.len() - 1];

    group.throughput(Throughput::Elements(node_count as u64));

    group.bench_function("dijkstra", |b| {
        b.iter(|| {
            dijkstra_search(
                black_box(&graph),
                black_box(start),
                black_box(end),
                black_box(&ShortestPathOptions::default()),
            )
            .expect("Dijkstra failed")
        });
    });

    group.bench_function("astar", |b| {
        b.iter(|| {
            astar_search(
                black_box(&graph),
                black_box(start),
                black_box(end),
                black_box(&ShortestPathOptions::default()),
            )
            .expect("A* failed")
        });
    });

    group.bench_function("bidirectional", |b| {
        b.iter(|| {
            bidirectional_search(
                black_box(&graph),
                black_box(start),
                black_box(end),
                black_box(&ShortestPathOptions::default()),
            )
            .expect("Bidirectional failed")
        });
    });

    group.finish();
}

/// Benchmark service area calculation
///
/// Time complexity: O((V + E) log V)
fn bench_service_area(c: &mut Criterion) {
    let mut group = c.benchmark_group("service_area");

    for &node_count in &[100, 500, 1000, 5000] {
        let graph = create_test_graph(node_count, 4, NetworkTopology::Grid);
        let nodes = graph.node_ids();

        if nodes.is_empty() {
            continue;
        }

        let start = nodes[0];
        let max_cost = 500.0;

        let options = ServiceAreaOptions {
            max_cost,
            intervals: vec![100.0, 200.0, 300.0, 400.0, 500.0],
            include_unreachable: false,
            cost_type: ServiceAreaCostType::Distance,
            weight_criteria: None,
        };

        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(node_count),
            &node_count,
            |b, _| {
                b.iter(|| {
                    calculate_service_area(black_box(&graph), black_box(start), black_box(&options))
                        .expect("Service area failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark isochrone calculation
fn bench_isochrones(c: &mut Criterion) {
    let mut group = c.benchmark_group("isochrones");

    for &node_count in &[100, 500, 1000, 5000] {
        let graph = create_test_graph(node_count, 4, NetworkTopology::Grid);
        let nodes = graph.node_ids();

        if nodes.is_empty() {
            continue;
        }

        let start = nodes[0];

        let options = IsochroneOptions {
            time_intervals: vec![100.0, 200.0, 300.0, 400.0, 500.0],
            smooth: true,
            smoothing_factor: 0.5,
            polygon_method: IsochronePolygonMethod::ConvexHull,
        };

        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(node_count),
            &node_count,
            |b, _| {
                b.iter(|| {
                    calculate_isochrones(black_box(&graph), black_box(start), black_box(&options))
                        .expect("Isochrone failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark batch routing (multiple queries)
fn bench_batch_routing(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_routing");

    let node_count = 1000;
    let graph = create_test_graph(node_count, 4, NetworkTopology::Grid);
    let nodes = graph.node_ids();

    if nodes.len() < 10 {
        return;
    }

    for &batch_size in &[10, 50, 100, 500] {
        // Create query pairs
        let mut queries = Vec::new();
        for i in 0..batch_size {
            let start_idx = (i * 17) % nodes.len();
            let end_idx = (i * 31 + 11) % nodes.len();
            queries.push((nodes[start_idx], nodes[end_idx]));
        }

        let options = RouteOptions::default();

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    calculate_routes_batch(
                        black_box(&graph),
                        black_box(&queries),
                        black_box(&options),
                    )
                    .expect("Batch routing failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark different network topologies
fn bench_network_topologies(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_topologies");

    let node_count = 2000;
    let topologies = [
        ("grid", NetworkTopology::Grid),
        ("random", NetworkTopology::Random),
        ("hub_spoke", NetworkTopology::HubSpoke),
    ];

    for (name, topology) in topologies.iter() {
        let graph = create_test_graph(node_count, 4, *topology);
        let nodes = graph.node_ids();

        if nodes.len() < 2 {
            continue;
        }

        let start = nodes[0];
        let end = nodes[nodes.len() / 2];

        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_function(*name, |b| {
            b.iter(|| {
                dijkstra_search(
                    black_box(&graph),
                    black_box(start),
                    black_box(end),
                    black_box(&ShortestPathOptions::default()),
                )
                .expect("Dijkstra failed")
            });
        });
    }

    group.finish();
}

/// Benchmark network density impact (sparse vs dense)
fn bench_network_density(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_density");

    let node_count = 1000;

    for &avg_degree in &[2, 4, 6, 8, 10] {
        let graph = create_test_graph(node_count, avg_degree, NetworkTopology::Random);
        let nodes = graph.node_ids();

        if nodes.len() < 2 {
            continue;
        }

        let start = nodes[0];
        let end = nodes[nodes.len() - 1];

        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::new("degree", avg_degree),
            &avg_degree,
            |b, _| {
                b.iter(|| {
                    dijkstra_search(
                        black_box(&graph),
                        black_box(start),
                        black_box(end),
                        black_box(&ShortestPathOptions::default()),
                    )
                    .expect("Dijkstra failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark path length impact (short vs long paths)
fn bench_path_length(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_length");

    let grid_size = 100;
    let node_count = grid_size * grid_size;
    let graph = create_test_graph(node_count, 4, NetworkTopology::Grid);
    let nodes = graph.node_ids();

    if nodes.is_empty() {
        return;
    }

    let start = nodes[0];

    // Different path lengths (as percentage of max distance)
    for &pct in &[10, 25, 50, 75, 100] {
        let end_idx = (nodes.len() * pct / 100).min(nodes.len() - 1);
        let end = nodes[end_idx];

        group.bench_with_input(BenchmarkId::new("distance_pct", pct), &pct, |b, _| {
            b.iter(|| {
                dijkstra_search(
                    black_box(&graph),
                    black_box(start),
                    black_box(end),
                    black_box(&ShortestPathOptions::default()),
                )
                .expect("Dijkstra failed")
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_dijkstra,
    bench_astar,
    bench_bidirectional,
    bench_algorithm_comparison,
    bench_service_area,
    bench_isochrones,
    bench_batch_routing,
    bench_network_topologies,
    bench_network_density,
    bench_path_length,
);

criterion_main!(benches);
