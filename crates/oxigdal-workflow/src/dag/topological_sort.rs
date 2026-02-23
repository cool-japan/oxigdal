//! Topological sorting for DAG execution ordering.

use crate::dag::graph::WorkflowDag;
use crate::error::{DagError, Result};
use petgraph::Direction;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;

use std::collections::{HashMap, HashSet, VecDeque};

/// Result of topological sort - ordered list of task IDs.
pub type TopologicalOrder = Vec<String>;

/// Execution level - tasks that can be executed in parallel.
pub type ExecutionLevel = Vec<String>;

/// Layered execution plan - groups of tasks that can execute in parallel.
pub type ExecutionPlan = Vec<ExecutionLevel>;

/// Perform topological sort on the DAG using Kahn's algorithm.
pub fn topological_sort(dag: &WorkflowDag) -> Result<TopologicalOrder> {
    let mut in_degree: HashMap<NodeIndex, usize> = HashMap::new();
    let mut queue = VecDeque::new();
    let mut result = Vec::new();

    // Calculate in-degree for all nodes
    for node_idx in dag.graph.node_indices() {
        let degree = dag
            .graph
            .edges_directed(node_idx, Direction::Incoming)
            .count();
        in_degree.insert(node_idx, degree);

        // Add nodes with no incoming edges to queue
        if degree == 0 {
            queue.push_back(node_idx);
        }
    }

    // Process nodes in topological order
    while let Some(node_idx) = queue.pop_front() {
        if let Some(task) = dag.graph.node_weight(node_idx) {
            result.push(task.id.clone());
        }

        // Reduce in-degree of neighbors
        for neighbor in dag.graph.neighbors(node_idx) {
            if let Some(degree) = in_degree.get_mut(&neighbor) {
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(neighbor);
                }
            }
        }
    }

    // Check if all nodes were processed (no cycles)
    if result.len() != dag.task_count() {
        return Err(DagError::cycle("Cycle detected during topological sort").into());
    }

    Ok(result)
}

/// Create an execution plan with parallelizable task groups.
pub fn create_execution_plan(dag: &WorkflowDag) -> Result<ExecutionPlan> {
    let mut in_degree: HashMap<NodeIndex, usize> = HashMap::new();
    let mut execution_plan = Vec::new();

    // Calculate in-degree for all nodes
    for node_idx in dag.graph.node_indices() {
        let degree = dag
            .graph
            .edges_directed(node_idx, Direction::Incoming)
            .count();
        in_degree.insert(node_idx, degree);
    }

    let mut processed_count = 0;
    let total_tasks = dag.task_count();

    // Process tasks level by level
    while processed_count < total_tasks {
        let mut current_level = Vec::new();

        // Find all tasks with in-degree 0
        for (&node_idx, &degree) in &in_degree {
            if degree == 0 {
                if let Some(task) = dag.graph.node_weight(node_idx) {
                    current_level.push(task.id.clone());
                }
            }
        }

        if current_level.is_empty() {
            return Err(DagError::cycle("Cycle detected in execution plan").into());
        }

        // Reduce in-degree of neighbors
        for task_id in &current_level {
            if let Some(&node_idx) = dag.task_map.get(task_id) {
                // Mark as processed
                in_degree.insert(node_idx, usize::MAX);

                // Update neighbors
                for neighbor in dag.graph.neighbors(node_idx) {
                    if let Some(degree) = in_degree.get_mut(&neighbor) {
                        if *degree != usize::MAX {
                            *degree = degree.saturating_sub(1);
                        }
                    }
                }
            }
        }

        processed_count += current_level.len();
        execution_plan.push(current_level);
    }

    Ok(execution_plan)
}

/// Calculate the critical path in the DAG.
/// Returns the longest path from root to leaf, representing the minimum execution time.
pub fn critical_path(dag: &WorkflowDag) -> Result<Vec<String>> {
    let topo_order = topological_sort(dag)?;

    // Calculate the longest path to each node
    let mut longest_path: HashMap<NodeIndex, (u64, Vec<String>)> = HashMap::new();

    for task_id in topo_order {
        if let Some(&node_idx) = dag.task_map.get(&task_id) {
            if let Some(task) = dag.graph.node_weight(node_idx) {
                // Get the execution time for this task
                let exec_time = task.timeout_secs.unwrap_or(60);

                // Find the longest path from predecessors
                let incoming_edges: Vec<_> = dag
                    .graph
                    .edges_directed(node_idx, Direction::Incoming)
                    .collect();

                let (max_predecessor_time, predecessor_path) = if incoming_edges.is_empty() {
                    (0, Vec::new())
                } else {
                    incoming_edges
                        .iter()
                        .filter_map(|edge| {
                            let source_idx = edge.source();
                            longest_path
                                .get(&source_idx)
                                .map(|(time, path)| (*time, path.clone()))
                        })
                        .max_by_key(|(time, _)| *time)
                        .unwrap_or((0, Vec::new()))
                };

                // Calculate path for this node
                let mut current_path = predecessor_path;
                current_path.push(task_id.clone());

                let total_time = max_predecessor_time + exec_time;
                longest_path.insert(node_idx, (total_time, current_path));
            }
        }
    }

    // Find the overall longest path (critical path)
    longest_path
        .values()
        .max_by_key(|(time, _)| *time)
        .map(|(_, path)| path.clone())
        .ok_or_else(|| DagError::EmptyDag.into())
}

