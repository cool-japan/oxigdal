//! Workflow versioning system.
//!
//! Provides semantic versioning, migration, and rollback capabilities
//! for workflow definitions.

pub mod migration;
pub mod rollback;

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub use migration::{MigrationPlan, MigrationStep, WorkflowMigration};
pub use rollback::{RollbackManager, RollbackPoint};

/// Workflow version information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowVersion {
    /// Version number (semantic versioning).
    pub version: String,
    /// Workflow definition at this version.
    pub definition: WorkflowDefinition,
    /// Version metadata.
    pub metadata: VersionMetadata,
    /// Previous version (if any).
    pub previous_version: Option<String>,
    /// Migration notes.
    pub migration_notes: Option<String>,
}

/// Version metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMetadata {
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Author.
    pub author: String,
    /// Changelog.
    pub changelog: Vec<ChangelogEntry>,
    /// Breaking changes.
    pub breaking_changes: Vec<String>,
    /// Deprecated features.
    pub deprecations: Vec<String>,
    /// Tags.
    pub tags: Vec<String>,
}

/// Changelog entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangelogEntry {
    /// Change type.
    pub change_type: ChangeType,
    /// Change description.
    pub description: String,
    /// Affected components.
    pub affected_components: Vec<String>,
}

/// Change type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    /// New feature added.
    Feature,
    /// Bug fix.
    Fix,
    /// Performance improvement.
    Performance,
    /// Breaking change.
    Breaking,
    /// Deprecation.
    Deprecation,
    /// Documentation update.
    Documentation,
    /// Refactoring.
    Refactor,
}

/// Version comparison result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionComparison {
    /// First version is less than second.
    Less,
    /// Versions are equal.
    Equal,
    /// First version is greater than second.
    Greater,
}

/// Workflow version manager.
pub struct WorkflowVersionManager {
    versions: Arc<DashMap<String, HashMap<String, WorkflowVersion>>>,
    migration: WorkflowMigration,
    rollback: RollbackManager,
}

impl WorkflowVersionManager {
    /// Create a new version manager.
    pub fn new() -> Self {
        Self {
            versions: Arc::new(DashMap::new()),
            migration: WorkflowMigration::new(),
            rollback: RollbackManager::new(),
        }
    }

    /// Register a new workflow version.
    pub fn register_version(&self, workflow_id: String, version: WorkflowVersion) -> Result<()> {
        // Validate version format
        Self::validate_version(&version.version)?;

        let mut workflow_versions = self.versions.entry(workflow_id.clone()).or_default();

        if workflow_versions.contains_key(&version.version) {
            return Err(WorkflowError::versioning(format!(
                "Version {} already exists for workflow {}",
                version.version, workflow_id
            )));
        }

        workflow_versions.insert(version.version.clone(), version);

        Ok(())
    }

    /// Get a specific version.
    pub fn get_version(&self, workflow_id: &str, version: &str) -> Option<WorkflowVersion> {
        self.versions
            .get(workflow_id)
            .and_then(|entry| entry.get(version).cloned())
    }

    /// Get the latest version.
    pub fn get_latest_version(&self, workflow_id: &str) -> Option<WorkflowVersion> {
        self.versions.get(workflow_id).and_then(|entry| {
            entry
                .values()
                .max_by(|a, b| Self::compare_versions(&a.version, &b.version))
                .cloned()
        })
    }

    /// List all versions for a workflow.
    pub fn list_versions(&self, workflow_id: &str) -> Vec<WorkflowVersion> {
        self.versions
            .get(workflow_id)
            .map(|entry| {
                let mut versions: Vec<WorkflowVersion> = entry.values().cloned().collect();
                versions.sort_by(|a, b| Self::compare_versions(&a.version, &b.version));
                versions
            })
            .unwrap_or_default()
    }

    /// Check if a version is compatible with another.
    pub fn is_compatible(&self, version1: &str, version2: &str) -> Result<bool> {
        let (major1, minor1, _) = Self::parse_version(version1)?;
        let (major2, minor2, _) = Self::parse_version(version2)?;

        // Same major version is compatible
        Ok(major1 == major2 && minor1 <= minor2)
    }

    /// Migrate from one version to another.
    pub fn migrate(
        &self,
        workflow_id: &str,
        from_version: &str,
        to_version: &str,
    ) -> Result<WorkflowDefinition> {
        let from = self
            .get_version(workflow_id, from_version)
            .ok_or_else(|| WorkflowError::not_found(from_version))?;

        let to = self
            .get_version(workflow_id, to_version)
            .ok_or_else(|| WorkflowError::not_found(to_version))?;

        self.migration.migrate(from.definition, to.definition)
    }

    /// Create a rollback point.
    pub fn create_rollback_point(&self, workflow_id: String, version: String) -> Result<String> {
        let workflow_version = self
            .get_version(&workflow_id, &version)
            .ok_or_else(|| WorkflowError::not_found(&version))?;

        self.rollback
            .create_rollback_point(workflow_id, workflow_version.definition)
    }

    /// Rollback to a previous point.
    pub fn rollback(&self, rollback_id: &str) -> Result<WorkflowDefinition> {
        self.rollback.rollback(rollback_id)
    }

    /// Validate semantic version format.
    fn validate_version(version: &str) -> Result<()> {
        Self::parse_version(version).map(|_| ())
    }

