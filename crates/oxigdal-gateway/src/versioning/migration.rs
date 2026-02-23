//! Version migration helpers.
//!
//! Provides tools for migrating requests and responses between API versions.

use crate::error::Result;
use crate::versioning::ApiVersion;
use async_trait::async_trait;
use serde_json::Value;

/// Migration path between two versions.
#[derive(Debug, Clone)]
pub struct MigrationPath {
    /// Source version.
    pub from: ApiVersion,
    /// Target version.
    pub to: ApiVersion,
    /// Migration direction.
    pub direction: MigrationDirection,
}

/// Migration direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationDirection {
    /// Forward migration (upgrade).
    Forward,
    /// Backward migration (downgrade).
    Backward,
}

impl MigrationPath {
    /// Creates a new migration path.
    pub fn new(from: ApiVersion, to: ApiVersion) -> Self {
        let direction = if from < to {
            MigrationDirection::Forward
        } else {
            MigrationDirection::Backward
        };

        Self {
            from,
            to,
            direction,
        }
    }

    /// Checks if this is a forward migration.
    pub fn is_forward(&self) -> bool {
        self.direction == MigrationDirection::Forward
    }

    /// Checks if this is a backward migration.
    pub fn is_backward(&self) -> bool {
        self.direction == MigrationDirection::Backward
    }
}

/// Request migrator trait.
#[async_trait]
pub trait RequestMigrator: Send + Sync {
    /// Migrates request from one version to another.
    async fn migrate_request(&self, request: Value, path: &MigrationPath) -> Result<Value>;
}

/// Response migrator trait.
#[async_trait]
pub trait ResponseMigrator: Send + Sync {
    /// Migrates response from one version to another.
    async fn migrate_response(&self, response: Value, path: &MigrationPath) -> Result<Value>;
}

/// Version migrator managing request and response migrations.
pub struct VersionMigrator {
    request_migrators: Vec<Box<dyn RequestMigrator>>,
    response_migrators: Vec<Box<dyn ResponseMigrator>>,
}

impl VersionMigrator {
    /// Creates a new version migrator.
    pub fn new() -> Self {
        Self {
            request_migrators: Vec::new(),
            response_migrators: Vec::new(),
        }
    }

    /// Adds a request migrator.
    pub fn add_request_migrator(&mut self, migrator: Box<dyn RequestMigrator>) {
        self.request_migrators.push(migrator);
    }

    /// Adds a response migrator.
    pub fn add_response_migrator(&mut self, migrator: Box<dyn ResponseMigrator>) {
        self.response_migrators.push(migrator);
    }

    /// Migrates request through all migrators.
    pub async fn migrate_request(&self, request: Value, path: &MigrationPath) -> Result<Value> {
        let mut current = request;
        for migrator in &self.request_migrators {
            current = migrator.migrate_request(current, path).await?;
        }
        Ok(current)
    }

    /// Migrates response through all migrators.
    pub async fn migrate_response(&self, response: Value, path: &MigrationPath) -> Result<Value> {
        let mut current = response;
        for migrator in &self.response_migrators {
            current = migrator.migrate_response(current, path).await?;
        }
        Ok(current)
    }
}

impl Default for VersionMigrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Field renaming migrator.
pub struct FieldRenameMigrator {
    renames: Vec<(ApiVersion, String, String)>,
}

impl FieldRenameMigrator {
    /// Creates a new field rename migrator.
    pub fn new() -> Self {
        Self {
            renames: Vec::new(),
        }
    }

    /// Adds a field rename rule.
    pub fn add_rename(&mut self, version: ApiVersion, old_name: String, new_name: String) {
        self.renames.push((version, old_name, new_name));
    }

    fn apply_renames(&self, mut value: Value, path: &MigrationPath) -> Value {
        if let Value::Object(ref mut map) = value {
            for (version, old_name, new_name) in &self.renames {
                if path.is_forward() && &path.to >= version {
                    if let Some(v) = map.remove(old_name) {
                        map.insert(new_name.clone(), v);
                    }
                } else if path.is_backward() && &path.from >= version {
                    if let Some(v) = map.remove(new_name) {
                        map.insert(old_name.clone(), v);
                    }
                }
            }
        }
        value
    }
}

