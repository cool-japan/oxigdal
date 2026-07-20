//! Version comparison and diff functionality.
//!
//! Provides detailed comparison between workflow versions including:
//! - Structural diff of workflow definitions
//! - Task-level comparison
//! - Metadata diff
//! - Change detection and categorization

use crate::dag::{TaskNode, WorkflowDag};
use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Represents a diff between two workflow versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDiff {
    /// Source version.
    pub from_version: String,
    /// Target version.
    pub to_version: String,
    /// Summary of changes.
    pub summary: DiffSummary,
    /// Task-level changes.
    pub task_changes: Vec<TaskChange>,
    /// Dependency changes.
    pub dependency_changes: Vec<DependencyChange>,
    /// Metadata changes.
    pub metadata_changes: Vec<MetadataChange>,
    /// Structural changes.
    pub structural_changes: Vec<StructuralChange>,
}

/// Summary of changes between versions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiffSummary {
    /// Number of tasks added.
    pub tasks_added: usize,
    /// Number of tasks removed.
    pub tasks_removed: usize,
    /// Number of tasks modified.
    pub tasks_modified: usize,
    /// Number of dependencies added.
    pub dependencies_added: usize,
    /// Number of dependencies removed.
    pub dependencies_removed: usize,
    /// Whether the change is breaking.
    pub is_breaking: bool,
    /// Estimated migration complexity (1-10).
    pub migration_complexity: u8,
    /// Human-readable summary.
    pub description: String,
}

/// Represents a change to a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChange {
    /// Task ID.
    pub task_id: String,
    /// Type of change.
    pub change_type: TaskChangeType,
    /// Old task definition (if modified or removed).
    pub old_task: Option<TaskNodeSnapshot>,
    /// New task definition (if modified or added).
    pub new_task: Option<TaskNodeSnapshot>,
    /// Specific field changes (if modified).
    pub field_changes: Vec<FieldChange>,
}

/// Snapshot of a task node for diff purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNodeSnapshot {
    /// Task ID.
    pub id: String,
    /// Task name.
    pub name: String,
    /// Task description.
    pub description: Option<String>,
    /// Task configuration.
    pub config: serde_json::Value,
    /// Timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Metadata.
    pub metadata: HashMap<String, String>,
}

impl From<&TaskNode> for TaskNodeSnapshot {
    fn from(node: &TaskNode) -> Self {
        Self {
            id: node.id.clone(),
            name: node.name.clone(),
            description: node.description.clone(),
            config: node.config.clone(),
            timeout_secs: node.timeout_secs,
            metadata: node.metadata.clone(),
        }
    }
}

/// Type of task change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskChangeType {
    /// Task was added.
    Added,
    /// Task was removed.
    Removed,
    /// Task was modified.
    Modified,
    /// Task was renamed.
    Renamed,
    /// Task was moved.
    Moved,
}

/// Represents a change to a specific field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange {
    /// Field path (e.g., "config.batch_size").
    pub field_path: String,
    /// Old value (as JSON).
    pub old_value: Option<serde_json::Value>,
    /// New value (as JSON).
    pub new_value: Option<serde_json::Value>,
    /// Whether this is a breaking change.
    pub is_breaking: bool,
}

/// Represents a change to dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyChange {
    /// Source task ID.
    pub from_task: String,
    /// Target task ID.
    pub to_task: String,
    /// Type of change.
    pub change_type: DependencyChangeType,
}

/// Type of dependency change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyChangeType {
    /// Dependency was added.
    Added,
    /// Dependency was removed.
    Removed,
}

/// Represents a change to metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataChange {
    /// Metadata key.
    pub key: String,
    /// Old value.
    pub old_value: Option<String>,
    /// New value.
    pub new_value: Option<String>,
}

/// Represents a structural change to the workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StructuralChange {
    /// Name changed.
    NameChanged {
        old_name: String,
        new_name: String,
    },
    /// Description changed.
    DescriptionChanged {
        old_description: Option<String>,
        new_description: Option<String>,
    },
    /// Parallelism level changed.
    ParallelismChanged {
        old_level: usize,
        new_level: usize,
    },
    /// Entry points changed.
    EntryPointsChanged {
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// Exit points changed.
    ExitPointsChanged {
        added: Vec<String>,
        removed: Vec<String>,
    },
}

/// Version diff calculator.
pub struct VersionDiffCalculator {
    /// Whether to detect renames.
    detect_renames: bool,
    /// Similarity threshold for rename detection (0.0-1.0).
    rename_threshold: f64,
    /// Breaking change patterns.
    breaking_patterns: Vec<BreakingChangePattern>,
}

