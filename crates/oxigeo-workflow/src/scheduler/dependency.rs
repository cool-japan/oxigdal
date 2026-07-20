//! Cross-workflow dependency scheduling.

use crate::error::{Result, WorkflowError};
use crate::scheduler::{ExecutionStatus, SchedulerConfig};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Workflow dependency definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDependency {
    /// Workflow ID that depends on others.
    pub workflow_id: String,
    /// List of workflow IDs this workflow depends on.
    pub dependencies: Vec<DependencyRule>,
    /// Dependency resolution strategy.
    pub strategy: DependencyStrategy,
    /// Description of the dependency.
    pub description: Option<String>,
}

/// Dependency rule for a single dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyRule {
    /// Dependent workflow ID.
    pub workflow_id: String,
    /// Required execution status.
    pub required_status: ExecutionStatus,
    /// Optional time window in seconds (dependency must complete within this window).
    pub time_window_secs: Option<u64>,
    /// Optional execution version/tag requirement.
    pub version_requirement: Option<String>,
}

/// Dependency resolution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyStrategy {
    /// All dependencies must be satisfied.
    All,
    /// At least one dependency must be satisfied.
    Any,
    /// Exactly N dependencies must be satisfied.
    AtLeast {
        /// Minimum number of dependencies that must be satisfied.
        count: usize,
    },
    /// Custom voting strategy (majority).
    Majority,
}

/// Dependency graph for tracking workflow dependencies.
#[derive(Debug)]
pub struct DependencyGraph {
    /// Map of workflow ID to its dependencies.
    dependencies: HashMap<String, HashSet<String>>,
    /// Map of workflow ID to workflows that depend on it.
    dependents: HashMap<String, HashSet<String>>,
}

impl DependencyGraph {
    /// Create a new empty dependency graph.
    pub fn new() -> Self {
        Self {
            dependencies: HashMap::new(),
            dependents: HashMap::new(),
        }
    }

    /// Add a dependency edge.
    pub fn add_dependency(&mut self, workflow_id: String, dependency_id: String) {
        self.dependencies
            .entry(workflow_id.clone())
            .or_default()
            .insert(dependency_id.clone());

        self.dependents
            .entry(dependency_id)
            .or_default()
            .insert(workflow_id);
    }

    /// Remove a dependency edge.
    pub fn remove_dependency(&mut self, workflow_id: &str, dependency_id: &str) {
        if let Some(deps) = self.dependencies.get_mut(workflow_id) {
            deps.remove(dependency_id);
        }

        if let Some(dependents) = self.dependents.get_mut(dependency_id) {
            dependents.remove(workflow_id);
        }
    }

    /// Get all dependencies for a workflow.
    pub fn get_dependencies(&self, workflow_id: &str) -> Option<&HashSet<String>> {
        self.dependencies.get(workflow_id)
    }

    /// Get all workflows that depend on a given workflow.
    pub fn get_dependents(&self, workflow_id: &str) -> Option<&HashSet<String>> {
        self.dependents.get(workflow_id)
    }

    /// Check for circular dependencies using DFS.
    pub fn has_cycle(&self, start_id: &str) -> bool {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        self.has_cycle_util(start_id, &mut visited, &mut rec_stack)
    }

    /// Utility function for cycle detection.
    fn has_cycle_util(
        &self,
        current: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> bool {
        if rec_stack.contains(current) {
            return true;
        }

        if visited.contains(current) {
            return false;
        }

        visited.insert(current.to_string());
        rec_stack.insert(current.to_string());

        if let Some(deps) = self.dependencies.get(current) {
            for dep in deps {
                if self.has_cycle_util(dep, visited, rec_stack) {
                    return true;
                }
            }
        }

        rec_stack.remove(current);
        false
    }

    /// Get execution order using topological sort.
    pub fn get_execution_order(&self) -> Result<Vec<String>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut zero_in_degree = Vec::new();
        let mut result = Vec::new();

        // Initialize in-degrees for all nodes
        for workflow_id in self.dependencies.keys() {
            in_degree.entry(workflow_id.clone()).or_insert(0);
        }
        for deps in self.dependencies.values() {
            for dep in deps {
                in_degree.entry(dep.clone()).or_insert(0);
            }
        }

        // Calculate in-degrees: if workflow_id depends on deps,
        // then workflow_id has incoming edges from each dep
        for (workflow_id, deps) in &self.dependencies {
            for _ in deps {
                *in_degree.entry(workflow_id.clone()).or_insert(0) += 1;
            }
        }

        // Find nodes with zero in-degree
        for (id, &degree) in &in_degree {
            if degree == 0 {
                zero_in_degree.push(id.clone());
            }
        }

        // Process nodes
        while let Some(current) = zero_in_degree.pop() {
            result.push(current.clone());

            if let Some(dependents) = self.dependents.get(&current) {
                for dependent in dependents {
                    if let Some(degree) = in_degree.get_mut(dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            zero_in_degree.push(dependent.clone());
                        }
                    }
                }
            }
        }

