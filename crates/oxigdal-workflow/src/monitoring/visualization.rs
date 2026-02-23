//! DAG visualization for workflow monitoring.
//!
//! Supports multiple output formats:
//! - **DOT** (Graphviz) - full-featured graph rendering
//! - **Mermaid** - browser-friendly diagrams
//! - **JSON** - structured data for programmatic consumption
//! - **SVG** - self-contained vector graphics with grid-based layout
//! - **ASCII** - terminal-friendly text rendering
//! - **PlantUML** - UML-style activity diagrams

use crate::dag::{EdgeType, WorkflowDag};
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Graph output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphFormat {
    /// DOT format (Graphviz).
    Dot,
    /// Mermaid format.
    Mermaid,
    /// JSON format.
    Json,
    /// SVG format.
    Svg,
    /// ASCII art format for terminal display.
    Ascii,
    /// PlantUML format.
    PlantUml,
}

/// Visualization configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizationConfig {
    /// Output format.
    pub format: GraphFormat,
    /// Show task status colors.
    pub show_status_colors: bool,
    /// Show task durations.
    pub show_durations: bool,
    /// Show task dependencies.
    pub show_dependencies: bool,
    /// Highlight critical path.
    pub highlight_critical_path: bool,
    /// Graph direction (TB, LR, etc).
    pub direction: String,
    /// Show edge labels (condition text).
    pub show_edge_labels: bool,
    /// Show task descriptions.
    pub show_descriptions: bool,
    /// Show resource requirements.
    pub show_resources: bool,
    /// Custom node colors keyed by task ID.
    pub custom_colors: HashMap<String, String>,
    /// Task status overrides for coloring (keyed by task ID).
    pub task_statuses: HashMap<String, TaskVisualStatus>,
}

/// Visual status for a task node, used for coloring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskVisualStatus {
    /// Task is pending execution.
    Pending,
    /// Task is currently running.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
    /// Task was skipped.
    Skipped,
    /// Task was cancelled.
    Cancelled,
}

impl TaskVisualStatus {
    /// Get the display color for DOT format.
    fn dot_color(&self) -> &'static str {
        match self {
            Self::Pending => "#e0e0e0",
            Self::Running => "#64b5f6",
            Self::Completed => "#81c784",
            Self::Failed => "#e57373",
            Self::Skipped => "#fff176",
            Self::Cancelled => "#bdbdbd",
        }
    }

    /// Get the display color for SVG format.
    fn svg_color(&self) -> &'static str {
        match self {
            Self::Pending => "#e0e0e0",
            Self::Running => "#64b5f6",
            Self::Completed => "#81c784",
            Self::Failed => "#e57373",
            Self::Skipped => "#fff176",
            Self::Cancelled => "#bdbdbd",
        }
    }

    /// Get the Mermaid CSS class name.
    fn mermaid_class(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
        }
    }
}

impl Default for VisualizationConfig {
    fn default() -> Self {
        Self {
            format: GraphFormat::Dot,
            show_status_colors: true,
            show_durations: true,
            show_dependencies: true,
            highlight_critical_path: false,
            direction: "TB".to_string(),
            show_edge_labels: true,
            show_descriptions: false,
            show_resources: false,
            custom_colors: HashMap::new(),
            task_statuses: HashMap::new(),
        }
    }
}

/// DAG visualizer.
pub struct DagVisualizer {
    config: VisualizationConfig,
}

impl DagVisualizer {
    /// Create a new DAG visualizer with default configuration.
    pub fn new() -> Self {
        Self {
            config: VisualizationConfig::default(),
        }
    }

    /// Create a DAG visualizer with custom configuration.
    pub fn with_config(config: VisualizationConfig) -> Self {
        Self { config }
    }

    /// Set the output format.
    pub fn set_format(&mut self, format: GraphFormat) {
        self.config.format = format;
    }

    /// Set task visual statuses for status-aware rendering.
    pub fn set_task_statuses(&mut self, statuses: HashMap<String, TaskVisualStatus>) {
        self.config.task_statuses = statuses;
    }

    /// Set a single task's visual status.
    pub fn set_task_status(&mut self, task_id: &str, status: TaskVisualStatus) {
        self.config
            .task_statuses
            .insert(task_id.to_string(), status);
    }

    /// Visualize a DAG.
    pub fn visualize(&self, dag: &WorkflowDag) -> Result<String> {
        match self.config.format {
            GraphFormat::Dot => self.to_dot(dag),
            GraphFormat::Mermaid => self.to_mermaid(dag),
            GraphFormat::Json => self.to_json(dag),
            GraphFormat::Svg => self.to_svg(dag),
            GraphFormat::Ascii => self.to_ascii(dag),
            GraphFormat::PlantUml => self.to_plantuml(dag),
        }
    }

    /// Get the fill color for a task node.
    fn node_fill_color(&self, task_id: &str) -> String {
        // Custom color takes precedence
        if let Some(color) = self.config.custom_colors.get(task_id) {
            return color.clone();
        }
        // Then status-based color
        if let Some(status) = self.config.task_statuses.get(task_id) {
            return status.dot_color().to_string();
        }
        // Default
        if self.config.show_status_colors {
            "lightblue".to_string()
        } else {
            "white".to_string()
        }
    }

    // ─── DOT FORMAT ────────────────────────────────────────────────