/// Pattern for detecting breaking changes.
#[derive(Debug, Clone)]
pub struct BreakingChangePattern {
    /// Pattern name.
    pub name: String,
    /// Field paths that indicate breaking changes when modified.
    pub field_paths: Vec<String>,
    /// Whether removal of the field is breaking.
    pub removal_is_breaking: bool,
}

impl Default for VersionDiffCalculator {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionDiffCalculator {
    /// Create a new diff calculator.
    pub fn new() -> Self {
        Self {
            detect_renames: true,
            rename_threshold: 0.8,
            breaking_patterns: Self::default_breaking_patterns(),
        }
    }

    /// Create default breaking change patterns.
    fn default_breaking_patterns() -> Vec<BreakingChangePattern> {
        vec![
            BreakingChangePattern {
                name: "config_schema".to_string(),
                field_paths: vec!["config".to_string()],
                removal_is_breaking: true,
            },
            BreakingChangePattern {
                name: "required_resources".to_string(),
                field_paths: vec!["resources.min_memory".to_string(), "resources.min_cpu".to_string()],
                removal_is_breaking: false,
            },
        ]
    }

    /// Enable or disable rename detection.
    pub fn with_rename_detection(mut self, enabled: bool) -> Self {
        self.detect_renames = enabled;
        self
    }