        // Check if all nodes were processed (no cycle)
        if result.len() != in_degree.len() {
            return Err(WorkflowError::validation("Circular dependency detected"));
        }

        Ok(result)
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Dependency scheduler for managing cross-workflow dependencies.
pub struct DependencyScheduler {
    /// Scheduler configuration (reserved for future enhancements).
    _config: SchedulerConfig,
    dependencies: Arc<DashMap<String, WorkflowDependency>>,
    graph: Arc<parking_lot::RwLock<DependencyGraph>>,
    execution_status: Arc<DashMap<String, ExecutionStatus>>,
}

impl DependencyScheduler {
    /// Create a new dependency scheduler.
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            _config: config,
            dependencies: Arc::new(DashMap::new()),
            graph: Arc::new(parking_lot::RwLock::new(DependencyGraph::new())),
            execution_status: Arc::new(DashMap::new()),
        }
    }

    /// Add a workflow dependency.
    pub fn add_dependency(&self, dependency: WorkflowDependency) -> Result<()> {
        let workflow_id = dependency.workflow_id.clone();

        // Update the graph
        let mut graph = self.graph.write();
        for rule in &dependency.dependencies {
            graph.add_dependency(workflow_id.clone(), rule.workflow_id.clone());
        }

        // Check for cycles
        if graph.has_cycle(&workflow_id) {
            // Rollback
            for rule in &dependency.dependencies {
                graph.remove_dependency(&workflow_id, &rule.workflow_id);
            }
            return Err(WorkflowError::validation(format!(
                "Adding dependency would create a cycle for workflow '{}'",
                workflow_id
            )));
        }

        drop(graph);

        self.dependencies.insert(workflow_id, dependency);
        Ok(())
    }

    /// Remove a workflow dependency.
    pub fn remove_dependency(&self, workflow_id: &str) -> Result<()> {
        let entry = self
            .dependencies
            .remove(workflow_id)
            .ok_or_else(|| WorkflowError::not_found(workflow_id))?;

        let dependency = entry.1;

        // Update the graph
        let mut graph = self.graph.write();
        for rule in &dependency.dependencies {
            graph.remove_dependency(workflow_id, &rule.workflow_id);
        }

        Ok(())
    }

    /// Check if a workflow's dependencies are satisfied.
    pub fn are_dependencies_satisfied(&self, workflow_id: &str) -> Result<bool> {
        let dependency = self
            .dependencies
            .get(workflow_id)
            .ok_or_else(|| WorkflowError::not_found(workflow_id))?;

        let mut satisfied_count = 0;
        let total_count = dependency.dependencies.len();

        for rule in &dependency.dependencies {
            if self.is_dependency_satisfied(rule)? {
                satisfied_count += 1;
            }
        }

        let result = match dependency.strategy {
            DependencyStrategy::All => satisfied_count == total_count,
            DependencyStrategy::Any => satisfied_count > 0,
            DependencyStrategy::AtLeast { count } => satisfied_count >= count,
            DependencyStrategy::Majority => satisfied_count > total_count / 2,
        };

        Ok(result)
    }

    /// Check if a single dependency rule is satisfied.
    fn is_dependency_satisfied(&self, rule: &DependencyRule) -> Result<bool> {
        let status = self
            .execution_status
            .get(&rule.workflow_id)
            .map(|entry| *entry.value())
            .unwrap_or(ExecutionStatus::Pending);

        Ok(status == rule.required_status)
    }

    /// Update the execution status of a workflow.
    pub fn update_status(&self, workflow_id: String, status: ExecutionStatus) {
        self.execution_status.insert(workflow_id, status);
    }

    /// Get workflows that can be executed (dependencies satisfied).
    pub fn get_executable_workflows(&self) -> Result<Vec<String>> {
        let mut executable = Vec::new();

        for entry in self.dependencies.iter() {
            let workflow_id = entry.key();
            if self.are_dependencies_satisfied(workflow_id)? {
                executable.push(workflow_id.clone());
            }
        }

        Ok(executable)
    }

    /// Get the dependency graph.
    pub fn get_graph(&self) -> parking_lot::RwLockReadGuard<'_, DependencyGraph> {
        self.graph.read()
    }

    /// Get execution order for all workflows.
    pub fn get_execution_order(&self) -> Result<Vec<String>> {
        self.graph.read().get_execution_order()
    }

    /// Clear all dependencies.
    pub fn clear(&self) {
        self.dependencies.clear();
        self.execution_status.clear();
        let mut graph = self.graph.write();
        *graph = DependencyGraph::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_graph_creation() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("workflow1".to_string(), "workflow2".to_string());

        assert!(graph.get_dependencies("workflow1").is_some());
        assert_eq!(
            graph
                .get_dependencies("workflow1")
                .expect("Missing deps")
                .len(),
            1
        );
    }

    #[test]
    fn test_dependency_graph_cycle_detection() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("workflow1".to_string(), "workflow2".to_string());
        graph.add_dependency("workflow2".to_string(), "workflow3".to_string());
        graph.add_dependency("workflow3".to_string(), "workflow1".to_string());

        assert!(graph.has_cycle("workflow1"));
    }

    #[test]
    fn test_dependency_graph_execution_order() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("workflow1".to_string(), "workflow2".to_string());
        graph.add_dependency("workflow2".to_string(), "workflow3".to_string());

        let order = graph.get_execution_order().expect("Failed to get order");
        assert!(!order.is_empty());
    }

    #[test]
    fn test_dependency_scheduler() {
        let scheduler = DependencyScheduler::new(SchedulerConfig::default());

        let dependency = WorkflowDependency {
            workflow_id: "workflow1".to_string(),
            dependencies: vec![DependencyRule {
                workflow_id: "workflow2".to_string(),
                required_status: ExecutionStatus::Success,
                time_window_secs: None,
                version_requirement: None,
            }],
            strategy: DependencyStrategy::All,
            description: None,
        };

        scheduler
            .add_dependency(dependency)
            .expect("Failed to add dependency");

        // Initially not satisfied
        assert!(
            !scheduler
                .are_dependencies_satisfied("workflow1")
                .expect("Check failed")
        );

        // Update status
        scheduler.update_status("workflow2".to_string(), ExecutionStatus::Success);

        // Now satisfied
        assert!(
            scheduler
                .are_dependencies_satisfied("workflow1")
                .expect("Check failed")
        );
    }

    #[test]
    fn test_dependency_cycle_prevention() {
        let scheduler = DependencyScheduler::new(SchedulerConfig::default());

        let dep1 = WorkflowDependency {
            workflow_id: "workflow1".to_string(),
            dependencies: vec![DependencyRule {
                workflow_id: "workflow2".to_string(),
                required_status: ExecutionStatus::Success,
                time_window_secs: None,
                version_requirement: None,
            }],
            strategy: DependencyStrategy::All,
            description: None,
        };

        scheduler.add_dependency(dep1).expect("Failed to add");

        let dep2 = WorkflowDependency {
            workflow_id: "workflow2".to_string(),
            dependencies: vec![DependencyRule {
                workflow_id: "workflow1".to_string(),
                required_status: ExecutionStatus::Success,
                time_window_secs: None,
                version_requirement: None,
            }],
            strategy: DependencyStrategy::All,
            description: None,
        };

        // Should fail due to cycle
        assert!(scheduler.add_dependency(dep2).is_err());
    }
}