    /// Convert DAG to DOT format (Graphviz).
    fn to_dot(&self, dag: &WorkflowDag) -> Result<String> {
        let mut dot = String::from("digraph workflow {\n");
        dot.push_str(&format!("  rankdir={};\n", self.config.direction));
        dot.push_str("  node [shape=box, style=\"rounded,filled\", fontname=\"Helvetica\"];\n");
        dot.push_str("  edge [fontname=\"Helvetica\", fontsize=10];\n\n");

        // Add nodes
        for node in dag.tasks() {
            let mut label_parts = vec![node.name.clone()];
            if self.config.show_durations {
                label_parts.push(format!("id: {}", node.id));
            }
            if self.config.show_descriptions {
                if let Some(ref desc) = node.description {
                    label_parts.push(desc.clone());
                }
            }
            if self.config.show_resources {
                label_parts.push(format!(
                    "cpu: {:.1}, mem: {}MB",
                    node.resources.cpu_cores, node.resources.memory_mb
                ));
            }

            let label = label_parts.join("\\n");
            let color = self.node_fill_color(&node.id);

            dot.push_str(&format!(
                "  \"{}\" [label=\"{}\", fillcolor=\"{}\"];\n",
                node.id, label, color
            ));
        }

        dot.push('\n');

        // Add edges from DAG
        if self.config.show_dependencies {
            for (from_id, to_id, edge) in dag.edges() {
                let edge_style = match edge.edge_type {
                    EdgeType::Data => "solid",
                    EdgeType::Control => "dashed",
                    EdgeType::Conditional => "dotted",
                };

                let edge_color = match edge.edge_type {
                    EdgeType::Data => "#2196F3",
                    EdgeType::Control => "#757575",
                    EdgeType::Conditional => "#FF9800",
                };

                let mut attrs = vec![
                    format!("style={}", edge_style),
                    format!("color=\"{}\"", edge_color),
                ];

                if self.config.show_edge_labels {
                    if let Some(ref condition) = edge.condition {
                        attrs.push(format!("label=\"{}\"", condition));
                    } else {
                        // Show edge type label for non-control edges
                        match edge.edge_type {
                            EdgeType::Data => attrs.push("label=\"data\"".to_string()),
                            EdgeType::Conditional => {
                                attrs.push("label=\"conditional\"".to_string())
                            }
                            EdgeType::Control => {} // No label for default control edges
                        }
                    }
                }

                dot.push_str(&format!(
                    "  \"{}\" -> \"{}\" [{}];\n",
                    from_id,
                    to_id,
                    attrs.join(", ")
                ));
            }
        }

        dot.push_str("}\n");

        Ok(dot)
    }

    // ─── MERMAID FORMAT ────────────────────────────────────────────

    /// Convert DAG to Mermaid format.
    fn to_mermaid(&self, dag: &WorkflowDag) -> Result<String> {
        let mut mermaid = format!("graph {}\n", self.config.direction);

        // Add nodes with display labels
        for node in dag.tasks() {
            let label = if self.config.show_durations {
                format!("{}<br/>id: {}", node.name, node.id)
            } else {
                node.name.clone()
            };

            // Mermaid node shapes: [] = rectangle, () = rounded, {} = rhombus
            mermaid.push_str(&format!("  {}[\"{}\"]\n", node.id, label));
        }

        mermaid.push('\n');

        // Add edges from DAG with proper styling
        if self.config.show_dependencies {
            for (from_id, to_id, edge) in dag.edges() {
                let arrow = match edge.edge_type {
                    EdgeType::Data => "-->",
                    EdgeType::Control => "-.->",
                    EdgeType::Conditional => "==>",
                };

                if self.config.show_edge_labels {
                    if let Some(ref condition) = edge.condition {
                        mermaid.push_str(&format!(
                            "  {} {}|\"{}\"| {}\n",
                            from_id, arrow, condition, to_id
                        ));
                    } else {
                        match edge.edge_type {
                            EdgeType::Data => {
                                mermaid.push_str(&format!(
                                    "  {} {}|data| {}\n",
                                    from_id, arrow, to_id
                                ));
                            }
                            _ => {
                                mermaid.push_str(&format!("  {} {} {}\n", from_id, arrow, to_id));
                            }
                        }
                    }
                } else {
                    mermaid.push_str(&format!("  {} {} {}\n", from_id, arrow, to_id));
                }
            }
        }

        // Add style classes for task statuses
        if !self.config.task_statuses.is_empty() {
            mermaid.push('\n');
            // Define CSS classes
            mermaid.push_str("  classDef pending fill:#e0e0e0,stroke:#9e9e9e\n");
            mermaid.push_str("  classDef running fill:#64b5f6,stroke:#1976d2\n");
            mermaid.push_str("  classDef completed fill:#81c784,stroke:#388e3c\n");
            mermaid.push_str("  classDef failed fill:#e57373,stroke:#d32f2f\n");
            mermaid.push_str("  classDef skipped fill:#fff176,stroke:#f9a825\n");
            mermaid.push_str("  classDef cancelled fill:#bdbdbd,stroke:#616161\n");

            // Assign classes to nodes
            for (task_id, status) in &self.config.task_statuses {
                mermaid.push_str(&format!("  class {} {}\n", task_id, status.mermaid_class()));
            }
        }

        Ok(mermaid)
    }

    // ─── JSON FORMAT ───────────────────────────────────────────────

