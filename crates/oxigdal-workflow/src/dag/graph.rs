//! DAG construction and validation.

use crate::error::{DagError, Result};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};

/// A task node in the workflow DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    /// Unique task identifier.
    pub id: String,
    /// Task name.
    pub name: String,
    /// Task description.
    pub description: Option<String>,
    /// Task configuration as JSON.
    pub config: serde_json::Value,
    /// Retry policy.
    pub retry: RetryPolicy,
    /// Timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Resource requirements.
    pub resources: ResourceRequirements,
    /// Custom metadata.
    pub metadata: HashMap<String, String>,
}

/// Retry policy for task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Delay between retries in milliseconds.
    pub delay_ms: u64,
    /// Backoff multiplier for exponential backoff.
    pub backoff_multiplier: f64,
    /// Maximum delay in milliseconds.
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            delay_ms: 1000,
            backoff_multiplier: 2.0,
            max_delay_ms: 60000,
        }
    }
}

/// Resource requirements for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    /// CPU cores required (can be fractional).
    pub cpu_cores: f64,
    /// Memory required in MB.
    pub memory_mb: u64,
    /// GPU required.
    pub gpu: bool,
    /// Disk space required in MB.
    pub disk_mb: u64,
    /// Custom resource requirements.
    pub custom: HashMap<String, f64>,
}

impl Default for ResourceRequirements {
    fn default() -> Self {
        Self {
            cpu_cores: 1.0,
            memory_mb: 1024,
            gpu: false,
            disk_mb: 1024,
            custom: HashMap::new(),
        }
    }
}

impl PartialEq for TaskNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for TaskNode {}

impl Hash for TaskNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// An edge representing a dependency between tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEdge {
    /// Edge type (data dependency, control dependency, etc.).
    pub edge_type: EdgeType,
    /// Condition for edge activation.
    pub condition: Option<String>,
}

/// Type of dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeType {
    /// Data dependency - output of one task is input to another.
    Data,
    /// Control dependency - one task must complete before another starts.
    Control,
    /// Conditional - edge is only followed if condition is met.
    Conditional,
}

impl Default for TaskEdge {
    fn default() -> Self {
        Self {
            edge_type: EdgeType::Control,
            condition: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]

/// Workflow DAG structure.
pub struct WorkflowDag {
    /// Underlying directed graph.
    pub(crate) graph: DiGraph<TaskNode, TaskEdge>,
    /// Mapping from task ID to node index.
    pub(crate) task_map: HashMap<String, NodeIndex>,
}

impl WorkflowDag {
    /// Create a new empty workflow DAG.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            task_map: HashMap::new(),
        }
    }

    /// Add a task to the DAG.
    pub fn add_task(&mut self, task: TaskNode) -> Result<NodeIndex> {
        if self.task_map.contains_key(&task.id) {
            return Err(
                DagError::InvalidNode(format!("Task '{}' already exists in DAG", task.id)).into(),
            );
        }

        let node_index = self.graph.add_node(task.clone());
        self.task_map.insert(task.id.clone(), node_index);
        Ok(node_index)
    }

    /// Add a dependency edge between two tasks.
    pub fn add_dependency(
        &mut self,
        from_task_id: &str,
        to_task_id: &str,
        edge: TaskEdge,
    ) -> Result<()> {
        let from_idx = self
            .task_map
            .get(from_task_id)
            .ok_or_else(|| DagError::invalid_node(from_task_id))?;

        let to_idx = self
            .task_map
            .get(to_task_id)
            .ok_or_else(|| DagError::invalid_node(to_task_id))?;

        self.graph.add_edge(*from_idx, *to_idx, edge);
        Ok(())
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: &str) -> Option<&TaskNode> {
        self.task_map
            .get(task_id)
            .and_then(|idx| self.graph.node_weight(*idx))
    }

    /// Get a task by ID (mutable).
    pub fn get_task_mut(&mut self, task_id: &str) -> Option<&mut TaskNode> {
        self.task_map
            .get(task_id)
            .and_then(|idx| self.graph.node_weight_mut(*idx))
    }