    /// Set the similarity threshold for rename detection.
    pub fn with_rename_threshold(mut self, threshold: f64) -> Self {
        self.rename_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Add a custom breaking change pattern.
    pub fn with_breaking_pattern(mut self, pattern: BreakingChangePattern) -> Self {
        self.breaking_patterns.push(pattern);
        self
    }

    /// Calculate the diff between two workflow definitions.
    pub fn calculate_diff(
        &self,
        from: &WorkflowDefinition,
        to: &WorkflowDefinition,
    ) -> Result<VersionDiff> {
        let mut task_changes = Vec::new();
        let mut dependency_changes = Vec::new();
        let mut metadata_changes = Vec::new();
        let mut structural_changes = Vec::new();

        // Calculate task changes
        self.calculate_task_changes(&from.dag, &to.dag, &mut task_changes)?;

        // Calculate dependency changes
        self.calculate_dependency_changes(&from.dag, &to.dag, &mut dependency_changes)?;

        // Calculate structural changes
        self.calculate_structural_changes(from, to, &mut structural_changes);

        // Calculate summary
        let summary = self.calculate_summary(&task_changes, &dependency_changes, &structural_changes);

        Ok(VersionDiff {
            from_version: from.version.clone(),
            to_version: to.version.clone(),
            summary,
            task_changes,
            dependency_changes,
            metadata_changes,
            structural_changes,
        })
    }

    /// Calculate task-level changes.
    fn calculate_task_changes(
        &self,
        from_dag: &WorkflowDag,
        to_dag: &WorkflowDag,
        changes: &mut Vec<TaskChange>,
    ) -> Result<()> {
        let from_tasks: HashMap<&str, &TaskNode> = from_dag
            .get_tasks()
            .iter()
            .map(|t| (t.id.as_str(), t))
            .collect();

        let to_tasks: HashMap<&str, &TaskNode> = to_dag
            .get_tasks()
            .iter()
            .map(|t| (t.id.as_str(), t))
            .collect();

        let from_ids: HashSet<&str> = from_tasks.keys().copied().collect();
        let to_ids: HashSet<&str> = to_tasks.keys().copied().collect();

        // Find added tasks
        for id in to_ids.difference(&from_ids) {
            if let Some(task) = to_tasks.get(id) {
                changes.push(TaskChange {
                    task_id: id.to_string(),
                    change_type: TaskChangeType::Added,
                    old_task: None,
                    new_task: Some(TaskNodeSnapshot::from(*task)),
                    field_changes: Vec::new(),
                });
            }
        }

        // Find removed tasks
        for id in from_ids.difference(&to_ids) {
            if let Some(task) = from_tasks.get(id) {
                changes.push(TaskChange {
                    task_id: id.to_string(),
                    change_type: TaskChangeType::Removed,
                    old_task: Some(TaskNodeSnapshot::from(*task)),
                    new_task: None,
                    field_changes: Vec::new(),
                });
            }
        }

        // Find modified tasks
        for id in from_ids.intersection(&to_ids) {
            if let (Some(old_task), Some(new_task)) = (from_tasks.get(id), to_tasks.get(id)) {
                let field_changes = self.calculate_field_changes(old_task, new_task);
                if !field_changes.is_empty() {
                    changes.push(TaskChange {
                        task_id: id.to_string(),
                        change_type: TaskChangeType::Modified,
                        old_task: Some(TaskNodeSnapshot::from(*old_task)),
                        new_task: Some(TaskNodeSnapshot::from(*new_task)),
                        field_changes,
                    });
                }
            }
        }

        Ok(())
    }

    /// Calculate field-level changes for a task.
    fn calculate_field_changes(&self, old_task: &TaskNode, new_task: &TaskNode) -> Vec<FieldChange> {
        let mut changes = Vec::new();

        // Compare name
        if old_task.name != new_task.name {
            changes.push(FieldChange {
                field_path: "name".to_string(),
                old_value: Some(serde_json::Value::String(old_task.name.clone())),
                new_value: Some(serde_json::Value::String(new_task.name.clone())),
                is_breaking: false,
            });
        }

        // Compare description
        if old_task.description != new_task.description {
            changes.push(FieldChange {
                field_path: "description".to_string(),
                old_value: old_task.description.as_ref().map(|s| serde_json::Value::String(s.clone())),
                new_value: new_task.description.as_ref().map(|s| serde_json::Value::String(s.clone())),
                is_breaking: false,
            });
        }

        // Compare config
        if old_task.config != new_task.config {
            changes.push(FieldChange {
                field_path: "config".to_string(),
                old_value: Some(old_task.config.clone()),
                new_value: Some(new_task.config.clone()),
                is_breaking: self.is_config_change_breaking(&old_task.config, &new_task.config),
            });
        }

        // Compare timeout
        if old_task.timeout_secs != new_task.timeout_secs {
            changes.push(FieldChange {
                field_path: "timeout_secs".to_string(),
                old_value: old_task.timeout_secs.map(|t| serde_json::Value::Number(t.into())),
                new_value: new_task.timeout_secs.map(|t| serde_json::Value::Number(t.into())),
                is_breaking: false,
            });
        }

        changes
    }

    /// Check if a config change is breaking.
    fn is_config_change_breaking(
        &self,
        _old_config: &serde_json::Value,
        _new_config: &serde_json::Value,
    ) -> bool {
        // Simplified breaking change detection
        // In a real implementation, this would analyze the config structure
        false
    }

    /// Calculate dependency changes.
    fn calculate_dependency_changes(
        &self,
        from_dag: &WorkflowDag,
        to_dag: &WorkflowDag,
        changes: &mut Vec<DependencyChange>,
    ) -> Result<()> {
        let from_deps = from_dag.get_all_dependencies();
        let to_deps = to_dag.get_all_dependencies();

        let from_set: HashSet<(&str, &str)> = from_deps.iter().map(|(f, t)| (f.as_str(), t.as_str())).collect();
        let to_set: HashSet<(&str, &str)> = to_deps.iter().map(|(f, t)| (f.as_str(), t.as_str())).collect();

        // Find added dependencies
        for (from, to) in to_set.difference(&from_set) {
            changes.push(DependencyChange {
                from_task: from.to_string(),
                to_task: to.to_string(),
                change_type: DependencyChangeType::Added,
            });
        }

        // Find removed dependencies
        for (from, to) in from_set.difference(&to_set) {
            changes.push(DependencyChange {
                from_task: from.to_string(),
                to_task: to.to_string(),
                change_type: DependencyChangeType::Removed,
            });
        }

        Ok(())
    }

    /// Calculate structural changes.
    fn calculate_structural_changes(
        &self,
        from: &WorkflowDefinition,
        to: &WorkflowDefinition,
        changes: &mut Vec<StructuralChange>,
    ) {
        // Name changed
        if from.name != to.name {
            changes.push(StructuralChange::NameChanged {
                old_name: from.name.clone(),
                new_name: to.name.clone(),
            });
        }

        // Description changed
        if from.description != to.description {
            changes.push(StructuralChange::DescriptionChanged {
                old_description: from.description.clone(),
                new_description: to.description.clone(),
            });
        }

        // Entry points changed
        let from_entry: HashSet<&str> = from.dag.get_entry_points().iter().map(|s| s.as_str()).collect();
        let to_entry: HashSet<&str> = to.dag.get_entry_points().iter().map(|s| s.as_str()).collect();

        let added_entry: Vec<String> = to_entry.difference(&from_entry).map(|s| s.to_string()).collect();
        let removed_entry: Vec<String> = from_entry.difference(&to_entry).map(|s| s.to_string()).collect();

        if !added_entry.is_empty() || !removed_entry.is_empty() {
            changes.push(StructuralChange::EntryPointsChanged {
                added: added_entry,
                removed: removed_entry,
            });
        }
    }

    /// Calculate the summary of changes.
    fn calculate_summary(
        &self,
        task_changes: &[TaskChange],
        dependency_changes: &[DependencyChange],
        structural_changes: &[StructuralChange],
    ) -> DiffSummary {
        let tasks_added = task_changes.iter().filter(|c| c.change_type == TaskChangeType::Added).count();
        let tasks_removed = task_changes.iter().filter(|c| c.change_type == TaskChangeType::Removed).count();
        let tasks_modified = task_changes.iter().filter(|c| c.change_type == TaskChangeType::Modified).count();

        let dependencies_added = dependency_changes.iter().filter(|c| c.change_type == DependencyChangeType::Added).count();
        let dependencies_removed = dependency_changes.iter().filter(|c| c.change_type == DependencyChangeType::Removed).count();

        // Check for breaking changes
        let has_breaking_field_changes = task_changes
            .iter()
            .any(|c| c.field_changes.iter().any(|f| f.is_breaking));

        let is_breaking = tasks_removed > 0 || dependencies_removed > 0 || has_breaking_field_changes;

        // Calculate migration complexity (1-10)
        let complexity = self.calculate_migration_complexity(
            tasks_added,
            tasks_removed,
            tasks_modified,
            dependencies_added,
            dependencies_removed,
            is_breaking,
        );

        // Generate description
        let description = self.generate_summary_description(
            tasks_added,
            tasks_removed,
            tasks_modified,
            structural_changes.len(),
        );

        DiffSummary {
            tasks_added,
            tasks_removed,
            tasks_modified,
            dependencies_added,
            dependencies_removed,
            is_breaking,
            migration_complexity: complexity,
            description,
        }
    }

    /// Calculate migration complexity score (1-10).
    fn calculate_migration_complexity(
        &self,
        tasks_added: usize,
        tasks_removed: usize,
        tasks_modified: usize,
        deps_added: usize,
        deps_removed: usize,
        is_breaking: bool,
    ) -> u8 {
        let mut score = 1u8;

        // Base complexity from change counts
        score = score.saturating_add((tasks_added / 2).min(3) as u8);
        score = score.saturating_add((tasks_removed * 2).min(4) as u8);
        score = score.saturating_add((tasks_modified / 2).min(2) as u8);
        score = score.saturating_add((deps_added / 3).min(2) as u8);
        score = score.saturating_add((deps_removed / 2).min(2) as u8);

        // Breaking changes increase complexity
        if is_breaking {
            score = score.saturating_add(3);
        }

        score.min(10)
    }

    /// Generate a human-readable summary description.
    fn generate_summary_description(
        &self,
        tasks_added: usize,
        tasks_removed: usize,
        tasks_modified: usize,
        structural_changes: usize,
    ) -> String {
        let mut parts = Vec::new();

        if tasks_added > 0 {
            parts.push(format!("{} task(s) added", tasks_added));
        }
        if tasks_removed > 0 {
            parts.push(format!("{} task(s) removed", tasks_removed));
        }
        if tasks_modified > 0 {
            parts.push(format!("{} task(s) modified", tasks_modified));
        }
        if structural_changes > 0 {
            parts.push(format!("{} structural change(s)", structural_changes));
        }

        if parts.is_empty() {
            "No significant changes".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Generate a patch that can be applied to transform one version to another.
    pub fn generate_patch(
        &self,
        from: &WorkflowDefinition,
        to: &WorkflowDefinition,
    ) -> Result<VersionPatch> {
        let diff = self.calculate_diff(from, to)?;

        Ok(VersionPatch {
            from_version: diff.from_version,
            to_version: diff.to_version,
            operations: self.diff_to_operations(&diff),
            reversible: !diff.summary.is_breaking,
        })
    }

    /// Convert a diff to patch operations.
    fn diff_to_operations(&self, diff: &VersionDiff) -> Vec<PatchOperation> {
        let mut operations = Vec::new();

        // Convert task changes to operations
        for change in &diff.task_changes {
            match change.change_type {
                TaskChangeType::Added => {
                    if let Some(ref task) = change.new_task {
                        operations.push(PatchOperation::AddTask {
                            task: task.clone(),
                        });
                    }
                }
                TaskChangeType::Removed => {
                    operations.push(PatchOperation::RemoveTask {
                        task_id: change.task_id.clone(),
                    });
                }
                TaskChangeType::Modified => {
                    for field_change in &change.field_changes {
                        operations.push(PatchOperation::ModifyField {
                            task_id: change.task_id.clone(),
                            field_path: field_change.field_path.clone(),
                            old_value: field_change.old_value.clone(),
                            new_value: field_change.new_value.clone(),
                        });
                    }
                }
                TaskChangeType::Renamed | TaskChangeType::Moved => {
                    // Handle as modify for now
                }
            }
        }

        // Convert dependency changes to operations
        for change in &diff.dependency_changes {
            match change.change_type {
                DependencyChangeType::Added => {
                    operations.push(PatchOperation::AddDependency {
                        from_task: change.from_task.clone(),
                        to_task: change.to_task.clone(),
                    });
                }
                DependencyChangeType::Removed => {
                    operations.push(PatchOperation::RemoveDependency {
                        from_task: change.from_task.clone(),
                        to_task: change.to_task.clone(),
                    });
                }
            }
        }

        operations
    }
}

/// A patch that can transform one workflow version to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionPatch {
    /// Source version.
    pub from_version: String,
    /// Target version.
    pub to_version: String,
    /// Patch operations.
    pub operations: Vec<PatchOperation>,
    /// Whether the patch is reversible.
    pub reversible: bool,
}

/// A single patch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatchOperation {
    /// Add a task.
    AddTask {
        task: TaskNodeSnapshot,
    },
    /// Remove a task.
    RemoveTask {
        task_id: String,
    },
    /// Modify a field.
    ModifyField {
        task_id: String,
        field_path: String,
        old_value: Option<serde_json::Value>,
        new_value: Option<serde_json::Value>,
    },
    /// Add a dependency.
    AddDependency {
        from_task: String,
        to_task: String,
    },
    /// Remove a dependency.
    RemoveDependency {
        from_task: String,
        to_task: String,
    },
    /// Rename workflow.
    RenameWorkflow {
        old_name: String,
        new_name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::graph::{ResourceRequirements, RetryPolicy};

    fn create_test_workflow(version: &str, tasks: Vec<(&str, &str)>) -> WorkflowDefinition {
        let mut dag = WorkflowDag::new();
        for (id, name) in tasks {
            let node = TaskNode {
                id: id.to_string(),
                name: name.to_string(),
                description: None,
                config: serde_json::json!({}),
                retry: RetryPolicy::default(),
                timeout_secs: Some(60),
                resources: ResourceRequirements::default(),
                metadata: HashMap::new(),
            };
            let _ = dag.add_task(node);
        }

        WorkflowDefinition {
            id: "test".to_string(),
            name: "Test Workflow".to_string(),
            version: version.to_string(),
            dag,
            description: None,
        }
    }

    #[test]
    fn test_calculate_diff_added_task() {
        let calculator = VersionDiffCalculator::new();

        let v1 = create_test_workflow("1.0.0", vec![("task1", "Task 1")]);
        let v2 = create_test_workflow("1.1.0", vec![("task1", "Task 1"), ("task2", "Task 2")]);

        let diff = calculator.calculate_diff(&v1, &v2).expect("Diff calculation failed");

        assert_eq!(diff.summary.tasks_added, 1);
        assert_eq!(diff.summary.tasks_removed, 0);
        assert!(!diff.summary.is_breaking);
    }

    #[test]
    fn test_calculate_diff_removed_task() {
        let calculator = VersionDiffCalculator::new();

        let v1 = create_test_workflow("1.0.0", vec![("task1", "Task 1"), ("task2", "Task 2")]);
        let v2 = create_test_workflow("2.0.0", vec![("task1", "Task 1")]);

        let diff = calculator.calculate_diff(&v1, &v2).expect("Diff calculation failed");

        assert_eq!(diff.summary.tasks_added, 0);
        assert_eq!(diff.summary.tasks_removed, 1);
        assert!(diff.summary.is_breaking);
    }

    #[test]
    fn test_generate_patch() {
        let calculator = VersionDiffCalculator::new();

        let v1 = create_test_workflow("1.0.0", vec![("task1", "Task 1")]);
        let v2 = create_test_workflow("1.1.0", vec![("task1", "Task 1"), ("task2", "Task 2")]);

        let patch = calculator.generate_patch(&v1, &v2).expect("Patch generation failed");

        assert_eq!(patch.from_version, "1.0.0");
        assert_eq!(patch.to_version, "1.1.0");
        assert!(!patch.operations.is_empty());
    }
}
