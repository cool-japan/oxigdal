//! Task graph engine for managing task dependencies and execution order.
//!
//! This module implements a directed acyclic graph (DAG) for task dependencies,
//! with support for topological sorting, parallel execution planning, result caching,
//! and graph optimization through fusion and pruning.

use crate::error::{ClusterError, Result};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// A task in the task graph.
#[derive(Debug, Clone)]
pub struct Task {
    /// Unique task ID
    pub id: TaskId,

    /// Task name
    pub name: String,

    /// Task type
    pub task_type: String,

    /// Task priority (higher = more important)
    pub priority: i32,

    /// Task payload (serialized data)
    pub payload: Vec<u8>,

    /// Dependencies (tasks that must complete before this one)
    pub dependencies: Vec<TaskId>,

    /// Estimated execution time
    pub estimated_duration: Option<Duration>,

    /// Resource requirements
    pub resources: ResourceRequirements,

    /// Data locality hints (preferred worker IDs)
    pub locality_hints: Vec<String>,

    /// Creation timestamp
    pub created_at: Instant,

    /// Scheduled timestamp
    pub scheduled_at: Option<Instant>,

    /// Started timestamp
    pub started_at: Option<Instant>,

    /// Completed timestamp
    pub completed_at: Option<Instant>,

    /// Task status
    pub status: TaskStatus,

    /// Result (if completed)
    pub result: Option<TaskResult>,

    /// Error (if failed)
    pub error: Option<String>,

    /// Number of retry attempts
    pub retry_count: u32,

    /// Checkpoint data
    pub checkpoint: Option<Vec<u8>>,
}

/// Task identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub Uuid);

impl TaskId {
    /// Create a new random task ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create task ID from UUID.
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Resource requirements for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    /// CPU cores required
    pub cpu_cores: f64,

    /// Memory required (bytes)
    pub memory_bytes: u64,

    /// GPU required
    pub gpu: bool,

    /// Storage required (bytes)
    pub storage_bytes: u64,
}

impl Default for ResourceRequirements {
    fn default() -> Self {
        Self {
            cpu_cores: 1.0,
            memory_bytes: 1024 * 1024 * 1024, // 1 GB
            gpu: false,
            storage_bytes: 0,
        }
    }
}

/// Task status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is waiting for dependencies
    Pending,

    /// Task is ready to be scheduled
    Ready,

    /// Task is scheduled on a worker
    Scheduled,

    /// Task is running
    Running,

    /// Task completed successfully
    Completed,

    /// Task failed
    Failed,

    /// Task was cancelled
    Cancelled,
}

/// Task result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Result data
    pub data: Vec<u8>,

    /// Execution duration
    pub duration: Duration,

    /// Worker ID that executed the task
    pub worker_id: String,
}

/// Task graph for managing dependencies.
pub struct TaskGraph {
    /// All tasks in the graph
    tasks: DashMap<TaskId, Arc<RwLock<Task>>>,

    /// Dependency graph (task -> dependencies)
    dependencies: DashMap<TaskId, HashSet<TaskId>>,

    /// Reverse dependency graph (task -> dependents)
    dependents: DashMap<TaskId, HashSet<TaskId>>,

    /// Cached results
    result_cache: DashMap<String, Arc<TaskResult>>,

    /// Execution plan cache
    plan_cache: RwLock<Option<ExecutionPlan>>,
}

/// Execution plan for tasks.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    /// Tasks organized by execution level (topological order)
    pub levels: Vec<Vec<TaskId>>,

    /// Estimated total duration
    pub estimated_duration: Duration,

    /// Critical path
    pub critical_path: Vec<TaskId>,

    /// Parallelism by level
    pub parallelism: Vec<usize>,
}

impl TaskGraph {
    /// Create a new task graph.
    pub fn new() -> Self {
        Self {
            tasks: DashMap::new(),
            dependencies: DashMap::new(),
            dependents: DashMap::new(),
            result_cache: DashMap::new(),
            plan_cache: RwLock::new(None),
        }
    }