impl Default for FieldRenameMigrator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RequestMigrator for FieldRenameMigrator {
    async fn migrate_request(&self, request: Value, path: &MigrationPath) -> Result<Value> {
        Ok(self.apply_renames(request, path))
    }
}

#[async_trait]
impl ResponseMigrator for FieldRenameMigrator {
    async fn migrate_response(&self, response: Value, path: &MigrationPath) -> Result<Value> {
        Ok(self.apply_renames(response, path))
    }
}

/// Field removal migrator.
pub struct FieldRemovalMigrator {
    removals: Vec<(ApiVersion, String)>,
}

impl FieldRemovalMigrator {
    /// Creates a new field removal migrator.
    pub fn new() -> Self {
        Self {
            removals: Vec::new(),
        }
    }

    /// Adds a field removal rule.
    pub fn add_removal(&mut self, version: ApiVersion, field: String) {
        self.removals.push((version, field));
    }

    fn apply_removals(&self, mut value: Value, path: &MigrationPath) -> Value {
        if let Value::Object(ref mut map) = value {
            for (version, field) in &self.removals {
                if path.is_forward() && &path.to >= version {
                    map.remove(field);
                }
            }
        }
        value
    }
}

impl Default for FieldRemovalMigrator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ResponseMigrator for FieldRemovalMigrator {
    async fn migrate_response(&self, response: Value, path: &MigrationPath) -> Result<Value> {
        Ok(self.apply_removals(response, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_migration_path() {
        let v1 = ApiVersion::new(1, 0, 0);
        let v2 = ApiVersion::new(2, 0, 0);

        let forward = MigrationPath::new(v1.clone(), v2.clone());
        assert!(forward.is_forward());
        assert!(!forward.is_backward());

        let backward = MigrationPath::new(v2, v1);
        assert!(backward.is_backward());
        assert!(!backward.is_forward());
    }

    #[tokio::test]
    async fn test_field_rename_migrator_forward() {
        let mut migrator = FieldRenameMigrator::new();
        migrator.add_rename(
            ApiVersion::new(2, 0, 0),
            "old_field".to_string(),
            "new_field".to_string(),
        );

        let request = json!({
            "old_field": "value",
            "other": 123
        });

        let path = MigrationPath::new(ApiVersion::new(1, 0, 0), ApiVersion::new(2, 0, 0));
        let result = migrator.migrate_request(request, &path).await;

        assert!(result.is_ok());
        if let Ok(result) = result {
            assert!(result["new_field"].as_str() == Some("value"));
            assert!(result.get("old_field").is_none());
        }
    }

    #[tokio::test]
    async fn test_field_rename_migrator_backward() {
        let mut migrator = FieldRenameMigrator::new();
        migrator.add_rename(
            ApiVersion::new(2, 0, 0),
            "old_field".to_string(),
            "new_field".to_string(),
        );

        let response = json!({
            "new_field": "value",
            "other": 123
        });

        let path = MigrationPath::new(ApiVersion::new(2, 0, 0), ApiVersion::new(1, 0, 0));
        let result = migrator.migrate_response(response, &path).await;

        assert!(result.is_ok());
        if let Ok(result) = result {
            assert!(result["old_field"].as_str() == Some("value"));
            assert!(result.get("new_field").is_none());
        }
    }

    #[tokio::test]
    async fn test_field_removal_migrator() {
        let mut migrator = FieldRemovalMigrator::new();
        migrator.add_removal(ApiVersion::new(2, 0, 0), "deprecated_field".to_string());

        let response = json!({
            "deprecated_field": "value",
            "current_field": 123
        });

        let path = MigrationPath::new(ApiVersion::new(1, 0, 0), ApiVersion::new(2, 0, 0));
        let result = migrator.migrate_response(response, &path).await;

        assert!(result.is_ok());
        if let Ok(result) = result {
            assert!(result.get("deprecated_field").is_none());
            assert!(result["current_field"].as_i64() == Some(123));
        }
    }
}