    /// Convert DAG to JSON format.
    fn to_json(&self, dag: &WorkflowDag) -> Result<String> {
        #[derive(Serialize)]
        struct JsonEdge {
            from: String,
            to: String,
            edge_type: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            condition: Option<String>,
        }

        #[derive(Serialize)]
        struct JsonNode {
            id: String,
            name: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            description: Option<String>,
            dependencies: Vec<String>,
            dependents: Vec<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            timeout_secs: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            status: Option<String>,
            metadata: HashMap<String, String>,
        }

        #[derive(Serialize)]
        struct JsonSummary {
            node_count: usize,
            edge_count: usize,
            root_count: usize,
            leaf_count: usize,
        }

        #[derive(Serialize)]
        struct JsonGraph {
            nodes: Vec<JsonNode>,
            edges: Vec<JsonEdge>,
            roots: Vec<String>,
            leaves: Vec<String>,
            summary: JsonSummary,
        }

        let nodes: Vec<JsonNode> = dag
            .tasks()
            .iter()
            .map(|node| {
                let status = self
                    .config
                    .task_statuses
                    .get(&node.id)
                    .map(|s| format!("{:?}", s));

                JsonNode {
                    id: node.id.clone(),
                    name: node.name.clone(),
                    description: node.description.clone(),
                    dependencies: dag.get_dependencies(&node.id),
                    dependents: dag.get_dependents(&node.id),
                    timeout_secs: node.timeout_secs,
                    status,
                    metadata: node.metadata.clone(),
                }
            })
            .collect();

        let edges: Vec<JsonEdge> = dag
            .edges()
            .iter()
            .map(|(from_id, to_id, edge)| {
                let edge_type_str = match edge.edge_type {
                    EdgeType::Data => "data",
                    EdgeType::Control => "control",
                    EdgeType::Conditional => "conditional",
                };
                JsonEdge {
                    from: from_id.to_string(),
                    to: to_id.to_string(),
                    edge_type: edge_type_str.to_string(),
                    condition: edge.condition.clone(),
                }
            })
            .collect();

        let roots: Vec<String> = dag.root_tasks().iter().map(|t| t.id.clone()).collect();
        let leaves: Vec<String> = dag.leaf_tasks().iter().map(|t| t.id.clone()).collect();

        let dag_summary = dag.summary();
        let summary = JsonSummary {
            node_count: dag_summary.node_count,
            edge_count: dag_summary.edge_count,
            root_count: dag_summary.root_count,
            leaf_count: dag_summary.leaf_count,
        };

        let graph = JsonGraph {
            nodes,
            edges,
            roots,
            leaves,
            summary,
        };

        serde_json::to_string_pretty(&graph)
            .map_err(|e| crate::error::WorkflowError::monitoring(format!("JSON error: {}", e)))
    }

    // ─── SVG FORMAT ────────────────────────────────────────────────

    /// Convert DAG to SVG format with a grid-based layout.
    ///
    /// Uses topological layering to position nodes in rows. Within each layer
    /// nodes are distributed horizontally with even spacing.
    fn to_svg(&self, dag: &WorkflowDag) -> Result<String> {
        let tasks = dag.tasks();
        if tasks.is_empty() {
            return Ok(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
  <text x="100" y="50" text-anchor="middle" font-family="Helvetica" font-size="14">Empty DAG</text>
</svg>"#
                    .to_string(),
            );
        }

        // Compute execution plan (layers) for layout
        let layers = crate::dag::create_execution_plan(dag)?;

        // Layout constants
        let node_width: f64 = 160.0;
        let node_height: f64 = 50.0;
        let layer_gap: f64 = 80.0;
        let node_gap: f64 = 40.0;
        let padding: f64 = 40.0;

        // Calculate positions for each node
        let mut positions: HashMap<String, (f64, f64)> = HashMap::new();

        let max_layer_width = layers.iter().map(|layer| layer.len()).max().unwrap_or(1);

        let canvas_width =
            max_layer_width as f64 * (node_width + node_gap) - node_gap + 2.0 * padding;

        for (layer_idx, layer) in layers.iter().enumerate() {
            let layer_width = layer.len() as f64 * (node_width + node_gap) - node_gap;
            let x_offset = (canvas_width - layer_width) / 2.0;

            for (node_idx, task_id) in layer.iter().enumerate() {
                let x = x_offset + node_idx as f64 * (node_width + node_gap);
                let y = padding + layer_idx as f64 * (node_height + layer_gap);
                positions.insert(task_id.clone(), (x, y));
            }
        }

        let canvas_height =
            layers.len() as f64 * (node_height + layer_gap) - layer_gap + 2.0 * padding;

        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
            canvas_width, canvas_height, canvas_width, canvas_height
        );
        svg.push('\n');