    /// Add a task to the graph.
    pub fn add_task(&self, mut task: Task) -> Result<TaskId> {
        let task_id = task.id;

        // Validate dependencies don't create cycles
        for dep_id in &task.dependencies {
            if self.would_create_cycle(task_id, *dep_id)? {
                return Err(ClusterError::DependencyCycle(format!(
                    "Adding task {} would create a cycle",
                    task_id
                )));
            }
        }

        // Set initial status
        if task.dependencies.is_empty() {
            task.status = TaskStatus::Ready;
        } else {
            task.status = TaskStatus::Pending;
        }

        // Store task
        self.tasks
            .insert(task_id, Arc::new(RwLock::new(task.clone())));

        // Update dependency graphs
        let deps: HashSet<TaskId> = task.dependencies.iter().copied().collect();
        self.dependencies.insert(task_id, deps.clone());

        for dep_id in deps {
            self.dependents.entry(dep_id).or_default().insert(task_id);
        }

        // Invalidate execution plan cache
        *self.plan_cache.write() = None;

        Ok(task_id)
    }

    /// Remove a task from the graph.
    pub fn remove_task(&self, task_id: TaskId) -> Result<()> {
        // Remove from tasks
        self.tasks.remove(&task_id);

        // Remove from dependency graph
        if let Some((_, deps)) = self.dependencies.remove(&task_id) {
            for dep_id in deps {
                if let Some(mut dependents) = self.dependents.get_mut(&dep_id) {
                    dependents.remove(&task_id);
                }
            }
        }

        // Remove from reverse dependency graph
        if let Some((_, dependents)) = self.dependents.remove(&task_id) {
            for dependent_id in dependents {
                if let Some(mut deps) = self.dependencies.get_mut(&dependent_id) {
                    deps.remove(&task_id);
                }
            }
        }

        // Invalidate execution plan cache
        *self.plan_cache.write() = None;

        Ok(())
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: TaskId) -> Result<Arc<RwLock<Task>>> {
        self.tasks
            .get(&task_id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| ClusterError::TaskNotFound(task_id.to_string()))
    }

    /// Get all tasks.
    pub fn get_all_tasks(&self) -> Vec<Arc<RwLock<Task>>> {
        self.tasks
            .iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect()
    }

    /// Get tasks by status.
    pub fn get_tasks_by_status(&self, status: TaskStatus) -> Vec<Arc<RwLock<Task>>> {
        self.tasks
            .iter()
            .filter(|entry| entry.value().read().status == status)
            .map(|entry| Arc::clone(entry.value()))
            .collect()
    }

    /// Update task status.
    pub fn update_task_status(&self, task_id: TaskId, status: TaskStatus) -> Result<()> {
        let task = self.get_task(task_id)?;
        let mut task = task.write();

        let old_status = task.status;
        task.status = status;

        match status {
            TaskStatus::Scheduled => {
                task.scheduled_at = Some(Instant::now());
            }
            TaskStatus::Running => {
                task.started_at = Some(Instant::now());
            }
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled => {
                task.completed_at = Some(Instant::now());

                // If completed, update dependent tasks
                if status == TaskStatus::Completed {
                    drop(task); // Release lock
                    self.update_dependents(task_id)?;
                }
            }
            _ => {}
        }

        // Invalidate execution plan cache if status changed
        if old_status != status {
            *self.plan_cache.write() = None;
        }

        Ok(())
    }

    /// Set task result.
    pub fn set_task_result(&self, task_id: TaskId, result: TaskResult) -> Result<()> {
        let task = self.get_task(task_id)?;
        let mut task = task.write();

        task.result = Some(result.clone());
        task.status = TaskStatus::Completed;
        task.completed_at = Some(Instant::now());

        // Cache result if task has a name
        if !task.name.is_empty() {
            self.result_cache
                .insert(task.name.clone(), Arc::new(result));
        }

        Ok(())
    }

    /// Set task error.
    pub fn set_task_error(&self, task_id: TaskId, error: String) -> Result<()> {
        let task = self.get_task(task_id)?;
        let mut task = task.write();

        task.error = Some(error);
        task.status = TaskStatus::Failed;
        task.completed_at = Some(Instant::now());

        Ok(())
    }

    /// Get cached result by task name.
    pub fn get_cached_result(&self, name: &str) -> Option<Arc<TaskResult>> {
        self.result_cache
            .get(name)
            .map(|entry| Arc::clone(entry.value()))
    }

