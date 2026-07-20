//! Workflow rollback utilities.

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Rollback point for a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPoint {
    /// Rollback point ID.
    pub id: String,
    /// Workflow ID.
    pub workflow_id: String,
    /// Workflow definition at this point.
    pub definition: WorkflowDefinition,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Description.
    pub description: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
}

/// Rollback manager for workflow versions.
pub struct RollbackManager {
    rollback_points: Arc<DashMap<String, RollbackPoint>>,
    max_rollback_points: usize,
}

impl RollbackManager {
    /// Create a new rollback manager.
    pub fn new() -> Self {
        Self {
            rollback_points: Arc::new(DashMap::new()),
            max_rollback_points: 100,
        }
    }

    /// Create a new rollback manager with custom limits.
    pub fn with_max_points(max_points: usize) -> Self {
        Self {
            rollback_points: Arc::new(DashMap::new()),
            max_rollback_points: max_points,
        }
    }

    /// Create a rollback point.
    pub fn create_rollback_point(
        &self,
        workflow_id: String,
        definition: WorkflowDefinition,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();

        let rollback_point = RollbackPoint {
            id: id.clone(),
            workflow_id: workflow_id.clone(),
            definition,
            created_at: Utc::now(),
            description: None,
            tags: Vec::new(),
        };

        // Check if we've exceeded max rollback points for this workflow
        let workflow_points: Vec<String> = self
            .rollback_points
            .iter()
            .filter(|entry| entry.value().workflow_id == workflow_id)
            .map(|entry| entry.key().clone())
            .collect();

        if workflow_points.len() >= self.max_rollback_points {
            // Remove the oldest rollback point
            if let Some(oldest) = workflow_points.first() {
                self.rollback_points.remove(oldest);
            }
        }

        self.rollback_points.insert(id.clone(), rollback_point);

        Ok(id)
    }

    /// Rollback to a specific rollback point.
    pub fn rollback(&self, rollback_id: &str) -> Result<WorkflowDefinition> {
        let rollback_point = self
            .rollback_points
            .get(rollback_id)
            .ok_or_else(|| WorkflowError::not_found(rollback_id))?;

        Ok(rollback_point.definition.clone())
    }

    /// Get a rollback point.
    pub fn get_rollback_point(&self, rollback_id: &str) -> Option<RollbackPoint> {
        self.rollback_points
            .get(rollback_id)
            .map(|entry| entry.clone())
    }

    /// List all rollback points for a workflow.
    pub fn list_rollback_points(&self, workflow_id: &str) -> Vec<RollbackPoint> {
        let mut points: Vec<RollbackPoint> = self
            .rollback_points
            .iter()
            .filter(|entry| entry.value().workflow_id == workflow_id)
            .map(|entry| entry.value().clone())
            .collect();

        points.sort_by_key(|x| std::cmp::Reverse(x.created_at));

        points
    }

    /// Delete a rollback point.
    pub fn delete_rollback_point(&self, rollback_id: &str) -> Option<RollbackPoint> {
        self.rollback_points
            .remove(rollback_id)
            .map(|(_, point)| point)
    }

    /// Delete all rollback points for a workflow.
    pub fn delete_workflow_rollback_points(&self, workflow_id: &str) -> usize {
        let points_to_delete: Vec<String> = self
            .rollback_points
            .iter()
            .filter(|entry| entry.value().workflow_id == workflow_id)
            .map(|entry| entry.key().clone())
            .collect();

        let count = points_to_delete.len();

        for id in points_to_delete {
            self.rollback_points.remove(&id);
        }

        count
    }

    /// Get the latest rollback point for a workflow.
    pub fn get_latest_rollback_point(&self, workflow_id: &str) -> Option<RollbackPoint> {
        self.list_rollback_points(workflow_id).into_iter().next()
    }

    /// Clear all rollback points.
    pub fn clear_all(&self) {
        self.rollback_points.clear();
    }

    /// Get total count of rollback points.
    pub fn count(&self) -> usize {
        self.rollback_points.len()
    }

