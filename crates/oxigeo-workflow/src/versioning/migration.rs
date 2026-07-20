//! Workflow migration utilities.

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};
use serde::{Deserialize, Serialize};

/// Workflow migration manager.
pub struct WorkflowMigration;

impl WorkflowMigration {
    /// Create a new migration manager.
    pub fn new() -> Self {
        Self
    }

    /// Migrate a workflow from one version to another.
    pub fn migrate(
        &self,
        from: WorkflowDefinition,
        to: WorkflowDefinition,
    ) -> Result<WorkflowDefinition> {
        // Create migration plan
        let plan = self.create_migration_plan(&from, &to)?;

        // Execute migration
        self.execute_migration_plan(from, plan)
    }

    /// Create a migration plan.
    fn create_migration_plan(
        &self,
        from: &WorkflowDefinition,
        to: &WorkflowDefinition,
    ) -> Result<MigrationPlan> {
        let mut steps = Vec::new();

        // Compare versions
        if from.version != to.version {
            steps.push(MigrationStep::UpdateVersion {
                from: from.version.clone(),
                to: to.version.clone(),
            });
        }

        // Compare task counts
        if from.dag.task_count() != to.dag.task_count() {
            steps.push(MigrationStep::UpdateTaskCount {
                from: from.dag.task_count(),
                to: to.dag.task_count(),
            });
        }

        // Compare metadata
        Ok(MigrationPlan {
            steps,
            requires_downtime: false,
            estimated_duration_secs: 0,
        })
    }

    /// Execute a migration plan.
    fn execute_migration_plan(
        &self,
        mut workflow: WorkflowDefinition,
        plan: MigrationPlan,
    ) -> Result<WorkflowDefinition> {
        for step in plan.steps {
            workflow = self.execute_migration_step(workflow, step)?;
        }

        Ok(workflow)
    }

    /// Execute a single migration step.
    fn execute_migration_step(
        &self,
        mut workflow: WorkflowDefinition,
        step: MigrationStep,
    ) -> Result<WorkflowDefinition> {
        match step {
            MigrationStep::UpdateVersion { to, .. } => {
                workflow.version = to;
            }
            MigrationStep::UpdateTaskCount { .. } => {
                // Task updates would be handled separately
            }
            MigrationStep::UpdateMetadata => {
                // Metadata updates would be handled separately
            }
            MigrationStep::Custom { .. } => {
                // Custom migration logic
            }
        }

        Ok(workflow)
    }

    /// Validate migration compatibility.
    pub fn validate_migration(
        &self,
        from: &WorkflowDefinition,
        to: &WorkflowDefinition,
    ) -> Result<()> {
        // Check if migration is possible
        if from.id != to.id {
            return Err(WorkflowError::versioning(
                "Cannot migrate between different workflows",
            ));
        }

        Ok(())
    }
}

impl Default for WorkflowMigration {
    fn default() -> Self {
        Self::new()
    }
}

/// Migration plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    /// Migration steps to execute.
    pub steps: Vec<MigrationStep>,
    /// Whether the migration requires downtime.
    pub requires_downtime: bool,
    /// Estimated duration in seconds.
    pub estimated_duration_secs: u64,
}

/// Migration step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MigrationStep {
    /// Update workflow version.
    UpdateVersion {
        /// From version.
        from: String,
        /// To version.
        to: String,
    },
    /// Update task count.
    UpdateTaskCount {
        /// From count.
        from: usize,
        /// To count.
        to: usize,
    },
    /// Update metadata.
    UpdateMetadata,
    /// Custom migration step.
    Custom {
        /// Step name.
        name: String,
        /// Step description.
        description: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::WorkflowDag;

    #[test]
    fn test_migration_creation() {
        let migration = WorkflowMigration::new();

        let from = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let to = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            version: "2.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        assert!(migration.validate_migration(&from, &to).is_ok());
    }

    #[test]
    fn test_migration_plan() {
        let migration = WorkflowMigration::new();

        let from = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let to = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            version: "2.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let plan = migration.create_migration_plan(&from, &to).expect("Failed");

        assert!(!plan.steps.is_empty());
    }

    #[test]
    fn test_invalid_migration() {
        let migration = WorkflowMigration::new();

        let from = WorkflowDefinition {
            id: "test1".to_string(),
            name: "Test1".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let to = WorkflowDefinition {
            id: "test2".to_string(),
            name: "Test2".to_string(),
            description: None,
            version: "2.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        assert!(migration.validate_migration(&from, &to).is_err());
    }
}
