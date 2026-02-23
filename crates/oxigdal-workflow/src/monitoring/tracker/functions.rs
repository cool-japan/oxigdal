//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::dag::{TaskNode, WorkflowDag};
use crate::error::{Result, WorkflowError};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::types::{DagVisualizationData, ErrorCategory, ErrorRecord, ErrorSeverity, ErrorTracker, ExecutionHistoryStore, ExecutionTrigger, HistoricalExecution, LayoutInfo, NodeColor, PerformanceCollector, ProgressCalculator, TaskStatus, VisualizationEdge, VisualizationNode, WorkflowStatus, WorkflowTracker};

/// Generate visualization data from a DAG.
pub fn generate_visualization_data(dag: &WorkflowDag) -> Result<DagVisualizationData> {
    let tasks = dag.tasks();
    let mut nodes = Vec::with_capacity(tasks.len());
    let mut edges = Vec::new();
    let mut state_colors = HashMap::new();
    let level_width = 200.0;
    let node_height = 50.0;
    let vertical_spacing = 80.0;
    let mut node_levels: HashMap<String, usize> = HashMap::new();
    for task in &tasks {
        let level = if task.dependencies.is_empty() {
            0
        } else {
            task
                .dependencies
                .iter()
                .filter_map(|dep| node_levels.get(dep))
                .max()
                .unwrap_or(&0) + 1
        };
        node_levels.insert(task.id.clone(), level);
    }
    let mut level_counts: HashMap<usize, usize> = HashMap::new();
    for level in node_levels.values() {
        *level_counts.entry(*level).or_insert(0) += 1;
    }
    let mut level_positions: HashMap<usize, usize> = HashMap::new();
    for task in &tasks {
        let level = node_levels.get(&task.id).copied().unwrap_or(0);
        let pos_in_level = level_positions.entry(level).or_insert(0);
        let nodes_in_level = level_counts.get(&level).copied().unwrap_or(1);
        let x = level as f64 * level_width;
        let y = (*pos_in_level as f64 - (nodes_in_level as f64 - 1.0) / 2.0)
            * vertical_spacing;
        nodes
            .push(VisualizationNode {
                id: task.id.clone(),
                label: task.name.clone(),
                node_type: "task".to_string(),
                x,
                y,
                width: 150.0,
                height: node_height,
                metadata: task.metadata.clone(),
            });
        state_colors
            .insert(
                task.id.clone(),
                NodeColor {
                    fill: "#e3f2fd".to_string(),
                    border: "#1976d2".to_string(),
                    text: "#0d47a1".to_string(),
                },
            );
        for dep in &task.dependencies {
            edges
                .push(VisualizationEdge {
                    source: dep.clone(),
                    target: task.id.clone(),
                    label: None,
                    edge_type: "dependency".to_string(),
                    control_points: Vec::new(),
                });
        }
        *pos_in_level += 1;
    }
    let max_level = node_levels.values().max().copied().unwrap_or(0);
    let max_nodes_per_level = level_counts.values().max().copied().unwrap_or(1);
    let layout = LayoutInfo {
        width: (max_level + 1) as f64 * level_width,
        height: max_nodes_per_level as f64 * vertical_spacing,
        algorithm: "hierarchical".to_string(),
        direction: "LR".to_string(),
        levels: max_level + 1,
    };
    Ok(DagVisualizationData {
        nodes,
        edges,
        layout,
        state_colors,
    })
}
/// Update visualization colors based on execution state.
pub fn update_visualization_colors(
    data: &mut DagVisualizationData,
    task_statuses: &HashMap<String, TaskStatus>,
) {
    for (task_id, status) in task_statuses {
        let color = match status {
            TaskStatus::WaitingDependencies { .. } => {
                NodeColor {
                    fill: "#fff3e0".to_string(),
                    border: "#ff9800".to_string(),
                    text: "#e65100".to_string(),
                }
            }
            TaskStatus::Queued { .. } => {
                NodeColor {
                    fill: "#e8eaf6".to_string(),
                    border: "#3f51b5".to_string(),
                    text: "#1a237e".to_string(),
                }
            }
            TaskStatus::Running { .. } => {
                NodeColor {
                    fill: "#e8f5e9".to_string(),
                    border: "#4caf50".to_string(),
                    text: "#1b5e20".to_string(),
                }
            }
            TaskStatus::Completed { .. } => {
                NodeColor {
                    fill: "#c8e6c9".to_string(),
                    border: "#388e3c".to_string(),
                    text: "#1b5e20".to_string(),
                }
            }
            TaskStatus::Failed { .. } => {
                NodeColor {
                    fill: "#ffcdd2".to_string(),
                    border: "#f44336".to_string(),
                    text: "#b71c1c".to_string(),
                }
            }
            TaskStatus::Skipped { .. } => {
                NodeColor {
                    fill: "#f5f5f5".to_string(),
                    border: "#9e9e9e".to_string(),
                    text: "#616161".to_string(),
                }
            }
            TaskStatus::Cancelled => {
                NodeColor {
                    fill: "#fce4ec".to_string(),
                    border: "#e91e63".to_string(),
                    text: "#880e4f".to_string(),
                }
            }
        };
        data.state_colors.insert(task_id.clone(), color);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_workflow_tracker_creation() {
        let tracker = WorkflowTracker::new("wf1".to_string(), "exec1".to_string());
        let progress = tracker.get_progress().await;
        assert_eq!(progress.workflow_id, "wf1");
        assert_eq!(progress.execution_id, "exec1");
    }
    #[tokio::test]
    async fn test_status_updates() {
        let tracker = WorkflowTracker::new("wf1".to_string(), "exec1".to_string());
        tracker.start().await;
        tracker.mark_running("Processing".to_string()).await;
        let status = tracker.status.read().await.clone();
        assert!(matches!(status, WorkflowStatus::Running { .. }));
        tracker.mark_completed().await;
        let status = tracker.status.read().await.clone();
        assert!(matches!(status, WorkflowStatus::Completed { .. }));
    }
    #[tokio::test]
    async fn test_task_status_tracking() {
        let tracker = WorkflowTracker::new("wf1".to_string(), "exec1".to_string());
        tracker.set_total_tasks(3);
        tracker
            .update_task_status(
                "task1",
                TaskStatus::Running {
                    started_at: Utc::now(),
                    progress_percent: Some(50),
                },
            )
            .await;
        let statuses = tracker.get_task_statuses();
        assert!(matches!(statuses.get("task1"), Some(TaskStatus::Running { .. })));
    }
    #[tokio::test]
    async fn test_progress_calculation() {
        let tracker = WorkflowTracker::new("wf1".to_string(), "exec1".to_string());
        tracker.set_total_tasks(4);
        tracker
            .update_task_status(
                "task1",
                TaskStatus::Completed {
                    duration_ms: 100,
                    output_size_bytes: 1024,
                },
            )
            .await;
        tracker
            .update_task_status(
                "task2",
                TaskStatus::Completed {
                    duration_ms: 200,
                    output_size_bytes: 2048,
                },
            )
            .await;
        let progress = tracker.get_progress().await;
        assert_eq!(progress.tasks_completed, 2);
        assert_eq!(progress.overall_percent, 50);
    }
    #[test]
    fn test_error_tracker() {
        let tracker = ErrorTracker::new();
        let error = ErrorRecord {
            error_id: "err1".to_string(),
            timestamp: Utc::now(),
            workflow_id: "wf1".to_string(),
            execution_id: "exec1".to_string(),
            task_id: Some("task1".to_string()),
            category: ErrorCategory::TaskExecution,
            severity: ErrorSeverity::Error,
            error_code: "E001".to_string(),
            message: "Test error".to_string(),
            details: HashMap::new(),
            stack_trace: None,
            recovered: false,
            recovery_action: None,
        };
        tracker.record_error(error);
        let errors = tracker.get_errors("exec1");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].error_code, "E001");
    }
    #[test]
    fn test_history_store() {
        let store = ExecutionHistoryStore::new();
        let execution = HistoricalExecution {
            execution_id: "exec1".to_string(),
            workflow_id: "wf1".to_string(),
            workflow_name: "Test Workflow".to_string(),
            workflow_version: "1.0.0".to_string(),
            start_time: Utc::now(),
            end_time: None,
            duration: None,
            status: WorkflowStatus::Completed {
                completed_at: Utc::now(),
            },
            tasks: vec![],
            performance: None,
            error_summary: None,
            parameters: HashMap::new(),
            tags: vec!["test".to_string()],
            trigger: ExecutionTrigger::Manual {
                user: None,
            },
            parent_execution_id: None,
        };
        store.store(execution);
        let retrieved = store.get("exec1");
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.as_ref().map(| e | & e.workflow_name), Some(& "Test Workflow"
            .to_string())
        );
    }
    #[test]
    fn test_progress_calculator() {
        let mut calc = ProgressCalculator::new();
        calc.record_sample(10.0);
        std::thread::sleep(std::time::Duration::from_millis(10));
        calc.record_sample(20.0);
        std::thread::sleep(std::time::Duration::from_millis(10));
        calc.record_sample(30.0);
        let eta = calc.estimate_remaining(30.0);
        assert!(eta.is_some());
    }
    #[tokio::test]
    async fn test_performance_collector() {
        let collector = PerformanceCollector::new();
        collector.start_workflow().await;
        collector.task_started("task1").await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        collector.task_completed("task1", 1024).await;
        let throughput = collector.calculate_throughput().await;
        assert!(throughput.total_bytes_processed == 1024);
    }
    #[test]
    fn test_dag_visualization_data() {
        let mut dag = WorkflowDag::new();
        let task1 = TaskNode {
            id: "task1".to_string(),
            name: "Task 1".to_string(),
            dependencies: vec![],
            metadata: HashMap::new(),
        };
        let task2 = TaskNode {
            id: "task2".to_string(),
            name: "Task 2".to_string(),
            dependencies: vec!["task1".to_string()],
            metadata: HashMap::new(),
        };
        dag.add_node(task1).ok();
        dag.add_node(task2).ok();
        let viz_data = generate_visualization_data(&dag);
        assert!(viz_data.is_ok());
        let data = viz_data.ok();
        assert!(data.is_some());
        let data = data.as_ref();
        assert_eq!(data.map(| d | d.nodes.len()), Some(2));
        assert_eq!(data.map(| d | d.edges.len()), Some(1));
    }
    #[test]
    fn test_visualization_color_update() {
        let mut viz_data = DagVisualizationData {
            nodes: vec![
                VisualizationNode { id : "task1".to_string(), label : "Task 1"
                .to_string(), node_type : "task".to_string(), x : 0.0, y : 0.0, width :
                100.0, height : 50.0, metadata : HashMap::new(), }
            ],
            edges: vec![],
            layout: LayoutInfo {
                width: 100.0,
                height: 100.0,
                algorithm: "hierarchical".to_string(),
                direction: "LR".to_string(),
                levels: 1,
            },
            state_colors: HashMap::new(),
        };
        let mut statuses = HashMap::new();
        statuses
            .insert(
                "task1".to_string(),
                TaskStatus::Running {
                    started_at: Utc::now(),
                    progress_percent: Some(50),
                },
            );
        update_visualization_colors(&mut viz_data, &statuses);
        assert!(viz_data.state_colors.contains_key("task1"));
        assert_eq!(viz_data.state_colors["task1"].fill, "#e8f5e9");
    }
}