    /// Clear result cache.
    pub fn clear_result_cache(&self) {
        self.result_cache.clear();
    }

    /// Check if adding a dependency would create a cycle.
    fn would_create_cycle(&self, from: TaskId, to: TaskId) -> Result<bool> {
        if from == to {
            return Ok(true);
        }

        // BFS to check if there's a path from 'to' to 'from'
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(to);
        visited.insert(to);

        while let Some(current) = queue.pop_front() {
            if current == from {
                return Ok(true);
            }

            if let Some(deps) = self.dependencies.get(&current) {
                for dep in deps.iter() {
                    if visited.insert(*dep) {
                        queue.push_back(*dep);
                    }
                }
            }
        }

        Ok(false)
    }

    /// Update dependent tasks when a task completes.
    fn update_dependents(&self, completed_task_id: TaskId) -> Result<()> {
        if let Some(dependents) = self.dependents.get(&completed_task_id) {
            for dependent_id in dependents.iter() {
                let dependent_task = self.get_task(*dependent_id)?;
                let mut dependent_task = dependent_task.write();

                // Check if all dependencies are completed
                let all_deps_completed = dependent_task.dependencies.iter().all(|dep_id| {
                    self.tasks
                        .get(dep_id)
                        .map(|t| t.read().status == TaskStatus::Completed)
                        .unwrap_or(false)
                });

                if all_deps_completed && dependent_task.status == TaskStatus::Pending {
                    dependent_task.status = TaskStatus::Ready;
                }
            }
        }

        Ok(())
    }

    /// Build execution plan using topological sort.
    pub fn build_execution_plan(&self) -> Result<ExecutionPlan> {
        // Check cache first
        {
            let cache = self.plan_cache.read();
            if let Some(plan) = cache.as_ref() {
                return Ok(plan.clone());
            }
        }

        // Compute in-degrees
        let mut in_degrees = HashMap::new();
        for task_entry in self.tasks.iter() {
            let task_id = *task_entry.key();
            let task = task_entry.value().read();

            if task.status == TaskStatus::Completed
                || task.status == TaskStatus::Failed
                || task.status == TaskStatus::Cancelled
            {
                continue;
            }

            let deps = self
                .dependencies
                .get(&task_id)
                .map(|d| d.len())
                .unwrap_or(0);
            in_degrees.insert(task_id, deps);
        }

        // Topological sort with level assignment
        let mut levels = Vec::new();
        let mut current_level = Vec::new();
        let mut task_levels = HashMap::new();

        // Find all tasks with in-degree 0
        for (task_id, degree) in &in_degrees {
            if *degree == 0 {
                current_level.push(*task_id);
                task_levels.insert(*task_id, 0);
            }
        }

        let mut level_idx = 0;
        while !current_level.is_empty() {
            levels.push(current_level.clone());

            let mut next_level = Vec::new();

            for task_id in &current_level {
                if let Some(dependents) = self.dependents.get(task_id) {
                    for dependent_id in dependents.iter() {
                        if !in_degrees.contains_key(dependent_id) {
                            continue;
                        }

                        let new_degree = in_degrees
                            .get(dependent_id)
                            .copied()
                            .unwrap_or(0)
                            .saturating_sub(1);
                        in_degrees.insert(*dependent_id, new_degree);

                        if new_degree == 0 {
                            next_level.push(*dependent_id);
                            task_levels.insert(*dependent_id, level_idx + 1);
                        }
                    }
                }
            }

            current_level = next_level;
            level_idx += 1;
        }

        // Check for cycles (remaining tasks with non-zero in-degree)
        let remaining: Vec<_> = in_degrees
            .iter()
            .filter(|&(_, &degree)| degree > 0)
            .map(|(id, _)| *id)
            .collect();

        if !remaining.is_empty() {
            return Err(ClusterError::DependencyCycle(format!(
                "Cycle detected involving tasks: {:?}",
                remaining
            )));
        }

        // Compute parallelism per level
        let parallelism: Vec<usize> = levels.iter().map(|level| level.len()).collect();

        // Compute critical path
        let critical_path = self.compute_critical_path(&task_levels);

        // Estimate total duration
        let estimated_duration = self.estimate_total_duration(&levels);

        let plan = ExecutionPlan {
            levels,
            estimated_duration,
            critical_path,
            parallelism,
        };

        // Cache the plan
        *self.plan_cache.write() = Some(plan.clone());

        Ok(plan)
    }