    /// Get task dependencies (tasks that must complete before this task).
    pub fn get_dependencies(&self, task_id: &str) -> Vec<String> {
        if let Some(&idx) = self.task_map.get(task_id) {
            self.graph
                .edges_directed(idx, Direction::Incoming)
                .filter_map(|edge| {
                    self.graph
                        .node_weight(edge.source())
                        .map(|task| task.id.clone())
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get task dependents (tasks that depend on this task).
    pub fn get_dependents(&self, task_id: &str) -> Vec<String> {
        if let Some(&idx) = self.task_map.get(task_id) {
            self.graph
                .edges_directed(idx, Direction::Outgoing)
                .filter_map(|edge| {
                    self.graph
                        .node_weight(edge.target())
                        .map(|task| task.id.clone())
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Validate the DAG structure.
    pub fn validate(&self) -> Result<()> {
        // Check if DAG is empty
        if self.graph.node_count() == 0 {
            return Err(DagError::EmptyDag.into());
        }

        // Check for cycles
        self.check_cycles()?;

        // Check for unreachable nodes
        self.check_reachability()?;

        Ok(())
    }

    /// Check for cycles in the DAG using DFS.
    fn check_cycles(&self) -> Result<()> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for node_idx in self.graph.node_indices() {
            if !visited.contains(&node_idx) {
                if let Some(cycle_path) =
                    self.dfs_cycle_check(node_idx, &mut visited, &mut rec_stack)
                {
                    return Err(DagError::cycle(cycle_path).into());
                }
            }
        }

        Ok(())
    }

    /// DFS-based cycle detection.
    fn dfs_cycle_check(
        &self,
        node: NodeIndex,
        visited: &mut HashSet<NodeIndex>,
        rec_stack: &mut HashSet<NodeIndex>,
    ) -> Option<String> {
        visited.insert(node);
        rec_stack.insert(node);

        for neighbor in self.graph.neighbors(node) {
            if !visited.contains(&neighbor) {
                if let Some(path) = self.dfs_cycle_check(neighbor, visited, rec_stack) {
                    return Some(path);
                }
            } else if rec_stack.contains(&neighbor) {
                // Cycle detected, construct path
                let current_task = self.graph.node_weight(node).map(|t| &t.id)?;
                let next_task = self.graph.node_weight(neighbor).map(|t| &t.id)?;
                return Some(format!("{} -> {}", current_task, next_task));
            }
        }

        rec_stack.remove(&node);
        None
    }

    /// Check if all nodes are reachable from root nodes.
    fn check_reachability(&self) -> Result<()> {
        // Find root nodes (nodes with no incoming edges)
        let root_nodes: Vec<NodeIndex> = self
            .graph
            .node_indices()
            .filter(|&idx| self.graph.edges_directed(idx, Direction::Incoming).count() == 0)
            .collect();

        if root_nodes.is_empty() {
            // If no root nodes, check if the graph has cycles (all nodes have incoming edges)
            return Ok(());
        }

        // BFS from all root nodes to find reachable nodes
        let mut reachable = HashSet::new();
        let mut queue = VecDeque::from(root_nodes);

        while let Some(node) = queue.pop_front() {
            if reachable.insert(node) {
                for neighbor in self.graph.neighbors(node) {
                    if !reachable.contains(&neighbor) {
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        // Check if all nodes are reachable
        for node_idx in self.graph.node_indices() {
            if !reachable.contains(&node_idx) {
                if let Some(task) = self.graph.node_weight(node_idx) {
                    return Err(DagError::UnreachableNode(task.id.clone()).into());
                }
            }
        }

        Ok(())
    }

    /// Get all tasks in the DAG.
    pub fn tasks(&self) -> Vec<&TaskNode> {
        self.graph
            .node_indices()
            .filter_map(|idx| self.graph.node_weight(idx))
            .collect()
    }

    /// Get the number of tasks in the DAG.
    pub fn task_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Get the number of dependencies in the DAG.
    pub fn dependency_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Get root tasks (tasks with no dependencies).
    pub fn root_tasks(&self) -> Vec<&TaskNode> {
        self.graph
            .node_indices()
            .filter(|&idx| self.graph.edges_directed(idx, Direction::Incoming).count() == 0)
            .filter_map(|idx| self.graph.node_weight(idx))
            .collect()
    }

    /// Get leaf tasks (tasks with no dependents).
    pub fn leaf_tasks(&self) -> Vec<&TaskNode> {
        self.graph
            .node_indices()
            .filter(|&idx| self.graph.edges_directed(idx, Direction::Outgoing).count() == 0)
            .filter_map(|idx| self.graph.node_weight(idx))
            .collect()
    }

    /// Get all edges in the DAG as (from_task_id, to_task_id, edge_data) tuples.
    ///
    /// This method is useful for visualization and serialization purposes.
    /// Returns edges in the order they are stored in the graph.
    pub fn edges(&self) -> Vec<(&str, &str, &TaskEdge)> {
        self.graph
            .edge_indices()
            .filter_map(|edge_idx| {
                let (from_idx, to_idx) = self.graph.edge_endpoints(edge_idx)?;
                let from_node = self.graph.node_weight(from_idx)?;
                let to_node = self.graph.node_weight(to_idx)?;
                let edge = self.graph.edge_weight(edge_idx)?;
                Some((from_node.id.as_str(), to_node.id.as_str(), edge))
            })
            .collect()
    }

    /// Get all edges with their edge types as (from_task_id, to_task_id, edge_type) tuples.
    ///
    /// A simplified version of `edges()` that only returns edge types.
    pub fn edge_pairs(&self) -> Vec<(String, String)> {
        self.graph
            .edge_indices()
            .filter_map(|edge_idx| {
                let (from_idx, to_idx) = self.graph.edge_endpoints(edge_idx)?;
                let from_node = self.graph.node_weight(from_idx)?;
                let to_node = self.graph.node_weight(to_idx)?;
                Some((from_node.id.clone(), to_node.id.clone()))
            })
            .collect()
    }

    /// Get task dependencies along with their edge data.
    ///
    /// Returns a vector of (dependency_task_id, edge_data) tuples for the given task.
    /// Dependencies are tasks that must complete before the specified task can start.
    pub fn get_dependencies_with_edges(&self, task_id: &str) -> Vec<(String, &TaskEdge)> {
        if let Some(&idx) = self.task_map.get(task_id) {
            self.graph
                .edges_directed(idx, Direction::Incoming)
                .filter_map(|edge| {
                    let source_node = self.graph.node_weight(edge.source())?;
                    Some((source_node.id.clone(), edge.weight()))
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get task dependents along with their edge data.
    ///
    /// Returns a vector of (dependent_task_id, edge_data) tuples for the given task.
    /// Dependents are tasks that wait for the specified task to complete.
    pub fn get_dependents_with_edges(&self, task_id: &str) -> Vec<(String, &TaskEdge)> {
        if let Some(&idx) = self.task_map.get(task_id) {
            self.graph
                .edges_directed(idx, Direction::Outgoing)
                .filter_map(|edge| {
                    let target_node = self.graph.node_weight(edge.target())?;
                    Some((target_node.id.clone(), edge.weight()))
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get the edge data between two specific tasks, if it exists.
    ///
    /// Returns `None` if either task does not exist or no edge connects them.
    pub fn get_edge_between(&self, from_task_id: &str, to_task_id: &str) -> Option<&TaskEdge> {
        let from_idx = self.task_map.get(from_task_id)?;
        let to_idx = self.task_map.get(to_task_id)?;
        self.graph
            .find_edge(*from_idx, *to_idx)
            .and_then(|edge_idx| self.graph.edge_weight(edge_idx))
    }

    /// Check if a dependency exists between two tasks.
    ///
    /// Returns `true` if `from_task_id` has a direct edge to `to_task_id`.
    pub fn has_dependency(&self, from_task_id: &str, to_task_id: &str) -> bool {
        self.get_edge_between(from_task_id, to_task_id).is_some()
    }

    /// Check if a task has any dependencies (incoming edges).
    pub fn has_dependencies(&self, task_id: &str) -> bool {
        if let Some(&idx) = self.task_map.get(task_id) {
            self.graph.edges_directed(idx, Direction::Incoming).count() > 0
        } else {
            false
        }
    }

    /// Check if a task has any dependents (outgoing edges).
    pub fn has_dependents(&self, task_id: &str) -> bool {
        if let Some(&idx) = self.task_map.get(task_id) {
            self.graph.edges_directed(idx, Direction::Outgoing).count() > 0
        } else {
            false
        }
    }

    /// Get the in-degree of a task (number of dependencies).
    pub fn in_degree(&self, task_id: &str) -> usize {
        if let Some(&idx) = self.task_map.get(task_id) {
            self.graph.edges_directed(idx, Direction::Incoming).count()
        } else {
            0
        }
    }

    /// Get the out-degree of a task (number of dependents).
    pub fn out_degree(&self, task_id: &str) -> usize {
        if let Some(&idx) = self.task_map.get(task_id) {
            self.graph.edges_directed(idx, Direction::Outgoing).count()
        } else {
            0
        }
    }

    /// Get all task IDs in the DAG.
    pub fn task_ids(&self) -> Vec<String> {
        self.task_map.keys().cloned().collect()
    }

    /// Check if a task exists in the DAG.
    pub fn contains_task(&self, task_id: &str) -> bool {
        self.task_map.contains_key(task_id)
    }

    /// Remove a task from the DAG along with all its edges.
    ///
    /// Returns the removed task, or `None` if the task did not exist.
    pub fn remove_task(&mut self, task_id: &str) -> Option<TaskNode> {
        let node_idx = self.task_map.remove(task_id)?;
        self.graph.remove_node(node_idx)
    }

    /// Get edges filtered by edge type.
    pub fn edges_by_type(&self, edge_type: EdgeType) -> Vec<(&str, &str, &TaskEdge)> {
        self.graph
            .edge_indices()
            .filter_map(|edge_idx| {
                let edge = self.graph.edge_weight(edge_idx)?;
                if edge.edge_type != edge_type {
                    return None;
                }
                let (from_idx, to_idx) = self.graph.edge_endpoints(edge_idx)?;
                let from_node = self.graph.node_weight(from_idx)?;
                let to_node = self.graph.node_weight(to_idx)?;
                Some((from_node.id.as_str(), to_node.id.as_str(), edge))
            })
            .collect()
    }

    /// Get a subgraph containing only the specified tasks and edges between them.
    ///
    /// Tasks not present in the original DAG are silently ignored.
    pub fn subgraph(&self, task_ids: &[&str]) -> WorkflowDag {
        let mut sub = WorkflowDag::new();
        let id_set: HashSet<&str> = task_ids.iter().copied().collect();

        // Add matching nodes
        for task_id in task_ids {
            if let Some(task) = self.get_task(task_id) {
                // Ignore errors from duplicate insertions if task_ids has duplicates
                let _ = sub.add_task(task.clone());
            }
        }

        // Add edges that connect nodes within the subgraph
        for (from_id, to_id, edge) in self.edges() {
            if id_set.contains(from_id) && id_set.contains(to_id) {
                let _ = sub.add_dependency(from_id, to_id, edge.clone());
            }
        }

        sub
    }

    /// Compute the transitive closure of dependencies for a task.
    ///
    /// Returns all tasks that must complete (directly or transitively) before
    /// the given task can execute.
    pub fn transitive_dependencies(&self, task_id: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        // Seed with direct dependencies
        for dep in self.get_dependencies(task_id) {
            if visited.insert(dep.clone()) {
                queue.push_back(dep);
            }
        }

        while let Some(current) = queue.pop_front() {
            for dep in self.get_dependencies(&current) {
                if visited.insert(dep.clone()) {
                    queue.push_back(dep);
                }
            }
        }

        visited.into_iter().collect()
    }

    /// Compute the transitive closure of dependents for a task.
    ///
    /// Returns all tasks that (directly or transitively) depend on the given task.
    pub fn transitive_dependents(&self, task_id: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        // Seed with direct dependents
        for dep in self.get_dependents(task_id) {
            if visited.insert(dep.clone()) {
                queue.push_back(dep);
            }
        }

        while let Some(current) = queue.pop_front() {
            for dep in self.get_dependents(&current) {
                if visited.insert(dep.clone()) {
                    queue.push_back(dep);
                }
            }
        }

        visited.into_iter().collect()
    }

    /// Get summary statistics about the DAG structure.
    pub fn summary(&self) -> DagSummary {
        let node_count = self.graph.node_count();
        let edge_count = self.graph.edge_count();
        let root_count = self.root_tasks().len();
        let leaf_count = self.leaf_tasks().len();

        let max_in_degree = self
            .graph
            .node_indices()
            .map(|idx| self.graph.edges_directed(idx, Direction::Incoming).count())
            .max()
            .unwrap_or(0);

        let max_out_degree = self
            .graph
            .node_indices()
            .map(|idx| self.graph.edges_directed(idx, Direction::Outgoing).count())
            .max()
            .unwrap_or(0);

        let data_edges = self.edges_by_type(EdgeType::Data).len();
        let control_edges = self.edges_by_type(EdgeType::Control).len();
        let conditional_edges = self.edges_by_type(EdgeType::Conditional).len();

        DagSummary {
            node_count,
            edge_count,
            root_count,
            leaf_count,
            max_in_degree,
            max_out_degree,
            data_edge_count: data_edges,
            control_edge_count: control_edges,
            conditional_edge_count: conditional_edges,
        }
    }
}

/// Summary statistics for a DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagSummary {
    /// Number of task nodes.
    pub node_count: usize,
    /// Number of dependency edges.
    pub edge_count: usize,
    /// Number of root tasks (no dependencies).
    pub root_count: usize,
    /// Number of leaf tasks (no dependents).
    pub leaf_count: usize,
    /// Maximum number of dependencies any single task has.
    pub max_in_degree: usize,
    /// Maximum number of dependents any single task has.
    pub max_out_degree: usize,
    /// Number of data dependency edges.
    pub data_edge_count: usize,
    /// Number of control dependency edges.
    pub control_edge_count: usize,
    /// Number of conditional dependency edges.
    pub conditional_edge_count: usize,
}

impl Default for WorkflowDag {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_add_task() {
        let mut dag = WorkflowDag::new();
        let task = create_test_task("task1", "Task 1");
        let result = dag.add_task(task);
        assert!(result.is_ok());
        assert_eq!(dag.task_count(), 1);
    }

    #[test]
    fn test_duplicate_task() {
        let mut dag = WorkflowDag::new();
        let task1 = create_test_task("task1", "Task 1");
        let task2 = create_test_task("task1", "Task 1 Duplicate");

        dag.add_task(task1).ok();
        let result = dag.add_task(task2);
        assert!(result.is_err());
    }

    #[test]
    fn test_add_dependency() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("task1", "Task 1")).ok();
        dag.add_task(create_test_task("task2", "Task 2")).ok();

        let result = dag.add_dependency("task1", "task2", TaskEdge::default());
        assert!(result.is_ok());
        assert_eq!(dag.dependency_count(), 1);
    }

    #[test]
    fn test_cycle_detection() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("task1", "Task 1")).ok();
        dag.add_task(create_test_task("task2", "Task 2")).ok();
        dag.add_task(create_test_task("task3", "Task 3")).ok();

        // Create a cycle: task1 -> task2 -> task3 -> task1
        dag.add_dependency("task1", "task2", TaskEdge::default())
            .ok();
        dag.add_dependency("task2", "task3", TaskEdge::default())
            .ok();
        dag.add_dependency("task3", "task1", TaskEdge::default())
            .ok();

        let result = dag.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_valid_dag() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("task1", "Task 1")).ok();
        dag.add_task(create_test_task("task2", "Task 2")).ok();
        dag.add_task(create_test_task("task3", "Task 3")).ok();

        // Create a valid DAG: task1 -> task2, task1 -> task3
        dag.add_dependency("task1", "task2", TaskEdge::default())
            .ok();
        dag.add_dependency("task1", "task3", TaskEdge::default())
            .ok();

        let result = dag.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_root_and_leaf_tasks() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("task1", "Task 1")).ok();
        dag.add_task(create_test_task("task2", "Task 2")).ok();
        dag.add_task(create_test_task("task3", "Task 3")).ok();

        dag.add_dependency("task1", "task2", TaskEdge::default())
            .ok();
        dag.add_dependency("task2", "task3", TaskEdge::default())
            .ok();

        let roots = dag.root_tasks();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, "task1");

        let leaves = dag.leaf_tasks();
        assert_eq!(leaves.len(), 1);
        assert_eq!(leaves[0].id, "task3");
    }

    #[test]
    fn test_edges() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();

        dag.add_dependency("t1", "t2", TaskEdge::default()).ok();
        dag.add_dependency(
            "t2",
            "t3",
            TaskEdge {
                edge_type: EdgeType::Data,
                condition: None,
            },
        )
        .ok();

        let edges = dag.edges();
        assert_eq!(edges.len(), 2);

        // Check first edge
        let (from, to, edge) = &edges[0];
        assert_eq!(*from, "t1");
        assert_eq!(*to, "t2");
        assert_eq!(edge.edge_type, EdgeType::Control);

        // Check second edge
        let (from, to, edge) = &edges[1];
        assert_eq!(*from, "t2");
        assert_eq!(*to, "t3");
        assert_eq!(edge.edge_type, EdgeType::Data);
    }

    #[test]
    fn test_get_dependencies_with_edges() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();

        dag.add_dependency(
            "t1",
            "t3",
            TaskEdge {
                edge_type: EdgeType::Data,
                condition: None,
            },
        )
        .ok();
        dag.add_dependency("t2", "t3", TaskEdge::default()).ok();

        let deps = dag.get_dependencies_with_edges("t3");
        assert_eq!(deps.len(), 2);

        // Both t1 and t2 should be dependencies of t3
        let dep_ids: Vec<&str> = deps.iter().map(|(id, _)| id.as_str()).collect();
        assert!(dep_ids.contains(&"t1"));
        assert!(dep_ids.contains(&"t2"));

        // No dependencies for root task
        let root_deps = dag.get_dependencies_with_edges("t1");
        assert!(root_deps.is_empty());

        // Non-existent task returns empty
        let missing_deps = dag.get_dependencies_with_edges("nonexistent");
        assert!(missing_deps.is_empty());
    }

    #[test]
    fn test_get_dependents_with_edges() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();

        dag.add_dependency("t1", "t2", TaskEdge::default()).ok();
        dag.add_dependency("t1", "t3", TaskEdge::default()).ok();

        let dependents = dag.get_dependents_with_edges("t1");
        assert_eq!(dependents.len(), 2);

        let dep_ids: Vec<&str> = dependents.iter().map(|(id, _)| id.as_str()).collect();
        assert!(dep_ids.contains(&"t2"));
        assert!(dep_ids.contains(&"t3"));
    }

    #[test]
    fn test_get_edge_between() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();

        dag.add_dependency(
            "t1",
            "t2",
            TaskEdge {
                edge_type: EdgeType::Data,
                condition: Some("output.ready".to_string()),
            },
        )
        .ok();

        let edge = dag.get_edge_between("t1", "t2");
        assert!(edge.is_some());
        let edge = edge.expect("Edge should exist");
        assert_eq!(edge.edge_type, EdgeType::Data);
        assert_eq!(edge.condition.as_deref(), Some("output.ready"));

        // Reverse direction should not exist
        assert!(dag.get_edge_between("t2", "t1").is_none());
        // Non-connected nodes
        assert!(dag.get_edge_between("t1", "t3").is_none());
    }

    #[test]
    fn test_has_dependency() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();

        dag.add_dependency("t1", "t2", TaskEdge::default()).ok();

        assert!(dag.has_dependency("t1", "t2"));
        assert!(!dag.has_dependency("t2", "t1"));
        assert!(!dag.has_dependency("t1", "nonexistent"));
    }

    #[test]
    fn test_has_dependencies_and_dependents() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();

        dag.add_dependency("t1", "t2", TaskEdge::default()).ok();
        dag.add_dependency("t2", "t3", TaskEdge::default()).ok();

        // t1: root, has dependents but no dependencies
        assert!(!dag.has_dependencies("t1"));
        assert!(dag.has_dependents("t1"));

        // t2: middle, has both
        assert!(dag.has_dependencies("t2"));
        assert!(dag.has_dependents("t2"));

        // t3: leaf, has dependencies but no dependents
        assert!(dag.has_dependencies("t3"));
        assert!(!dag.has_dependents("t3"));
    }

    #[test]
    fn test_in_out_degree() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();
        dag.add_task(create_test_task("t4", "Task 4")).ok();

        // t1 -> t3, t2 -> t3, t3 -> t4
        dag.add_dependency("t1", "t3", TaskEdge::default()).ok();
        dag.add_dependency("t2", "t3", TaskEdge::default()).ok();
        dag.add_dependency("t3", "t4", TaskEdge::default()).ok();

        assert_eq!(dag.in_degree("t1"), 0);
        assert_eq!(dag.out_degree("t1"), 1);
        assert_eq!(dag.in_degree("t3"), 2);
        assert_eq!(dag.out_degree("t3"), 1);
        assert_eq!(dag.in_degree("t4"), 1);
        assert_eq!(dag.out_degree("t4"), 0);
        // Non-existent
        assert_eq!(dag.in_degree("nonexistent"), 0);
    }

    #[test]
    fn test_task_ids_and_contains() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();

        let ids = dag.task_ids();
        assert_eq!(ids.len(), 2);
        assert!(dag.contains_task("t1"));
        assert!(dag.contains_task("t2"));
        assert!(!dag.contains_task("t3"));
    }

    #[test]
    fn test_remove_task() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_dependency("t1", "t2", TaskEdge::default()).ok();

        assert_eq!(dag.task_count(), 2);
        assert_eq!(dag.dependency_count(), 1);

        let removed = dag.remove_task("t1");
        assert!(removed.is_some());
        assert_eq!(removed.as_ref().map(|t| t.id.as_str()), Some("t1"));
        assert!(!dag.contains_task("t1"));

        // Removing non-existent should return None
        assert!(dag.remove_task("nonexistent").is_none());
    }

    #[test]
    fn test_edges_by_type() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();

        dag.add_dependency(
            "t1",
            "t2",
            TaskEdge {
                edge_type: EdgeType::Data,
                condition: None,
            },
        )
        .ok();
        dag.add_dependency("t1", "t3", TaskEdge::default()).ok();

        let data_edges = dag.edges_by_type(EdgeType::Data);
        assert_eq!(data_edges.len(), 1);
        assert_eq!(data_edges[0].0, "t1");
        assert_eq!(data_edges[0].1, "t2");

        let control_edges = dag.edges_by_type(EdgeType::Control);
        assert_eq!(control_edges.len(), 1);
        assert_eq!(control_edges[0].0, "t1");
        assert_eq!(control_edges[0].1, "t3");
    }

    #[test]
    fn test_subgraph() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();
        dag.add_task(create_test_task("t4", "Task 4")).ok();

        dag.add_dependency("t1", "t2", TaskEdge::default()).ok();
        dag.add_dependency("t2", "t3", TaskEdge::default()).ok();
        dag.add_dependency("t3", "t4", TaskEdge::default()).ok();

        // Extract subgraph with only t2 and t3
        let sub = dag.subgraph(&["t2", "t3"]);
        assert_eq!(sub.task_count(), 2);
        assert_eq!(sub.dependency_count(), 1);
        assert!(sub.contains_task("t2"));
        assert!(sub.contains_task("t3"));
        assert!(!sub.contains_task("t1"));
        assert!(!sub.contains_task("t4"));
    }