        // Defs for arrow markers
        svg.push_str(r##"  <defs>
    <marker id="arrowhead" markerWidth="10" markerHeight="7" refX="10" refY="3.5" orient="auto">
      <polygon points="0 0, 10 3.5, 0 7" fill="#757575"/>
    </marker>
    <marker id="arrowhead-data" markerWidth="10" markerHeight="7" refX="10" refY="3.5" orient="auto">
      <polygon points="0 0, 10 3.5, 0 7" fill="#2196F3"/>
    </marker>
    <marker id="arrowhead-cond" markerWidth="10" markerHeight="7" refX="10" refY="3.5" orient="auto">
      <polygon points="0 0, 10 3.5, 0 7" fill="#FF9800"/>
    </marker>
  </defs>
"##);

        // Background
        svg.push_str(&format!(
            "  <rect width=\"{}\" height=\"{}\" fill=\"{}\" rx=\"8\"/>\n",
            canvas_width, canvas_height, "#fafafa"
        ));

        // Draw edges first (so nodes appear on top)
        if self.config.show_dependencies {
            for (from_id, to_id, edge) in dag.edges() {
                if let (Some(&(fx, fy)), Some(&(tx, ty))) =
                    (positions.get(from_id), positions.get(to_id))
                {
                    let x1 = fx + node_width / 2.0;
                    let y1 = fy + node_height;
                    let x2 = tx + node_width / 2.0;
                    let y2 = ty;

                    let (stroke, dash, marker) = match edge.edge_type {
                        EdgeType::Data => ("#2196F3", "", "url(#arrowhead-data)"),
                        EdgeType::Control => {
                            ("#757575", "stroke-dasharray=\"6,3\"", "url(#arrowhead)")
                        }
                        EdgeType::Conditional => (
                            "#FF9800",
                            "stroke-dasharray=\"3,3\"",
                            "url(#arrowhead-cond)",
                        ),
                    };

                    // Use a cubic bezier for smoother curves
                    let mid_y = (y1 + y2) / 2.0;
                    svg.push_str(&format!(
                        "  <path d=\"M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}\" \
                         fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\" {} marker-end=\"{}\"/>\n",
                        x1, y1, x1, mid_y, x2, mid_y, x2, y2, stroke, dash, marker
                    ));

                    // Edge label
                    if self.config.show_edge_labels {
                        if let Some(ref condition) = edge.condition {
                            let label_x = (x1 + x2) / 2.0;
                            let label_y = mid_y - 6.0;
                            svg.push_str(&format!(
                                "  <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                                 font-family=\"Helvetica\" font-size=\"9\" fill=\"{}\">{}</text>\n",
                                label_x,
                                label_y,
                                stroke,
                                html_escape(condition)
                            ));
                        }
                    }
                }
            }
        }

        // Draw nodes
        for node in dag.tasks() {
            if let Some(&(x, y)) = positions.get(&node.id) {
                let fill = if let Some(color) = self.config.custom_colors.get(&node.id) {
                    color.clone()
                } else if let Some(status) = self.config.task_statuses.get(&node.id) {
                    status.svg_color().to_string()
                } else {
                    "#e3f2fd".to_string()
                };

                let stroke_color = "#90caf9";
                let text_color = "#212121";

                // Node rectangle
                svg.push_str(&format!(
                    "  <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" \
                     rx=\"6\" ry=\"6\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.5\"/>\n",
                    x, y, node_width, node_height, fill, stroke_color
                ));

                // Node label
                let label_x = x + node_width / 2.0;
                let label_y = y + node_height / 2.0 + 5.0;
                svg.push_str(&format!(
                    "  <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" \
                     font-family=\"Helvetica\" font-size=\"12\" font-weight=\"500\" \
                     fill=\"{}\">{}</text>\n",
                    label_x,
                    label_y,
                    text_color,
                    html_escape(&node.name)
                ));
            }
        }

        svg.push_str("</svg>\n");

        Ok(svg)
    }

    // ─── ASCII FORMAT ──────────────────────────────────────────────

    /// Convert DAG to ASCII art format for terminal display.
    fn to_ascii(&self, dag: &WorkflowDag) -> Result<String> {
        let tasks = dag.tasks();
        if tasks.is_empty() {
            return Ok("(empty DAG)\n".to_string());
        }

        // Use execution plan layers for layout
        let layers = crate::dag::create_execution_plan(dag)?;
        let mut output = String::new();

        // Header
        output.push_str(&format!(
            "Workflow DAG ({} tasks, {} edges)\n",
            dag.task_count(),
            dag.dependency_count()
        ));
        output.push_str(&"=".repeat(50));
        output.push('\n');

        for (layer_idx, layer) in layers.iter().enumerate() {
            if layer_idx > 0 {
                // Draw connecting arrows from previous layer
                self.ascii_draw_connectors(&mut output, dag, &layers[layer_idx - 1], layer);
            }

            // Draw nodes in this layer
            self.ascii_draw_layer(&mut output, dag, layer, layer_idx);
        }

        // Footer: summary
        output.push('\n');
        output.push_str(&"-".repeat(50));
        output.push('\n');

        let summary = dag.summary();
        output.push_str(&format!(
            "Roots: {} | Leaves: {} | Max fan-in: {} | Max fan-out: {}\n",
            summary.root_count, summary.leaf_count, summary.max_in_degree, summary.max_out_degree
        ));

        if summary.data_edge_count > 0 || summary.conditional_edge_count > 0 {
            output.push_str(&format!(
                "Edge types: {} data, {} control, {} conditional\n",
                summary.data_edge_count, summary.control_edge_count, summary.conditional_edge_count
            ));
        }

        Ok(output)
    }

    /// Draw a layer of nodes in ASCII.
    fn ascii_draw_layer(
        &self,
        output: &mut String,
        _dag: &WorkflowDag,
        layer: &[String],
        layer_idx: usize,
    ) {
        // Determine the widest label
        let labels: Vec<String> = layer
            .iter()
            .map(|id| {
                let status_marker = self
                    .config
                    .task_statuses
                    .get(id)
                    .map(|s| match s {
                        TaskVisualStatus::Pending => " [.]",
                        TaskVisualStatus::Running => " [>]",
                        TaskVisualStatus::Completed => " [+]",
                        TaskVisualStatus::Failed => " [X]",
                        TaskVisualStatus::Skipped => " [-]",
                        TaskVisualStatus::Cancelled => " [!]",
                    })
                    .unwrap_or("");
                format!("{}{}", id, status_marker)
            })
            .collect();

        let max_label_width = labels.iter().map(|l| l.len()).max().unwrap_or(0);
        let box_width = max_label_width + 4; // padding

        // Layer header
        output.push_str(&format!("Layer {}:\n", layer_idx));

        // Draw boxes side by side
        let top_border: Vec<String> = labels
            .iter()
            .map(|_| format!("+{}+", "-".repeat(box_width)))
            .collect();
        output.push_str(&format!("  {}\n", top_border.join("  ")));

        let content: Vec<String> = labels
            .iter()
            .map(|label| {
                let pad = box_width - label.len();
                let left_pad = pad / 2;
                let right_pad = pad - left_pad;
                format!(
                    "|{}{}{}|",
                    " ".repeat(left_pad),
                    label,
                    " ".repeat(right_pad)
                )
            })
            .collect();
        output.push_str(&format!("  {}\n", content.join("  ")));

        let bottom_border: Vec<String> = labels
            .iter()
            .map(|_| format!("+{}+", "-".repeat(box_width)))
            .collect();
        output.push_str(&format!("  {}\n", bottom_border.join("  ")));
    }