    /// Compute the critical path (longest path) through the graph.
    fn compute_critical_path(&self, task_levels: &HashMap<TaskId, usize>) -> Vec<TaskId> {
        let mut longest_path = Vec::new();
        let mut max_duration = Duration::from_secs(0);

        // Find the task at the highest level
        let max_level = task_levels.values().max().copied().unwrap_or(0);

        // For each task at the max level, trace back the longest path
        for (task_id, level) in task_levels {
            if *level == max_level {
                let path = self.trace_longest_path(*task_id);
                let path_duration: Duration = path
                    .iter()
                    .filter_map(|id| self.tasks.get(id).and_then(|t| t.read().estimated_duration))
                    .sum();

                if path_duration > max_duration {
                    max_duration = path_duration;
                    longest_path = path;
                }
            }
        }

        longest_path
    }

    /// Trace the longest path from a task back to root.
    fn trace_longest_path(&self, task_id: TaskId) -> Vec<TaskId> {
        let mut path = vec![task_id];
        let mut current = task_id;

        loop {
            let deps = self.dependencies.get(&current);
            if deps.is_none() || deps.as_ref().map(|d| d.is_empty()).unwrap_or(true) {
                break;
            }

            // Find dependency with longest estimated duration
            let longest_dep = deps.as_ref().and_then(|deps| {
                deps.iter()
                    .max_by_key(|dep_id| {
                        self.tasks
                            .get(dep_id)
                            .and_then(|t| t.read().estimated_duration)
                            .unwrap_or(Duration::from_secs(0))
                    })
                    .copied()
            });

            match longest_dep {
                Some(dep_id) => {
                    path.push(dep_id);
                    current = dep_id;
                }
                None => break,
            }
        }

        path.reverse();
        path
    }

    /// Estimate total duration for execution plan.
    fn estimate_total_duration(&self, levels: &[Vec<TaskId>]) -> Duration {
        levels
            .iter()
            .map(|level| {
                level
                    .iter()
                    .filter_map(|id| self.tasks.get(id).and_then(|t| t.read().estimated_duration))
                    .max()
                    .unwrap_or(Duration::from_secs(0))
            })
            .sum()
    }

    /// Optimize the graph by fusing compatible tasks.
    pub fn optimize_fusion(&self) -> Result<Vec<(TaskId, TaskId)>> {
        let mut fused_pairs = Vec::new();

        // Find tasks that can be fused
        for task_entry in self.tasks.iter() {
            let task_id = *task_entry.key();
            let task = task_entry.value().read();

            if let Some(dependents) = self.dependents.get(&task_id) {
                // If task has exactly one dependent, consider fusion
                if dependents.len() == 1 {
                    let dependent_id = *dependents.iter().next().ok_or_else(|| {
                        ClusterError::InvalidState("Empty dependents set".to_string())
                    })?;

                    let dependent = self.get_task(dependent_id)?;
                    let dependent = dependent.read();

                    // Check if fusion is beneficial
                    if self.can_fuse_tasks(&task, &dependent) {
                        fused_pairs.push((task_id, dependent_id));
                    }
                }
            }
        }

        Ok(fused_pairs)
    }

    /// Check if two tasks can be fused.
    fn can_fuse_tasks(&self, task1: &Task, task2: &Task) -> bool {
        // Same task type
        if task1.task_type != task2.task_type {
            return false;
        }

        // Compatible resource requirements
        if task1.resources.gpu != task2.resources.gpu {
            return false;
        }

        // Estimated durations are short enough to benefit from fusion
        let task1_dur = task1.estimated_duration.unwrap_or(Duration::from_secs(1));
        let task2_dur = task2.estimated_duration.unwrap_or(Duration::from_secs(1));

        if task1_dur + task2_dur > Duration::from_secs(60) {
            return false;
        }

        true
    }

    /// Prune completed tasks from the graph.
    pub fn prune_completed(&self) -> Result<usize> {
        let completed_tasks: Vec<TaskId> = self
            .tasks
            .iter()
            .filter(|entry| {
                matches!(
                    entry.value().read().status,
                    TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
                )
            })
            .map(|entry| *entry.key())
            .collect();

        let count = completed_tasks.len();

        for task_id in completed_tasks {
            self.remove_task(task_id)?;
        }

        Ok(count)
    }

