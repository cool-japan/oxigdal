//! Pre-built dashboard templates for common scenarios.

use super::{Dashboard, DashboardBuilder, GridPos, PanelConfig, PanelType, Query};

/// Create I/O performance dashboard.
pub fn create_io_performance_dashboard() -> Dashboard {
    DashboardBuilder::new("OxiGDAL I/O Performance")
        .with_description("Monitor I/O operations, throughput, and latency")
        .with_tag("oxigdal")
        .with_tag("io")
        .with_refresh_interval(5)
        // File I/O panel
        .add_panel(PanelConfig {
            title: "File I/O Operations".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_io_file_read_count[5m])".to_string(),
                    legend_format: Some("Read".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_io_file_write_count[5m])".to_string(),
                    legend_format: Some("Write".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos {
                x: 0,
                y: 0,
                w: 12,
                h: 8,
            },
        })
        // Throughput panel
        .add_panel(PanelConfig {
            title: "I/O Throughput (MB/s)".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "oxigdal_io_read_throughput_mbps".to_string(),
                    legend_format: Some("Read".to_string()),
                    interval: None,
                },
                Query {
                    expr: "oxigdal_io_write_throughput_mbps".to_string(),
                    legend_format: Some("Write".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos {
                x: 12,
                y: 0,
                w: 12,
                h: 8,
            },
        })
        // Latency panel
        .add_panel(PanelConfig {
            title: "I/O Latency (ms)".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "histogram_quantile(0.95, rate(oxigdal_io_read_latency_ms_bucket[5m]))"
                        .to_string(),
                    legend_format: Some("Read p95".to_string()),
                    interval: None,
                },
                Query {
                    expr: "histogram_quantile(0.99, rate(oxigdal_io_read_latency_ms_bucket[5m]))"
                        .to_string(),
                    legend_format: Some("Read p99".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos {
                x: 0,
                y: 8,
                w: 12,
                h: 8,
            },
        })
        // Network I/O panel
        .add_panel(PanelConfig {
            title: "Network I/O".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_io_network_bytes_sent[5m])".to_string(),
                    legend_format: Some("Sent".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_io_network_bytes_received[5m])".to_string(),
                    legend_format: Some("Received".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos {
                x: 12,
                y: 8,
                w: 12,
                h: 8,
            },
        })
        // Cloud storage panel
        .add_panel(PanelConfig {
            title: "Cloud Storage Operations".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_io_cloud_get_count[5m])".to_string(),
                    legend_format: Some("GET {{provider}}".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_io_cloud_put_count[5m])".to_string(),
                    legend_format: Some("PUT {{provider}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos {
                x: 0,
                y: 16,
                w: 12,
                h: 8,
            },
        })
        // Error rate panel
        .add_panel(PanelConfig {
            title: "Network Error Rate".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![Query {
                expr: "rate(oxigdal_io_network_errors[5m])".to_string(),
                legend_format: Some("Errors".to_string()),
                interval: None,
            }],
            grid_pos: GridPos {
                x: 12,
                y: 16,
                w: 12,
                h: 8,
            },
        })
        .build()
}

/// Create vector operations dashboard.
pub fn create_vector_operations_dashboard() -> Dashboard {
    DashboardBuilder::new("OxiGDAL Vector Operations")
        .with_description("Monitor vector data processing and spatial operations")
        .with_tag("oxigdal")
        .with_tag("vector")
        .with_refresh_interval(5)
        // Feature I/O panel
        .add_panel(PanelConfig {
            title: "Feature I/O Rate".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_vector_features_read[5m])".to_string(),
                    legend_format: Some("Read {{format}}".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_vector_features_written[5m])".to_string(),
                    legend_format: Some("Write {{format}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 0, y: 0, w: 12, h: 8 },
        })
        // Geometry operations panel
        .add_panel(PanelConfig {
            title: "Geometry Operations".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_vector_buffer_count[5m])".to_string(),
                    legend_format: Some("Buffer".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_vector_intersection_count[5m])".to_string(),
                    legend_format: Some("Intersection".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_vector_union_count[5m])".to_string(),
                    legend_format: Some("Union".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_vector_simplify_count[5m])".to_string(),
                    legend_format: Some("Simplify".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 12, y: 0, w: 12, h: 8 },
        })
        // Spatial query performance panel
        .add_panel(PanelConfig {
            title: "Spatial Query Duration (p95)".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "histogram_quantile(0.95, rate(oxigdal_vector_spatial_query_duration_bucket[5m]))".to_string(),
                    legend_format: Some("{{query_type}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 0, y: 8, w: 12, h: 8 },
        })
        // Active layers panel
        .add_panel(PanelConfig {
            title: "Active Vector Layers".to_string(),
            panel_type: PanelType::Gauge,
            queries: vec![
                Query {
                    expr: "oxigdal_vector_active_layers".to_string(),
                    legend_format: Some("Layers".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 12, y: 8, w: 6, h: 8 },
        })
        // Feature count panel
        .add_panel(PanelConfig {
            title: "Feature Count Distribution".to_string(),
            panel_type: PanelType::Heatmap,
            queries: vec![
                Query {
                    expr: "oxigdal_vector_feature_count".to_string(),
                    legend_format: Some("{{layer_type}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 18, y: 8, w: 6, h: 8 },
        })
        .build()
}

/// Create cache efficiency dashboard.
pub fn create_cache_efficiency_dashboard() -> Dashboard {
    DashboardBuilder::new("OxiGDAL Cache Efficiency")
        .with_description("Monitor cache performance and efficiency")
        .with_tag("oxigdal")
        .with_tag("cache")
        .with_refresh_interval(5)
        // Hit ratio panel
        .add_panel(PanelConfig {
            title: "Cache Hit Ratio".to_string(),
            panel_type: PanelType::Gauge,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_cache_hits[5m]) / (rate(oxigdal_cache_hits[5m]) + rate(oxigdal_cache_misses[5m]))".to_string(),
                    legend_format: Some("{{cache_type}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 0, y: 0, w: 6, h: 8 },
        })
        // Hit/miss rate panel
        .add_panel(PanelConfig {
            title: "Cache Operations".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_cache_hits[5m])".to_string(),
                    legend_format: Some("Hits {{cache_type}}".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_cache_misses[5m])".to_string(),
                    legend_format: Some("Misses {{cache_type}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 6, y: 0, w: 12, h: 8 },
        })
        // Eviction rate panel
        .add_panel(PanelConfig {
            title: "Cache Evictions".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_cache_evictions[5m])".to_string(),
                    legend_format: Some("{{cache_type}} - {{reason}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 18, y: 0, w: 6, h: 8 },
        })
        // Cache size panel
        .add_panel(PanelConfig {
            title: "Cache Size (MB)".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "oxigdal_cache_size_bytes / 1024 / 1024".to_string(),
                    legend_format: Some("{{cache_type}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 0, y: 8, w: 12, h: 8 },
        })
        // Entry count panel
        .add_panel(PanelConfig {
            title: "Cache Entries".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "oxigdal_cache_entries".to_string(),
                    legend_format: Some("{{cache_type}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 12, y: 8, w: 12, h: 8 },
        })
        // Time saved panel
        .add_panel(PanelConfig {
            title: "Time Saved by Cache (seconds)".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "sum(rate(oxigdal_cache_time_saved_ms[5m])) / 1000".to_string(),
                    legend_format: Some("Total".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 0, y: 16, w: 12, h: 8 },
        })
        // Prefetch efficiency panel
        .add_panel(PanelConfig {
            title: "Prefetch Efficiency".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_cache_prefetch_hits[5m])".to_string(),
                    legend_format: Some("Hits {{cache_type}}".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_cache_prefetch_waste[5m])".to_string(),
                    legend_format: Some("Waste {{cache_type}}".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos { x: 12, y: 16, w: 12, h: 8 },
        })
        .build()
}

/// Create all pre-built dashboards.
pub fn create_all_dashboards() -> Vec<Dashboard> {
    vec![
        super::grafana::create_performance_dashboard(),
        super::grafana::create_cluster_dashboard(),
        super::grafana::create_gpu_dashboard(),
        create_io_performance_dashboard(),
        create_vector_operations_dashboard(),
        create_cache_efficiency_dashboard(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_dashboard() {
        let dashboard = create_io_performance_dashboard();
        assert!(!dashboard.panels.is_empty());
        assert_eq!(dashboard.config.title, "OxiGDAL I/O Performance");
    }

    #[test]
    fn test_vector_dashboard() {
        let dashboard = create_vector_operations_dashboard();
        assert!(!dashboard.panels.is_empty());
    }

    #[test]
    fn test_cache_dashboard() {
        let dashboard = create_cache_efficiency_dashboard();
        assert!(!dashboard.panels.is_empty());
    }

    #[test]
    fn test_all_dashboards() {
        let dashboards = create_all_dashboards();
        assert_eq!(dashboards.len(), 6);
    }
}