    /// Draw connectors between layers in ASCII.
    fn ascii_draw_connectors(
        &self,
        output: &mut String,
        dag: &WorkflowDag,
        prev_layer: &[String],
        current_layer: &[String],
    ) {
        // Find edges that connect prev_layer to current_layer
        let mut has_connections = false;
        for to_id in current_layer {
            for from_id in prev_layer {
                if dag.has_dependency(from_id, to_id) {
                    has_connections = true;
                    break;
                }
            }
            if has_connections {
                break;
            }
        }

        if has_connections {
            // Draw arrow lines
            let mut connector_lines = Vec::new();
            for to_id in current_layer {
                let deps_in_prev: Vec<&str> = prev_layer
                    .iter()
                    .filter(|from_id| dag.has_dependency(from_id, to_id))
                    .map(|s| s.as_str())
                    .collect();

                if !deps_in_prev.is_empty() {
                    let edge_info: Vec<String> = deps_in_prev
                        .iter()
                        .map(|from_id| {
                            let edge_type = dag
                                .get_edge_between(from_id, to_id)
                                .map(|e| match e.edge_type {
                                    EdgeType::Data => "~~>",
                                    EdgeType::Control => "-->",
                                    EdgeType::Conditional => "==>",
                                })
                                .unwrap_or("-->");
                            format!("  {} {} {}", from_id, edge_type, to_id)
                        })
                        .collect();
                    connector_lines.extend(edge_info);
                }
            }

            for line in connector_lines {
                output.push_str(&format!("{}\n", line));
            }
        }
    }

    // ─── PLANTUML FORMAT ───────────────────────────────────────────

    /// Convert DAG to PlantUML format.
    fn to_plantuml(&self, dag: &WorkflowDag) -> Result<String> {
        let mut uml = String::from("@startuml\n");
        uml.push_str("!theme plain\n");

        // Direction
        let direction = match self.config.direction.as_str() {
            "LR" => "left to right direction\n",
            _ => "top to bottom direction\n",
        };
        uml.push_str(direction);
        uml.push('\n');

        // Skinparam for styling
        uml.push_str("skinparam activity {\n");
        uml.push_str("  BackgroundColor #e3f2fd\n");
        uml.push_str("  BorderColor #90caf9\n");
        uml.push_str("  FontName Helvetica\n");
        uml.push_str("}\n\n");

        // Define nodes as rectangles
        for node in dag.tasks() {
            let color = if let Some(status) = self.config.task_statuses.get(&node.id) {
                status.dot_color().to_string()
            } else {
                "#e3f2fd".to_string()
            };

            let label = if self.config.show_descriptions {
                if let Some(ref desc) = node.description {
                    format!("{}\\n{}", node.name, desc)
                } else {
                    node.name.clone()
                }
            } else {
                node.name.clone()
            };

            uml.push_str(&format!(
                "rectangle \"{}\" as {} {}\n",
                label, node.id, color
            ));
        }

        uml.push('\n');

        // Add edges
        if self.config.show_dependencies {
            for (from_id, to_id, edge) in dag.edges() {
                let arrow = match edge.edge_type {
                    EdgeType::Data => "-->",
                    EdgeType::Control => "..>",
                    EdgeType::Conditional => "-[#FF9800]->",
                };

                if self.config.show_edge_labels {
                    if let Some(ref condition) = edge.condition {
                        uml.push_str(&format!(
                            "{} {} {} : {}\n",
                            from_id, arrow, to_id, condition
                        ));
                    } else {
                        match edge.edge_type {
                            EdgeType::Data => {
                                uml.push_str(&format!("{} {} {} : data\n", from_id, arrow, to_id));
                            }
                            _ => {
                                uml.push_str(&format!("{} {} {}\n", from_id, arrow, to_id));
                            }
                        }
                    }
                } else {
                    uml.push_str(&format!("{} {} {}\n", from_id, arrow, to_id));
                }
            }
        }

        uml.push_str("\n@enduml\n");

        Ok(uml)
    }

    // ─── TIMELINE & GANTT ──────────────────────────────────────────

    /// Generate execution timeline visualization.
    pub fn visualize_timeline(
        &self,
        execution_history: &[crate::monitoring::TaskExecutionRecord],
    ) -> Result<String> {
        let mut timeline = String::from("# Execution Timeline\n\n");

        for task in execution_history {
            let duration = task
                .duration
                .map(|d| format!("{:.2}s", d.as_secs_f64()))
                .unwrap_or_else(|| "N/A".to_string());

            let status = format!("{:?}", task.status);

            timeline.push_str(&format!(
                "- {} [{}] Duration: {} Status: {}\n",
                task.task_name, task.task_id, duration, status
            ));
        }

        Ok(timeline)
    }

