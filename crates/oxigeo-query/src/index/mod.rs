//! Index metadata and selection.

pub mod selector;

use crate::optimizer::cost_model::{IndexStatistics, IndexType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Index metadata registry.
pub struct IndexRegistry {
    /// Indexes by table name.
    indexes: HashMap<String, Vec<Index>>,
}

impl IndexRegistry {
    /// Create a new index registry.
    pub fn new() -> Self {
        Self {
            indexes: HashMap::new(),
        }
    }

    /// Register an index.
    pub fn register_index(&mut self, table: String, index: Index) {
        self.indexes.entry(table).or_default().push(index);
    }

    /// Get indexes for a table.
    pub fn get_indexes(&self, table: &str) -> Option<&[Index]> {
        self.indexes.get(table).map(|v| v.as_slice())
    }

    /// Get index by name.
    pub fn get_index(&self, table: &str, name: &str) -> Option<&Index> {
        self.indexes.get(table)?.iter().find(|idx| idx.name == name)
    }
}

impl Default for IndexRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Index metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    /// Index name.
    pub name: String,
    /// Table name.
    pub table: String,
    /// Indexed columns.
    pub columns: Vec<String>,
    /// Index type.
    pub index_type: IndexType,
    /// Index statistics.
    pub statistics: IndexStatistics,
    /// Whether the index is unique.
    pub is_unique: bool,
    /// Whether the index is primary key.
    pub is_primary: bool,
}

impl Index {
    /// Create a new index.
    pub fn new(
        name: String,
        table: String,
        columns: Vec<String>,
        index_type: IndexType,
        statistics: IndexStatistics,
    ) -> Self {
        Self {
            name,
            table,
            columns,
            index_type,
            statistics,
            is_unique: false,
            is_primary: false,
        }
    }

    /// Mark as unique index.
    pub fn with_unique(mut self) -> Self {
        self.is_unique = true;
        self
    }

    /// Mark as primary key.
    pub fn with_primary(mut self) -> Self {
        self.is_primary = true;
        self.is_unique = true;
        self
    }

    /// Check if index covers columns.
    pub fn covers_columns(&self, columns: &[String]) -> bool {
        columns.iter().all(|col| self.columns.contains(col))
    }

    /// Check if index can be used for prefix match.
    pub fn supports_prefix(&self, columns: &[String]) -> bool {
        if columns.is_empty() {
            return false;
        }

        for (i, col) in columns.iter().enumerate() {
            if i >= self.columns.len() || &self.columns[i] != col {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_registry() {
        let mut registry = IndexRegistry::new();

        let idx_stats = IndexStatistics::new(
            "idx_id".to_string(),
            vec!["id".to_string()],
            IndexType::BTree,
            10000,
        );

        let index = Index::new(
            "idx_id".to_string(),
            "users".to_string(),
            vec!["id".to_string()],
            IndexType::BTree,
            idx_stats,
        );

        registry.register_index("users".to_string(), index);

        let indexes = registry.get_indexes("users");
        assert!(indexes.is_some());
        assert_eq!(indexes.as_ref().map(|i| i.len()), Some(1));
    }

    #[test]
    fn test_index_covers_columns() {
        let idx_stats = IndexStatistics::new(
            "idx_name_age".to_string(),
            vec!["name".to_string(), "age".to_string()],
            IndexType::BTree,
            10000,
        );

        let index = Index::new(
            "idx_name_age".to_string(),
            "users".to_string(),
            vec!["name".to_string(), "age".to_string()],
            IndexType::BTree,
            idx_stats,
        );

        assert!(index.covers_columns(&["name".to_string(), "age".to_string()]));
        assert!(index.covers_columns(&["name".to_string()]));
        assert!(!index.covers_columns(&["email".to_string()]));
    }

    #[test]
    fn test_index_supports_prefix() {
        let idx_stats = IndexStatistics::new(
            "idx_a_b_c".to_string(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            IndexType::BTree,
            10000,
        );

        let index = Index::new(
            "idx_a_b_c".to_string(),
            "table".to_string(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            IndexType::BTree,
            idx_stats,
        );

        assert!(index.supports_prefix(&["a".to_string()]));
        assert!(index.supports_prefix(&["a".to_string(), "b".to_string()]));
        assert!(!index.supports_prefix(&["b".to_string()]));
        assert!(!index.supports_prefix(&["a".to_string(), "c".to_string()]));
    }
}