    /// Parse semantic version.
    fn parse_version(version: &str) -> Result<(u32, u32, u32)> {
        let parts: Vec<&str> = version
            .split('-')
            .next()
            .ok_or_else(|| WorkflowError::versioning("Invalid version format"))?
            .split('.')
            .collect();

        if parts.len() != 3 {
            return Err(WorkflowError::versioning(
                "Version must have 3 parts (major.minor.patch)",
            ));
        }

        let major = parts[0]
            .parse::<u32>()
            .map_err(|_| WorkflowError::versioning("Invalid major version"))?;

        let minor = parts[1]
            .parse::<u32>()
            .map_err(|_| WorkflowError::versioning("Invalid minor version"))?;

        let patch = parts[2]
            .parse::<u32>()
            .map_err(|_| WorkflowError::versioning("Invalid patch version"))?;

        Ok((major, minor, patch))
    }

    /// Compare two versions.
    fn compare_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
        let Ok((major1, minor1, patch1)) = Self::parse_version(v1) else {
            return std::cmp::Ordering::Equal;
        };

        let Ok((major2, minor2, patch2)) = Self::parse_version(v2) else {
            return std::cmp::Ordering::Equal;
        };

        match major1.cmp(&major2) {
            std::cmp::Ordering::Equal => match minor1.cmp(&minor2) {
                std::cmp::Ordering::Equal => patch1.cmp(&patch2),
                other => other,
            },
            other => other,
        }
    }

    /// Check for breaking changes between versions.
    pub fn has_breaking_changes(&self, workflow_id: &str, from: &str, to: &str) -> Result<bool> {
        let from_version = self
            .get_version(workflow_id, from)
            .ok_or_else(|| WorkflowError::not_found(from))?;

        let to_version = self
            .get_version(workflow_id, to)
            .ok_or_else(|| WorkflowError::not_found(to))?;

        Ok(!to_version.metadata.breaking_changes.is_empty()
            && Self::compare_versions(&from_version.version, &to_version.version)
                == std::cmp::Ordering::Less)
    }
}

impl Default for WorkflowVersionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        assert!(WorkflowVersionManager::parse_version("1.0.0").is_ok());
        assert!(WorkflowVersionManager::parse_version("1.2.3").is_ok());
        assert!(WorkflowVersionManager::parse_version("invalid").is_err());
    }

    #[test]
    fn test_version_comparison() {
        use std::cmp::Ordering;

        assert_eq!(
            WorkflowVersionManager::compare_versions("1.0.0", "1.0.0"),
            Ordering::Equal
        );
        assert_eq!(
            WorkflowVersionManager::compare_versions("1.0.0", "2.0.0"),
            Ordering::Less
        );
        assert_eq!(
            WorkflowVersionManager::compare_versions("2.0.0", "1.0.0"),
            Ordering::Greater
        );
        assert_eq!(
            WorkflowVersionManager::compare_versions("1.0.0", "1.1.0"),
            Ordering::Less
        );
    }

    #[test]
    fn test_version_compatibility() {
        let manager = WorkflowVersionManager::new();

        assert!(
            manager
                .is_compatible("1.0.0", "1.1.0")
                .expect("Check failed")
        );
        assert!(
            !manager
                .is_compatible("1.0.0", "2.0.0")
                .expect("Check failed")
        );
    }

    #[test]
    fn test_register_version() {
        use crate::dag::WorkflowDag;

        let manager = WorkflowVersionManager::new();

        let version = WorkflowVersion {
            version: "1.0.0".to_string(),
            definition: WorkflowDefinition {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                version: "1.0.0".to_string(),
                dag: WorkflowDag::new(),
            },
            metadata: VersionMetadata {
                created_at: Utc::now(),
                author: "test".to_string(),
                changelog: vec![],
                breaking_changes: vec![],
                deprecations: vec![],
                tags: vec![],
            },
            previous_version: None,
            migration_notes: None,
        };

        assert!(
            manager
                .register_version("test-workflow".to_string(), version)
                .is_ok()
        );
    }

    #[test]
    fn test_get_latest_version() {
        use crate::dag::WorkflowDag;

        let manager = WorkflowVersionManager::new();

        let v1 = WorkflowVersion {
            version: "1.0.0".to_string(),
            definition: WorkflowDefinition {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                version: "1.0.0".to_string(),
                dag: WorkflowDag::new(),
            },
            metadata: VersionMetadata {
                created_at: Utc::now(),
                author: "test".to_string(),
                changelog: vec![],
                breaking_changes: vec![],
                deprecations: vec![],
                tags: vec![],
            },
            previous_version: None,
            migration_notes: None,
        };

        let v2 = WorkflowVersion {
            version: "2.0.0".to_string(),
            definition: WorkflowDefinition {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                version: "2.0.0".to_string(),
                dag: WorkflowDag::new(),
            },
            metadata: VersionMetadata {
                created_at: Utc::now(),
                author: "test".to_string(),
                changelog: vec![],
                breaking_changes: vec![],
                deprecations: vec![],
                tags: vec![],
            },
            previous_version: Some("1.0.0".to_string()),
            migration_notes: None,
        };

        manager
            .register_version("test".to_string(), v1)
            .expect("Failed");
        manager
            .register_version("test".to_string(), v2)
            .expect("Failed");

        let latest = manager.get_latest_version("test").expect("Not found");
        assert_eq!(latest.version, "2.0.0");
    }
}