/// Calculate the execution depth of each task (how many layers from root).
pub fn calculate_depths(dag: &WorkflowDag) -> Result<HashMap<String, usize>> {
    let execution_plan = create_execution_plan(dag)?;
    let mut depths = HashMap::new();

    for (depth, level) in execution_plan.iter().enumerate() {
        for task_id in level {
            depths.insert(task_id.clone(), depth);
        }
    }

    Ok(depths)
}

/// Find all paths from a source task to a destination task.
pub fn find_all_paths(dag: &WorkflowDag, source: &str, dest: &str) -> Vec<Vec<String>> {
    let source_idx = match dag.task_map.get(source) {
        Some(&idx) => idx,
        None => return Vec::new(),
    };

    let dest_idx = match dag.task_map.get(dest) {
        Some(&idx) => idx,
        None => return Vec::new(),
    };

    let mut all_paths = Vec::new();
    let mut current_path = Vec::new();
    let mut visited = HashSet::new();

    dfs_find_paths(
        dag,
        source_idx,
        dest_idx,
        &mut current_path,
        &mut visited,
        &mut all_paths,
    );

    all_paths
}

/// DFS helper for finding all paths.
fn dfs_find_paths(
    dag: &WorkflowDag,
    current: NodeIndex,
    dest: NodeIndex,
    path: &mut Vec<String>,
    visited: &mut HashSet<NodeIndex>,
    all_paths: &mut Vec<Vec<String>>,
) {
    if let Some(task) = dag.graph.node_weight(current) {
        path.push(task.id.clone());
    }

    visited.insert(current);

    if current == dest {
        all_paths.push(path.clone());
    } else {
        for neighbor in dag.graph.neighbors(current) {
            if !visited.contains(&neighbor) {
                dfs_find_paths(dag, neighbor, dest, path, visited, all_paths);
            }
        }
    }

    visited.remove(&current);
    path.pop();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::graph::{ResourceRequirements, RetryPolicy, TaskNode};
    use std::collections::HashMap;

    fn create_test_task(id: &str, timeout: u64) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            config: serde_json::json!({}),
            retry: RetryPolicy::default(),
            timeout_secs: Some(timeout),
            resources: ResourceRequirements::default(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_topological_sort() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", 10)).ok();
        dag.add_task(create_test_task("t2", 20)).ok();
        dag.add_task(create_test_task("t3", 30)).ok();

        dag.add_dependency("t1", "t2", Default::default()).ok();
        dag.add_dependency("t2", "t3", Default::default()).ok();

        let order = topological_sort(&dag).expect("Failed to sort");
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], "t1");
        assert_eq!(order[1], "t2");
        assert_eq!(order[2], "t3");
    }

    #[test]
    fn test_execution_plan() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", 10)).ok();
        dag.add_task(create_test_task("t2", 20)).ok();
        dag.add_task(create_test_task("t3", 30)).ok();
        dag.add_task(create_test_task("t4", 15)).ok();

        // t1 -> t2 -> t4
        // t1 -> t3 -> t4
        dag.add_dependency("t1", "t2", Default::default()).ok();
        dag.add_dependency("t1", "t3", Default::default()).ok();
        dag.add_dependency("t2", "t4", Default::default()).ok();
        dag.add_dependency("t3", "t4", Default::default()).ok();

        let plan = create_execution_plan(&dag).expect("Failed to create plan");
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].len(), 1); // t1
        assert_eq!(plan[1].len(), 2); // t2, t3
        assert_eq!(plan[2].len(), 1); // t4
    }

    #[test]
    fn test_critical_path() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", 10)).ok();
        dag.add_task(create_test_task("t2", 20)).ok();
        dag.add_task(create_test_task("t3", 30)).ok();
        dag.add_task(create_test_task("t4", 15)).ok();

        dag.add_dependency("t1", "t2", Default::default()).ok();
        dag.add_dependency("t1", "t3", Default::default()).ok();
        dag.add_dependency("t2", "t4", Default::default()).ok();
        dag.add_dependency("t3", "t4", Default::default()).ok();

        let path = critical_path(&dag).expect("Failed to find critical path");
        // Critical path should be t1 -> t3 -> t4 (10 + 30 + 15 = 55)
        assert!(path.contains(&"t1".to_string()));
        assert!(path.contains(&"t3".to_string()));
        assert!(path.contains(&"t4".to_string()));
    }

    #[test]
    fn test_calculate_depths() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", 10)).ok();
        dag.add_task(create_test_task("t2", 20)).ok();
        dag.add_task(create_test_task("t3", 30)).ok();

        dag.add_dependency("t1", "t2", Default::default()).ok();
        dag.add_dependency("t2", "t3", Default::default()).ok();

        let depths = calculate_depths(&dag).expect("Failed to calculate depths");
        assert_eq!(depths.get("t1"), Some(&0));
        assert_eq!(depths.get("t2"), Some(&1));
        assert_eq!(depths.get("t3"), Some(&2));
    }
}