    /// Generate Gantt chart data.
    pub fn generate_gantt_data(
        &self,
        execution_history: &[crate::monitoring::TaskExecutionRecord],
    ) -> Result<Vec<GanttTask>> {
        let mut tasks = Vec::new();

        for (idx, task) in execution_history.iter().enumerate() {
            let start_ms = task.start_time.timestamp_millis();
            let end_ms = task
                .end_time
                .map(|t| t.timestamp_millis())
                .unwrap_or(start_ms);

            tasks.push(GanttTask {
                id: task.task_id.clone(),
                name: task.task_name.clone(),
                start: start_ms,
                end: end_ms,
                duration_ms: (end_ms - start_ms) as u64,
                row: idx,
                status: format!("{:?}", task.status),
            });
        }

        Ok(tasks)
    }

    /// Generate Mermaid Gantt chart from execution history.
    pub fn generate_mermaid_gantt(
        &self,
        execution_history: &[crate::monitoring::TaskExecutionRecord],
    ) -> Result<String> {
        let mut gantt = String::from("gantt\n");
        gantt.push_str("  title Workflow Execution Timeline\n");
        gantt.push_str("  dateFormat x\n");
        gantt.push_str("  axisFormat %H:%M:%S\n\n");

        if execution_history.is_empty() {
            return Ok(gantt);
        }

        // Find the earliest start time as the baseline
        let base_time = execution_history
            .iter()
            .map(|t| t.start_time.timestamp_millis())
            .min()
            .unwrap_or(0);

        for task in execution_history {
            let start_offset = task.start_time.timestamp_millis() - base_time;
            let duration_ms = task.duration.map(|d| d.as_millis() as i64).unwrap_or(1000);

            let status_tag = match task.status {
                crate::monitoring::TaskExecutionStatus::Success => "",
                crate::monitoring::TaskExecutionStatus::Failed => "crit, ",
                crate::monitoring::TaskExecutionStatus::Running => "active, ",
                _ => "",
            };

            gantt.push_str(&format!(
                "  {} : {}{}, {}ms\n",
                task.task_name, status_tag, start_offset, duration_ms
            ));
        }

        Ok(gantt)
    }

    // ─── HTML ──────────────────────────────────────────────────────

