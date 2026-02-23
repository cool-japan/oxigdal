//! Real-time performance dashboards (Grafana templates).

pub mod grafana;
pub mod prometheus;
pub mod templates;

use crate::error::Result;
use serde::{Deserialize, Serialize};

/// Dashboard configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    /// Dashboard title.
    pub title: String,

    /// Dashboard description.
    pub description: String,

    /// Dashboard tags.
    pub tags: Vec<String>,

    /// Refresh interval in seconds.
    pub refresh_interval: u32,

    /// Time range.
    pub time_range: TimeRange,
}

/// Time range for dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    /// Start time (e.g., "now-1h").
    pub from: String,

    /// End time (e.g., "now").
    pub to: String,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            title: "OxiGDAL Dashboard".to_string(),
            description: "Real-time performance monitoring for OxiGDAL".to_string(),
            tags: vec!["oxigdal".to_string(), "geospatial".to_string()],
            refresh_interval: 5,
            time_range: TimeRange {
                from: "now-1h".to_string(),
                to: "now".to_string(),
            },
        }
    }
}

/// Panel configuration for dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelConfig {
    /// Panel title.
    pub title: String,

    /// Panel type (graph, singlestat, table, etc.).
    pub panel_type: PanelType,

    /// Data source queries.
    pub queries: Vec<Query>,

    /// Panel position and size.
    pub grid_pos: GridPos,
}

/// Panel type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelType {
    /// Time-series graph visualization.
    Graph,
    /// Single statistic value display.
    Singlestat,
    /// Tabular data display.
    Table,
    /// Heatmap visualization for density data.
    Heatmap,
    /// Gauge visualization for current values.
    Gauge,
    /// Bar gauge visualization for multiple values.
    BarGauge,
}

/// Query configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    /// Query expression.
    pub expr: String,

    /// Legend format.
    pub legend_format: Option<String>,

    /// Interval.
    pub interval: Option<String>,
}

/// Grid position and size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridPos {
    /// X coordinate on the dashboard grid.
    pub x: u32,
    /// Y coordinate on the dashboard grid.
    pub y: u32,
    /// Width in grid units.
    pub w: u32,
    /// Height in grid units.
    pub h: u32,
}

/// Dashboard builder.
pub struct DashboardBuilder {
    config: DashboardConfig,
    panels: Vec<PanelConfig>,
}

impl DashboardBuilder {
    /// Create a new dashboard builder.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            config: DashboardConfig {
                title: title.into(),
                ..Default::default()
            },
            panels: Vec::new(),
        }
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.config.description = description.into();
        self
    }

    /// Add a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.config.tags.push(tag.into());
        self
    }

    /// Set refresh interval.
    pub fn with_refresh_interval(mut self, seconds: u32) -> Self {
        self.config.refresh_interval = seconds;
        self
    }

    /// Add a panel.
    pub fn add_panel(mut self, panel: PanelConfig) -> Self {
        self.panels.push(panel);
        self
    }

    /// Build the dashboard.
    pub fn build(self) -> Dashboard {
        Dashboard {
            config: self.config,
            panels: self.panels,
        }
    }
}

/// Dashboard definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    /// Dashboard configuration settings.
    pub config: DashboardConfig,
    /// List of panels in the dashboard.
    pub panels: Vec<PanelConfig>,
}

impl Dashboard {
    /// Export dashboard as JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(crate::error::ObservabilityError::Serialization)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_builder() {
        let dashboard = DashboardBuilder::new("Test Dashboard")
            .with_description("Test description")
            .with_tag("test")
            .with_refresh_interval(10)
            .build();

        assert_eq!(dashboard.config.title, "Test Dashboard");
        assert_eq!(dashboard.config.refresh_interval, 10);
    }
}