    #[test]
    fn test_transitive_dependencies() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();
        dag.add_task(create_test_task("t4", "Task 4")).ok();

        dag.add_dependency("t1", "t2", TaskEdge::default()).ok();
        dag.add_dependency("t2", "t3", TaskEdge::default()).ok();
        dag.add_dependency("t3", "t4", TaskEdge::default()).ok();

        let trans_deps = dag.transitive_dependencies("t4");
        assert_eq!(trans_deps.len(), 3);
        assert!(trans_deps.contains(&"t1".to_string()));
        assert!(trans_deps.contains(&"t2".to_string()));
        assert!(trans_deps.contains(&"t3".to_string()));

        // Root has no transitive dependencies
        let root_deps = dag.transitive_dependencies("t1");
        assert!(root_deps.is_empty());
    }

    #[test]
    fn test_transitive_dependents() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();
        dag.add_task(create_test_task("t4", "Task 4")).ok();

        dag.add_dependency("t1", "t2", TaskEdge::default()).ok();
        dag.add_dependency("t2", "t3", TaskEdge::default()).ok();
        dag.add_dependency("t3", "t4", TaskEdge::default()).ok();

        let trans_dependents = dag.transitive_dependents("t1");
        assert_eq!(trans_dependents.len(), 3);
        assert!(trans_dependents.contains(&"t2".to_string()));
        assert!(trans_dependents.contains(&"t3".to_string()));
        assert!(trans_dependents.contains(&"t4".to_string()));

        // Leaf has no transitive dependents
        let leaf_deps = dag.transitive_dependents("t4");
        assert!(leaf_deps.is_empty());
    }

    #[test]
    fn test_summary() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();
        dag.add_task(create_test_task("t4", "Task 4")).ok();

        dag.add_dependency(
            "t1",
            "t2",
            TaskEdge {
                edge_type: EdgeType::Data,
                condition: None,
            },
        )
        .ok();
        dag.add_dependency("t1", "t3", TaskEdge::default()).ok();
        dag.add_dependency("t2", "t4", TaskEdge::default()).ok();
        dag.add_dependency("t3", "t4", TaskEdge::default()).ok();

        let summary = dag.summary();
        assert_eq!(summary.node_count, 4);
        assert_eq!(summary.edge_count, 4);
        assert_eq!(summary.root_count, 1);
        assert_eq!(summary.leaf_count, 1);
        assert_eq!(summary.max_in_degree, 2); // t4 has 2 incoming
        assert_eq!(summary.max_out_degree, 2); // t1 has 2 outgoing
        assert_eq!(summary.data_edge_count, 1);
        assert_eq!(summary.control_edge_count, 3);
        assert_eq!(summary.conditional_edge_count, 0);
    }

    #[test]
    fn test_edge_pairs() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_dependency("t1", "t2", TaskEdge::default()).ok();

        let pairs = dag.edge_pairs();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0], ("t1".to_string(), "t2".to_string()));
    }

    #[test]
    fn test_get_dependencies_and_dependents() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("t1", "Task 1")).ok();
        dag.add_task(create_test_task("t2", "Task 2")).ok();
        dag.add_task(create_test_task("t3", "Task 3")).ok();

        dag.add_dependency("t1", "t3", TaskEdge::default()).ok();
        dag.add_dependency("t2", "t3", TaskEdge::default()).ok();

        let deps = dag.get_dependencies("t3");
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"t1".to_string()));
        assert!(deps.contains(&"t2".to_string()));

        let dependents = dag.get_dependents("t1");
        assert_eq!(dependents.len(), 1);
        assert!(dependents.contains(&"t3".to_string()));
    }
}