    /// Generate HTML visualization with embedded Mermaid diagram.
    pub fn generate_html_visualization(&self, dag: &WorkflowDag) -> Result<String> {
        let dot = self.to_dot(dag)?;

        // Also generate Mermaid for interactive viewing
        let mermaid_viz = {
            let viz = self.clone_with_format(GraphFormat::Mermaid);
            viz.visualize(dag)?
        };

        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <title>Workflow Visualization</title>
    <meta charset="utf-8"/>
    <script src="https://cdn.jsdelivr.net/npm/mermaid/dist/mermaid.min.js"></script>
    <style>
        body {{
            font-family: 'Helvetica Neue', Arial, sans-serif;
            margin: 20px;
            background: #fafafa;
            color: #212121;
        }}
        h1, h2 {{
            color: #1565c0;
        }}
        .container {{
            max-width: 1200px;
            margin: 0 auto;
        }}
        pre {{
            background: #f5f5f5;
            padding: 15px;
            border-radius: 8px;
            overflow-x: auto;
            border: 1px solid #e0e0e0;
        }}
        .mermaid {{
            text-align: center;
            padding: 20px;
            background: white;
            border-radius: 8px;
            border: 1px solid #e0e0e0;
            margin: 15px 0;
        }}
        .tab-container {{
            display: flex;
            gap: 0;
            border-bottom: 2px solid #1565c0;
            margin-top: 20px;
        }}
        .tab {{
            padding: 10px 20px;
            cursor: pointer;
            background: #e3f2fd;
            border: 1px solid #90caf9;
            border-bottom: none;
            border-radius: 8px 8px 0 0;
        }}
        .tab.active {{
            background: white;
            font-weight: bold;
        }}
        .tab-content {{
            display: none;
            padding: 15px;
            background: white;
            border: 1px solid #e0e0e0;
            border-top: none;
        }}
        .tab-content.active {{
            display: block;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Workflow DAG</h1>

        <div class="tab-container">
            <div class="tab active" onclick="showTab('mermaid')">Mermaid</div>
            <div class="tab" onclick="showTab('dot')">DOT (Graphviz)</div>
        </div>

        <div id="mermaid" class="tab-content active">
            <div class="mermaid">
{}
            </div>
        </div>

        <div id="dot" class="tab-content">
            <pre>{}</pre>
            <p>Render this DOT graph at <a href="https://dreampuf.github.io/GraphvizOnline/" target="_blank">Graphviz Online</a></p>
        </div>
    </div>

    <script>
        mermaid.initialize({{ startOnLoad: true, theme: 'default' }});

        function showTab(tabId) {{
            document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
            document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
            document.getElementById(tabId).classList.add('active');
            event.target.classList.add('active');
        }}
    </script>
</body>
</html>"#,
            html_escape(&mermaid_viz),
            html_escape(&dot)
        );

        Ok(html)
    }

    /// Create a copy of this visualizer with a different format.
    fn clone_with_format(&self, format: GraphFormat) -> Self {
        let mut config = self.config.clone();
        config.format = format;
        Self { config }
    }
}

impl Default for DagVisualizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Gantt chart task representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GanttTask {
    /// Task ID.
    pub id: String,
    /// Task name.
    pub name: String,
    /// Start time (milliseconds since epoch).
    pub start: i64,
    /// End time (milliseconds since epoch).
    pub end: i64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Row number for display.
    pub row: usize,
    /// Task status.
    pub status: String,
}

/// Escape HTML special characters.
fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Workflow execution visualization data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionVisualization {
    /// Workflow ID.
    pub workflow_id: String,
    /// Execution ID.
    pub execution_id: String,
    /// DAG structure.
    pub dag_structure: String,
    /// Timeline data.
    pub timeline: Vec<GanttTask>,
    /// Statistics.
    pub statistics: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{EdgeType, ResourceRequirements, RetryPolicy, TaskEdge, TaskNode};

    fn create_test_task(id: &str, name: &str) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            config: serde_json::json!({}),
            retry: RetryPolicy::default(),
            timeout_secs: Some(60),
            resources: ResourceRequirements::default(),
            metadata: HashMap::new(),
        }
    }

    fn create_test_dag() -> WorkflowDag {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("ingest", "Data Ingestion"))
            .expect("Failed to add task");
        dag.add_task(create_test_task("validate", "Validation"))
            .expect("Failed to add task");
        dag.add_task(create_test_task("transform", "Transform"))
            .expect("Failed to add task");
        dag.add_task(create_test_task("output", "Output"))
            .expect("Failed to add task");

        dag.add_dependency(
            "ingest",
            "validate",
            TaskEdge {
                edge_type: EdgeType::Data,
                condition: None,
            },
        )
        .expect("Failed to add dependency");
        dag.add_dependency(
            "validate",
            "transform",
            TaskEdge {
                edge_type: EdgeType::Control,
                condition: None,
            },
        )
        .expect("Failed to add dependency");
        dag.add_dependency(
            "validate",
            "output",
            TaskEdge {
                edge_type: EdgeType::Conditional,
                condition: Some("skip_transform".to_string()),
            },
        )
        .expect("Failed to add dependency");
        dag.add_dependency(
            "transform",
            "output",
            TaskEdge {
                edge_type: EdgeType::Data,
                condition: None,
            },
        )
        .expect("Failed to add dependency");

        dag
    }

    #[test]
    fn test_visualizer_creation() {
        let visualizer = DagVisualizer::new();
        assert_eq!(visualizer.config.format, GraphFormat::Dot);
    }

    #[test]
    fn test_dot_generation() {
        let visualizer = DagVisualizer::new();
        let mut dag = WorkflowDag::new();

        dag.add_task(create_test_task("task1", "Task 1"))
            .expect("Failed to add task");
        dag.add_task(create_test_task("task2", "Task 2"))
            .expect("Failed to add task");
        dag.add_dependency("task1", "task2", TaskEdge::default())
            .expect("Failed to add dependency");

        let dot = visualizer.visualize(&dag).expect("Failed to generate DOT");
        assert!(dot.contains("digraph workflow"));
        assert!(dot.contains("task1"));
        assert!(dot.contains("task2"));
        assert!(dot.contains("->"));
    }

    #[test]
    fn test_dot_with_edge_types() {
        let visualizer = DagVisualizer::new();
        let dag = create_test_dag();

        let dot = visualizer.visualize(&dag).expect("Failed to generate DOT");
        assert!(dot.contains("style=solid")); // Data edges
        assert!(dot.contains("style=dashed")); // Control edges
        assert!(dot.contains("style=dotted")); // Conditional edges
        assert!(dot.contains("skip_transform")); // Condition label
    }

    #[test]
    fn test_mermaid_generation() {
        let mut visualizer = DagVisualizer::new();
        visualizer.set_format(GraphFormat::Mermaid);

        let dag = create_test_dag();
        let mermaid = visualizer
            .visualize(&dag)
            .expect("Failed to generate Mermaid");

        assert!(mermaid.contains("graph"));
        assert!(mermaid.contains("ingest"));
        assert!(mermaid.contains("validate"));
        assert!(mermaid.contains("transform"));
        assert!(mermaid.contains("output"));
        // Mermaid should contain edge arrows
        assert!(mermaid.contains("-->")); // Data edges
        assert!(mermaid.contains("-.->") || mermaid.contains("==>"));
    }

    #[test]
    fn test_mermaid_with_statuses() {
        let mut visualizer = DagVisualizer::new();
        visualizer.set_format(GraphFormat::Mermaid);
        visualizer.set_task_status("ingest", TaskVisualStatus::Completed);
        visualizer.set_task_status("validate", TaskVisualStatus::Running);

        let dag = create_test_dag();
        let mermaid = visualizer
            .visualize(&dag)
            .expect("Failed to generate Mermaid");

        assert!(mermaid.contains("classDef completed"));
        assert!(mermaid.contains("classDef running"));
        assert!(mermaid.contains("class ingest completed"));
        assert!(mermaid.contains("class validate running"));
    }

    #[test]
    fn test_json_generation() {
        let mut visualizer = DagVisualizer::new();
        visualizer.set_format(GraphFormat::Json);

        let dag = create_test_dag();

        let json = visualizer.visualize(&dag).expect("Failed to generate JSON");

        // Parse JSON to validate structure
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("Invalid JSON output");

        // Nodes
        let nodes = parsed["nodes"].as_array().expect("nodes should be array");
        assert_eq!(nodes.len(), 4);

        // Edges
        let edges = parsed["edges"].as_array().expect("edges should be array");
        assert_eq!(edges.len(), 4);

        // Check that dependencies are populated
        let validate_node = nodes
            .iter()
            .find(|n| n["id"] == "validate")
            .expect("validate node should exist");
        let validate_deps = validate_node["dependencies"]
            .as_array()
            .expect("dependencies should be array");
        assert_eq!(validate_deps.len(), 1);
        assert_eq!(validate_deps[0], "ingest");

        // Check edge types
        let data_edge = edges
            .iter()
            .find(|e| e["from"] == "ingest" && e["to"] == "validate")
            .expect("data edge should exist");
        assert_eq!(data_edge["edge_type"], "data");

        // Check roots and leaves
        let roots = parsed["roots"].as_array().expect("roots should be array");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0], "ingest");

        let leaves = parsed["leaves"].as_array().expect("leaves should be array");
        assert_eq!(leaves.len(), 1);
        assert_eq!(leaves[0], "output");

        // Summary
        assert_eq!(parsed["summary"]["node_count"], 4);
        assert_eq!(parsed["summary"]["edge_count"], 4);
    }

    #[test]
    fn test_svg_generation() {
        let mut visualizer = DagVisualizer::new();
        visualizer.set_format(GraphFormat::Svg);

        let dag = create_test_dag();

        let svg = visualizer.visualize(&dag).expect("Failed to generate SVG");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
        assert!(svg.contains("<rect")); // Node rectangles
        assert!(svg.contains("<text")); // Node labels
        assert!(svg.contains("<path")); // Edge paths
        assert!(svg.contains("arrowhead")); // Arrow markers
        assert!(svg.contains("Data Ingestion")); // Task name
    }

    #[test]
    fn test_svg_empty_dag() {
        let visualizer = DagVisualizer::with_config(VisualizationConfig {
            format: GraphFormat::Svg,
            ..Default::default()
        });

        let dag = WorkflowDag::new();
        let svg = visualizer.visualize(&dag).expect("Failed to generate SVG");
        assert!(svg.contains("Empty DAG"));
    }

    #[test]
    fn test_ascii_generation() {
        let mut visualizer = DagVisualizer::new();
        visualizer.set_format(GraphFormat::Ascii);

        let dag = create_test_dag();

        let ascii = visualizer
            .visualize(&dag)
            .expect("Failed to generate ASCII");

        assert!(ascii.contains("Workflow DAG"));
        assert!(ascii.contains("Layer 0"));
        assert!(ascii.contains("ingest"));
        assert!(ascii.contains("Roots:"));
        assert!(ascii.contains("Leaves:"));
    }

    #[test]
    fn test_ascii_with_statuses() {
        let mut visualizer = DagVisualizer::new();
        visualizer.set_format(GraphFormat::Ascii);
        visualizer.set_task_status("ingest", TaskVisualStatus::Completed);
        visualizer.set_task_status("validate", TaskVisualStatus::Failed);

        let dag = create_test_dag();
        let ascii = visualizer
            .visualize(&dag)
            .expect("Failed to generate ASCII");

        assert!(ascii.contains("[+]")); // Completed
        assert!(ascii.contains("[X]")); // Failed
    }

    #[test]
    fn test_plantuml_generation() {
        let mut visualizer = DagVisualizer::new();
        visualizer.set_format(GraphFormat::PlantUml);

        let dag = create_test_dag();

        let uml = visualizer
            .visualize(&dag)
            .expect("Failed to generate PlantUML");

        assert!(uml.contains("@startuml"));
        assert!(uml.contains("@enduml"));
        assert!(uml.contains("rectangle"));
        assert!(uml.contains("ingest"));
        assert!(uml.contains("-->")); // Data edge
        assert!(uml.contains("..>")); // Control edge
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<test>"), "&lt;test&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
    }

    #[test]
    fn test_html_generation() {
        let visualizer = DagVisualizer::new();
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("task1", "Task 1"))
            .expect("Failed to add task");

        let html = visualizer
            .generate_html_visualization(&dag)
            .expect("Failed to generate HTML");

        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("mermaid"));
        assert!(html.contains("digraph"));
    }

    #[test]
    fn test_format_switching() {
        let _visualizer = DagVisualizer::new();
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1"))
            .expect("Failed to add task");
        dag.add_task(create_test_task("t2", "Task 2"))
            .expect("Failed to add task");
        dag.add_dependency("t1", "t2", TaskEdge::default())
            .expect("Failed to add dependency");

        // Test all formats produce non-empty output
        for format in &[
            GraphFormat::Dot,
            GraphFormat::Mermaid,
            GraphFormat::Json,
            GraphFormat::Svg,
            GraphFormat::Ascii,
            GraphFormat::PlantUml,
        ] {
            let vis = DagVisualizer::with_config(VisualizationConfig {
                format: *format,
                ..Default::default()
            });
            let result = vis.visualize(&dag);
            assert!(
                result.is_ok(),
                "Format {:?} failed to produce output",
                format
            );
            let output = result.expect("Failed to visualize");
            assert!(
                !output.is_empty(),
                "Format {:?} produced empty output",
                format
            );
        }
    }

    #[test]
    fn test_custom_colors() {
        let visualizer = DagVisualizer::with_config(VisualizationConfig {
            format: GraphFormat::Dot,
            custom_colors: {
                let mut m = HashMap::new();
                m.insert("task1".to_string(), "#ff0000".to_string());
                m
            },
            ..Default::default()
        });

        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("task1", "Task 1"))
            .expect("Failed to add task");

        let dot = visualizer.visualize(&dag).expect("Failed to generate DOT");
        assert!(dot.contains("#ff0000"));
    }

    #[test]
    fn test_ascii_empty_dag() {
        let mut visualizer = DagVisualizer::new();
        visualizer.set_format(GraphFormat::Ascii);

        let dag = WorkflowDag::new();
        let ascii = visualizer
            .visualize(&dag)
            .expect("Failed to generate ASCII");
        assert!(ascii.contains("empty DAG"));
    }

    #[test]
    fn test_json_with_statuses() {
        let mut visualizer = DagVisualizer::new();
        visualizer.set_format(GraphFormat::Json);
        visualizer.set_task_status("task1", TaskVisualStatus::Completed);

        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("task1", "Task 1"))
            .expect("Failed to add task");

        let json = visualizer.visualize(&dag).expect("Failed to generate JSON");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("Invalid JSON");

        let node = &parsed["nodes"][0];
        assert!(node["status"].is_string());
    }
}
