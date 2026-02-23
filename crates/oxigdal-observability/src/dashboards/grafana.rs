//! Grafana dashboard templates for OxiGDAL.

use super::{Dashboard, DashboardBuilder, GridPos, PanelConfig, PanelType, Query};
use crate::error::Result;

/// Create a comprehensive OxiGDAL performance dashboard.
pub fn create_performance_dashboard() -> Dashboard {
    DashboardBuilder::new("OxiGDAL Performance")
        .with_description("Comprehensive performance monitoring for OxiGDAL operations")
        .with_tag("oxigdal")
        .with_tag("performance")
        .with_refresh_interval(5)
        // Raster operations panel
        .add_panel(PanelConfig {
            title: "Raster Operations Rate".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![
                Query {
                    expr: "rate(oxigdal_raster_read_count[5m])".to_string(),
                    legend_format: Some("Read {{format}}".to_string()),
                    interval: None,
                },
                Query {
                    expr: "rate(oxigdal_raster_write_count[5m])".to_string(),
                    legend_format: Some("Write {{format}}".to_string()),
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
        // Cache hit ratio panel
        .add_panel(PanelConfig {
            title: "Cache Hit Ratio".to_string(),
            panel_type: PanelType::Gauge,
            queries: vec![Query {
                expr: "oxigdal_cache_hits / (oxigdal_cache_hits + oxigdal_cache_misses)"
                    .to_string(),
                legend_format: Some("{{cache_type}}".to_string()),
                interval: None,
            }],
            grid_pos: GridPos {
                x: 12,
                y: 0,
                w: 6,
                h: 8,
            },
        })
        // Query performance panel
        .add_panel(PanelConfig {
            title: "Query Duration (p95)".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![Query {
                expr: "histogram_quantile(0.95, rate(oxigdal_query_duration_bucket[5m]))"
                    .to_string(),
                legend_format: Some("{{query_type}}".to_string()),
                interval: None,
            }],
            grid_pos: GridPos {
                x: 18,
                y: 0,
                w: 6,
                h: 8,
            },
        })
        .build()
}

/// Create cluster health dashboard.
pub fn create_cluster_dashboard() -> Dashboard {
    DashboardBuilder::new("OxiGDAL Cluster Health")
        .with_description("Cluster health and distributed system monitoring")
        .with_tag("oxigdal")
        .with_tag("cluster")
        .with_refresh_interval(10)
        // Cluster nodes panel
        .add_panel(PanelConfig {
            title: "Cluster Nodes".to_string(),
            panel_type: PanelType::Singlestat,
            queries: vec![
                Query {
                    expr: "oxigdal_cluster_nodes_total".to_string(),
                    legend_format: Some("Total".to_string()),
                    interval: None,
                },
                Query {
                    expr: "oxigdal_cluster_nodes_healthy".to_string(),
                    legend_format: Some("Healthy".to_string()),
                    interval: None,
                },
            ],
            grid_pos: GridPos {
                x: 0,
                y: 0,
                w: 6,
                h: 4,
            },
        })
        // Data transfer rate panel
        .add_panel(PanelConfig {
            title: "Data Transfer Rate".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![Query {
                expr: "rate(oxigdal_cluster_data_transfer_bytes[5m])".to_string(),
                legend_format: Some("{{from_node}} -> {{to_node}}".to_string()),
                interval: None,
            }],
            grid_pos: GridPos {
                x: 6,
                y: 0,
                w: 18,
                h: 8,
            },
        })
        .build()
}

/// Create GPU utilization dashboard.
pub fn create_gpu_dashboard() -> Dashboard {
    DashboardBuilder::new("OxiGDAL GPU Utilization")
        .with_description("GPU utilization and performance monitoring")
        .with_tag("oxigdal")
        .with_tag("gpu")
        .with_refresh_interval(5)
        // GPU utilization panel
        .add_panel(PanelConfig {
            title: "GPU Utilization".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![Query {
                expr: "oxigdal_gpu_utilization_percent".to_string(),
                legend_format: Some("GPU {{gpu_id}}".to_string()),
                interval: None,
            }],
            grid_pos: GridPos {
                x: 0,
                y: 0,
                w: 12,
                h: 8,
            },
        })
        // GPU memory usage panel
        .add_panel(PanelConfig {
            title: "GPU Memory Usage".to_string(),
            panel_type: PanelType::Graph,
            queries: vec![Query {
                expr: "oxigdal_gpu_memory_used_bytes / oxigdal_gpu_memory_total_bytes * 100"
                    .to_string(),
                legend_format: Some("GPU {{gpu_id}}".to_string()),
                interval: None,
            }],
            grid_pos: GridPos {
                x: 12,
                y: 0,
                w: 12,
                h: 8,
            },
        })
        .build()
}

/// Export dashboard as Grafana JSON.
pub fn export_grafana_json(dashboard: &Dashboard) -> Result<String> {
    dashboard.to_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_dashboard() {
        let dashboard = create_performance_dashboard();
        assert!(!dashboard.panels.is_empty());
        assert_eq!(dashboard.config.title, "OxiGDAL Performance");
    }

    #[test]
    fn test_cluster_dashboard() {
        let dashboard = create_cluster_dashboard();
        assert!(!dashboard.panels.is_empty());
    }

    #[test]
    fn test_gpu_dashboard() {
        let dashboard = create_gpu_dashboard();
        assert!(!dashboard.panels.is_empty());
    }
}