    /// Update rollback point description.
    pub fn update_description(&self, rollback_id: &str, description: String) -> Result<()> {
        let mut point = self
            .rollback_points
            .get_mut(rollback_id)
            .ok_or_else(|| WorkflowError::not_found(rollback_id))?;

        point.description = Some(description);

        Ok(())
    }

    /// Add tag to rollback point.
    pub fn add_tag(&self, rollback_id: &str, tag: String) -> Result<()> {
        let mut point = self
            .rollback_points
            .get_mut(rollback_id)
            .ok_or_else(|| WorkflowError::not_found(rollback_id))?;

        if !point.tags.contains(&tag) {
            point.tags.push(tag);
        }

        Ok(())
    }

    /// Search rollback points by tag.
    pub fn search_by_tag(&self, tag: &str) -> Vec<RollbackPoint> {
        self.rollback_points
            .iter()
            .filter(|entry| entry.value().tags.contains(&tag.to_string()))
            .map(|entry| entry.value().clone())
            .collect()
    }
}

impl Default for RollbackManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::WorkflowDag;

    #[test]
    fn test_rollback_manager_creation() {
        let manager = RollbackManager::new();
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_create_rollback_point() {
        let manager = RollbackManager::new();

        let definition = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let rollback_id = manager
            .create_rollback_point("test-workflow".to_string(), definition)
            .expect("Failed to create rollback point");

        assert!(manager.get_rollback_point(&rollback_id).is_some());
    }

    #[test]
    fn test_rollback() {
        let manager = RollbackManager::new();

        let definition = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let rollback_id = manager
            .create_rollback_point("test-workflow".to_string(), definition)
            .expect("Failed to create");

        let restored = manager.rollback(&rollback_id).expect("Failed to rollback");

        assert_eq!(restored.id, "test");
        assert_eq!(restored.version, "1.0.0");
    }

    #[test]
    fn test_list_rollback_points() {
        let manager = RollbackManager::new();

        for i in 0..3 {
            let definition = WorkflowDefinition {
                id: "test".to_string(),
                name: format!("Test {}", i),
                description: None,
                version: format!("1.0.{}", i),
                dag: WorkflowDag::new(),
            };

            manager
                .create_rollback_point("test-workflow".to_string(), definition)
                .expect("Failed to create");
        }

        let points = manager.list_rollback_points("test-workflow");
        assert_eq!(points.len(), 3);
    }

    #[test]
    fn test_delete_rollback_point() {
        let manager = RollbackManager::new();

        let definition = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let rollback_id = manager
            .create_rollback_point("test".to_string(), definition)
            .expect("Failed to create");

        assert!(manager.delete_rollback_point(&rollback_id).is_some());
        assert!(manager.get_rollback_point(&rollback_id).is_none());
    }

    #[test]
    fn test_max_rollback_points() {
        let manager = RollbackManager::with_max_points(3);

        for i in 0..5 {
            let definition = WorkflowDefinition {
                id: "test".to_string(),
                name: format!("Test {}", i),
                description: None,
                version: format!("1.0.{}", i),
                dag: WorkflowDag::new(),
            };

            manager
                .create_rollback_point("test".to_string(), definition)
                .expect("Failed to create");
        }

        let points = manager.list_rollback_points("test");
        // Should only keep the last 3
        assert!(points.len() <= 3);
    }

    #[test]
    fn test_update_description() {
        let manager = RollbackManager::new();

        let definition = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let id = manager
            .create_rollback_point("test".to_string(), definition)
            .expect("Failed to create");

        manager
            .update_description(&id, "Test description".to_string())
            .expect("Failed to update");

        let point = manager.get_rollback_point(&id).expect("Not found");
        assert_eq!(point.description, Some("Test description".to_string()));
    }

    #[test]
    fn test_search_by_tag() {
        let manager = RollbackManager::new();

        let definition = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let id = manager
            .create_rollback_point("test".to_string(), definition)
            .expect("Failed to create");

        manager
            .add_tag(&id, "production".to_string())
            .expect("Failed to add tag");

        let tagged = manager.search_by_tag("production");
        assert_eq!(tagged.len(), 1);
    }
}