    /// Get graph statistics.
    pub fn get_statistics(&self) -> TaskGraphStatistics {
        let total_tasks = self.tasks.len();
        let mut status_counts = HashMap::new();

        for entry in self.tasks.iter() {
            let status = entry.value().read().status;
            *status_counts.entry(status).or_insert(0) += 1;
        }

        let total_edges = self.dependencies.iter().map(|e| e.value().len()).sum();

        TaskGraphStatistics {
            total_tasks,
            status_counts,
            total_edges,
            cached_results: self.result_cache.len(),
        }
    }
}

impl Default for TaskGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Task graph statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraphStatistics {
    /// Total number of tasks
    pub total_tasks: usize,

    /// Task counts by status
    pub status_counts: HashMap<TaskStatus, usize>,

    /// Total number of dependencies
    pub total_edges: usize,

    /// Number of cached results
    pub cached_results: usize,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn create_test_task(name: &str, dependencies: Vec<TaskId>) -> Task {
        Task {
            id: TaskId::new(),
            name: name.to_string(),
            task_type: "test".to_string(),
            priority: 0,
            payload: vec![],
            dependencies,
            estimated_duration: Some(Duration::from_secs(1)),
            resources: ResourceRequirements::default(),
            locality_hints: vec![],
            created_at: Instant::now(),
            scheduled_at: None,
            started_at: None,
            completed_at: None,
            status: TaskStatus::Pending,
            result: None,
            error: None,
            retry_count: 0,
            checkpoint: None,
        }
    }

    #[test]
    fn test_task_graph_creation() {
        let graph = TaskGraph::new();
        assert_eq!(graph.tasks.len(), 0);
    }

    #[test]
    fn test_add_task() {
        let graph = TaskGraph::new();
        let task = create_test_task("task1", vec![]);

        let result = graph.add_task(task);
        assert!(result.is_ok());
        assert_eq!(graph.tasks.len(), 1);
    }

    #[test]
    fn test_task_dependencies() {
        let graph = TaskGraph::new();

        let task1 = create_test_task("task1", vec![]);
        let task1_id = graph.add_task(task1).ok().unwrap_or_default();

        let task2 = create_test_task("task2", vec![task1_id]);
        let result = graph.add_task(task2);

        assert!(result.is_ok());
        assert_eq!(graph.tasks.len(), 2);
    }

    #[test]
    fn test_cycle_detection() {
        let graph = TaskGraph::new();

        let task1 = create_test_task("task1", vec![]);
        let task1_id = task1.id;
        let _ = graph.add_task(task1);

        let task2 = create_test_task("task2", vec![task1_id]);
        let task2_id = task2.id;
        let _ = graph.add_task(task2);

        // Test for cycle detection between task1 and task2
        let result = graph.would_create_cycle(task1_id, task2_id);

        assert!(result.is_ok());
        assert!(result.ok().unwrap_or(false));
    }

    #[test]
    fn test_execution_plan() {
        let graph = TaskGraph::new();

        let task1 = create_test_task("task1", vec![]);
        let task1_id = graph.add_task(task1).ok().unwrap_or_default();

        let task2 = create_test_task("task2", vec![task1_id]);
        graph.add_task(task2).ok();

        let plan = graph.build_execution_plan();
        assert!(plan.is_ok());

        let plan = plan.ok();
        if let Some(plan) = plan {
            assert_eq!(plan.levels.len(), 2);
            assert_eq!(plan.levels[0].len(), 1);
            assert_eq!(plan.levels[1].len(), 1);
        }
    }

    #[test]
    fn test_task_status_update() {
        let graph = TaskGraph::new();
        let task = create_test_task("task1", vec![]);
        let task_id = graph.add_task(task).ok().unwrap_or_default();

        let result = graph.update_task_status(task_id, TaskStatus::Running);
        assert!(result.is_ok());

        let task = graph.get_task(task_id);
        assert!(task.is_ok());
        if let Ok(task) = task {
            assert_eq!(task.read().status, TaskStatus::Running);
        }
    }
}
