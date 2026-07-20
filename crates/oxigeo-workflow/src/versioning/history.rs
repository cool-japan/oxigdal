//! Version history tracking.
//!
//! Provides comprehensive version history management including:
//! - Full version history with timeline
//! - Version lineage tracking
//! - Audit trail for changes
//! - Version search and filtering

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};
use crate::versioning::{ChangeType, WorkflowVersion};
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// A single entry in the version history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Entry ID.
    pub id: String,
    /// Workflow ID.
    pub workflow_id: String,
    /// Version string.
    pub version: String,
    /// Parent version (if any).
    pub parent_version: Option<String>,
    /// Timestamp of the version creation.
    pub created_at: DateTime<Utc>,
    /// Author of the version.
    pub author: String,
    /// Commit message or description.
    pub message: String,
    /// Type of change.
    pub change_type: HistoryChangeType,
    /// Associated tags.
    pub tags: Vec<String>,
    /// Whether this version is a release.
    pub is_release: bool,
    /// Branch name (if applicable).
    pub branch: Option<String>,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
}

/// Type of change for history entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryChangeType {
    /// Initial version creation.
    Initial,
    /// Feature addition.
    Feature,
    /// Bug fix.
    Fix,
    /// Refactoring.
    Refactor,
    /// Breaking change.
    Breaking,
    /// Hotfix.
    Hotfix,
    /// Merge from another branch.
    Merge,
    /// Revert to previous version.
    Revert,
    /// Configuration change.
    Config,
    /// Deprecation.
    Deprecation,
}

impl From<ChangeType> for HistoryChangeType {
    fn from(change_type: ChangeType) -> Self {
        match change_type {
            ChangeType::Feature => HistoryChangeType::Feature,
            ChangeType::Fix => HistoryChangeType::Fix,
            ChangeType::Performance => HistoryChangeType::Feature,
            ChangeType::Breaking => HistoryChangeType::Breaking,
            ChangeType::Deprecation => HistoryChangeType::Deprecation,
            ChangeType::Documentation => HistoryChangeType::Config,
            ChangeType::Refactor => HistoryChangeType::Refactor,
        }
    }
}

/// Version lineage node for tracking ancestry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageNode {
    /// Version string.
    pub version: String,
    /// Parent versions.
    pub parents: Vec<String>,
    /// Child versions.
    pub children: Vec<String>,
    /// Branch name.
    pub branch: String,
    /// Depth in the lineage tree.
    pub depth: usize,
}

/// Version timeline view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionTimeline {
    /// Workflow ID.
    pub workflow_id: String,
    /// Timeline entries sorted by date.
    pub entries: Vec<TimelineEntry>,
    /// Start date of the timeline.
    pub start_date: DateTime<Utc>,
    /// End date of the timeline.
    pub end_date: DateTime<Utc>,
    /// Total number of versions.
    pub total_versions: usize,
}

/// A single timeline entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// Version string.
    pub version: String,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Author.
    pub author: String,
    /// Message.
    pub message: String,
    /// Change type.
    pub change_type: HistoryChangeType,
    /// Branch.
    pub branch: Option<String>,
    /// Is release.
    pub is_release: bool,
}

/// Search criteria for version history.
#[derive(Debug, Clone, Default)]
pub struct HistorySearchCriteria {
    /// Filter by author.
    pub author: Option<String>,
    /// Filter by date range start.
    pub from_date: Option<DateTime<Utc>>,
    /// Filter by date range end.
    pub to_date: Option<DateTime<Utc>>,
    /// Filter by change type.
    pub change_type: Option<HistoryChangeType>,
    /// Filter by tag.
    pub tag: Option<String>,
    /// Filter by branch.
    pub branch: Option<String>,
    /// Only include releases.
    pub releases_only: bool,
    /// Search in message.
    pub message_contains: Option<String>,
    /// Maximum results.
    pub limit: Option<usize>,
}

impl HistorySearchCriteria {
    /// Create new empty search criteria.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by author.
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Filter by date range.
    pub fn with_date_range(mut self, from: DateTime<Utc>, to: DateTime<Utc>) -> Self {
        self.from_date = Some(from);
        self.to_date = Some(to);
        self
    }

    /// Filter by change type.
    pub fn with_change_type(mut self, change_type: HistoryChangeType) -> Self {
        self.change_type = Some(change_type);
        self
    }

    /// Filter by tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Filter by branch.
    pub fn with_branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = Some(branch.into());
        self
    }

    /// Only include releases.
    pub fn releases_only(mut self) -> Self {
        self.releases_only = true;
        self
    }

    /// Search in message.
    pub fn with_message_contains(mut self, text: impl Into<String>) -> Self {
        self.message_contains = Some(text.into());
        self
    }

    /// Limit results.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Version history manager.
pub struct VersionHistoryManager {
    /// History entries by workflow ID.
    history: Arc<DashMap<String, Vec<HistoryEntry>>>,
    /// Version to entry mapping for quick lookup.
    version_index: Arc<DashMap<String, (String, usize)>>,
    /// Maximum history entries per workflow.
    max_entries_per_workflow: usize,
    /// Retention period for history entries.
    retention_period: Option<Duration>,
}

impl Default for VersionHistoryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionHistoryManager {
    /// Create a new version history manager.
    pub fn new() -> Self {
        Self {
            history: Arc::new(DashMap::new()),
            version_index: Arc::new(DashMap::new()),
            max_entries_per_workflow: 1000,
            retention_period: None,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(max_entries: usize, retention_period: Option<Duration>) -> Self {
        Self {
            history: Arc::new(DashMap::new()),
            version_index: Arc::new(DashMap::new()),
            max_entries_per_workflow: max_entries,
            retention_period,
        }
    }

    /// Add a history entry.
    pub fn add_entry(&self, entry: HistoryEntry) -> Result<()> {
        let workflow_id = entry.workflow_id.clone();
        let version = entry.version.clone();

        // Check if version already exists
        let index_key = format!("{}:{}", workflow_id, version);
        if self.version_index.contains_key(&index_key) {
            return Err(WorkflowError::versioning(format!(
                "Version {} already exists in history for workflow {}",
                version, workflow_id
            )));
        }

        let mut entries = self.history.entry(workflow_id.clone()).or_default();

        // Enforce max entries
        while entries.len() >= self.max_entries_per_workflow {
            if let Some(removed) = entries.pop() {
                let key = format!("{}:{}", workflow_id, removed.version);
                self.version_index.remove(&key);
            }
        }

        // Add new entry
        let index = entries.len();
        entries.push(entry);

        // Update index
        self.version_index.insert(index_key, (workflow_id, index));

        Ok(())
    }

    /// Record a new version in history.
    pub fn record_version(
        &self,
        workflow_version: &WorkflowVersion,
        author: &str,
        message: &str,
        change_type: HistoryChangeType,
    ) -> Result<String> {
        let entry_id = uuid::Uuid::new_v4().to_string();

        let entry = HistoryEntry {
            id: entry_id.clone(),
            workflow_id: workflow_version.definition.id.clone(),
            version: workflow_version.version.clone(),
            parent_version: workflow_version.previous_version.clone(),
            created_at: workflow_version.metadata.created_at,
            author: author.to_string(),
            message: message.to_string(),
            change_type,
            tags: workflow_version.metadata.tags.clone(),
            is_release: false,
            branch: None,
            metadata: HashMap::new(),
        };

        self.add_entry(entry)?;
        Ok(entry_id)
    }

    /// Get history for a workflow.
    pub fn get_history(&self, workflow_id: &str) -> Vec<HistoryEntry> {
        self.history
            .get(workflow_id)
            .map(|entries| entries.clone())
            .unwrap_or_default()
    }

    /// Get a specific history entry by version.
    pub fn get_entry(&self, workflow_id: &str, version: &str) -> Option<HistoryEntry> {
        let index_key = format!("{}:{}", workflow_id, version);

        self.version_index.get(&index_key).and_then(|index_info| {
            let (wf_id, idx) = index_info.value();
            self.history.get(wf_id).and_then(|entries| entries.get(*idx).cloned())
        })
    }

    /// Search history entries.
    pub fn search(&self, workflow_id: &str, criteria: &HistorySearchCriteria) -> Vec<HistoryEntry> {
        let entries = self.get_history(workflow_id);

        let filtered: Vec<HistoryEntry> = entries
            .into_iter()
            .filter(|entry| self.matches_criteria(entry, criteria))
            .collect();

        match criteria.limit {
            Some(limit) => filtered.into_iter().take(limit).collect(),
            None => filtered,
        }
    }

    /// Check if an entry matches the search criteria.
    fn matches_criteria(&self, entry: &HistoryEntry, criteria: &HistorySearchCriteria) -> bool {
        // Author filter
        if let Some(ref author) = criteria.author {
            if &entry.author != author {
                return false;
            }
        }

        // Date range filter
        if let Some(from_date) = criteria.from_date {
            if entry.created_at < from_date {
                return false;
            }
        }
        if let Some(to_date) = criteria.to_date {
            if entry.created_at > to_date {
                return false;
            }
        }

        // Change type filter
        if let Some(change_type) = criteria.change_type {
            if entry.change_type != change_type {
                return false;
            }
        }

        // Tag filter
        if let Some(ref tag) = criteria.tag {
            if !entry.tags.contains(tag) {
                return false;
            }
        }

        // Branch filter
        if let Some(ref branch) = criteria.branch {
            match &entry.branch {
                Some(entry_branch) if entry_branch == branch => {}
                _ => return false,
            }
        }

        // Releases only filter
        if criteria.releases_only && !entry.is_release {
            return false;
        }

        // Message contains filter
        if let Some(ref text) = criteria.message_contains {
            if !entry.message.to_lowercase().contains(&text.to_lowercase()) {
                return false;
            }
        }

        true
    }

    /// Get the version timeline for a workflow.
    pub fn get_timeline(&self, workflow_id: &str) -> VersionTimeline {
        let entries = self.get_history(workflow_id);

        let mut sorted_entries = entries.clone();
        sorted_entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        let timeline_entries: Vec<TimelineEntry> = sorted_entries
            .iter()
            .map(|e| TimelineEntry {
                version: e.version.clone(),
                timestamp: e.created_at,
                author: e.author.clone(),
                message: e.message.clone(),
                change_type: e.change_type,
                branch: e.branch.clone(),
                is_release: e.is_release,
            })
            .collect();

        let start_date = sorted_entries
            .first()
            .map(|e| e.created_at)
            .unwrap_or_else(Utc::now);

        let end_date = sorted_entries
            .last()
            .map(|e| e.created_at)
            .unwrap_or_else(Utc::now);

        VersionTimeline {
            workflow_id: workflow_id.to_string(),
            entries: timeline_entries,
            start_date,
            end_date,
            total_versions: entries.len(),
        }
    }

    /// Build the version lineage tree.
    pub fn build_lineage(&self, workflow_id: &str) -> HashMap<String, LineageNode> {
        let entries = self.get_history(workflow_id);
        let mut lineage: HashMap<String, LineageNode> = HashMap::new();

        // First pass: create all nodes
        for entry in &entries {
            lineage.insert(
                entry.version.clone(),
                LineageNode {
                    version: entry.version.clone(),
                    parents: entry.parent_version.clone().into_iter().collect(),
                    children: Vec::new(),
                    branch: entry.branch.clone().unwrap_or_else(|| "main".to_string()),
                    depth: 0,
                },
            );
        }

        // Second pass: link children
        let parent_child_pairs: Vec<(String, String)> = entries
            .iter()
            .filter_map(|e| {
                e.parent_version
                    .as_ref()
                    .map(|p| (p.clone(), e.version.clone()))
            })
            .collect();

        for (parent, child) in parent_child_pairs {
            if let Some(node) = lineage.get_mut(&parent) {
                node.children.push(child);
            }
        }

        // Third pass: calculate depths using BFS
        self.calculate_depths(&mut lineage);

        lineage
    }

    /// Calculate depths in the lineage tree using BFS.
    fn calculate_depths(&self, lineage: &mut HashMap<String, LineageNode>) {
        // Find root nodes (no parents)
        let roots: Vec<String> = lineage
            .iter()
            .filter(|(_, node)| node.parents.is_empty())
            .map(|(v, _)| v.clone())
            .collect();

        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        for root in roots {
            queue.push_back((root, 0));
        }

        while let Some((version, depth)) = queue.pop_front() {
            if let Some(node) = lineage.get_mut(&version) {
                node.depth = depth;
                for child in node.children.clone() {
                    queue.push_back((child, depth + 1));
                }
            }
        }
    }

    /// Get the ancestry path from a version to the root.
    pub fn get_ancestry(&self, workflow_id: &str, version: &str) -> Vec<String> {
        let lineage = self.build_lineage(workflow_id);
        let mut path = Vec::new();
        let mut current = Some(version.to_string());

        while let Some(v) = current {
            path.push(v.clone());
            current = lineage
                .get(&v)
                .and_then(|node| node.parents.first().cloned());
        }

        path
    }

    /// Find the common ancestor of two versions.
    pub fn find_common_ancestor(
        &self,
        workflow_id: &str,
        version1: &str,
        version2: &str,
    ) -> Option<String> {
        let ancestry1 = self.get_ancestry(workflow_id, version1);
        let ancestry2: std::collections::HashSet<String> =
            self.get_ancestry(workflow_id, version2).into_iter().collect();

        ancestry1.into_iter().find(|v| ancestry2.contains(v))
    }

    /// Mark a version as a release.
    pub fn mark_as_release(&self, workflow_id: &str, version: &str) -> Result<()> {
        let index_key = format!("{}:{}", workflow_id, version);

        if let Some(index_info) = self.version_index.get(&index_key) {
            let (wf_id, idx) = index_info.value().clone();
            if let Some(mut entries) = self.history.get_mut(&wf_id) {
                if let Some(entry) = entries.get_mut(idx) {
                    entry.is_release = true;
                    return Ok(());
                }
            }
        }

        Err(WorkflowError::not_found(format!(
            "Version {} not found for workflow {}",
            version, workflow_id
        )))
    }

    /// Add a tag to a version.
    pub fn add_tag(&self, workflow_id: &str, version: &str, tag: &str) -> Result<()> {
        let index_key = format!("{}:{}", workflow_id, version);

        if let Some(index_info) = self.version_index.get(&index_key) {
            let (wf_id, idx) = index_info.value().clone();
            if let Some(mut entries) = self.history.get_mut(&wf_id) {
                if let Some(entry) = entries.get_mut(idx) {
                    if !entry.tags.contains(&tag.to_string()) {
                        entry.tags.push(tag.to_string());
                    }
                    return Ok(());
                }
            }
        }

        Err(WorkflowError::not_found(format!(
            "Version {} not found for workflow {}",
            version, workflow_id
        )))
    }

    /// Get all releases for a workflow.
    pub fn get_releases(&self, workflow_id: &str) -> Vec<HistoryEntry> {
        self.search(workflow_id, &HistorySearchCriteria::new().releases_only())
    }

    /// Get recent history entries.
    pub fn get_recent(&self, workflow_id: &str, count: usize) -> Vec<HistoryEntry> {
        let mut entries = self.get_history(workflow_id);
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        entries.into_iter().take(count).collect()
    }

    /// Clean up old history entries based on retention period.
    pub fn cleanup_old_entries(&self) -> usize {
        let Some(retention) = self.retention_period else {
            return 0;
        };

        let cutoff = Utc::now() - retention;
        let mut removed = 0;

        for mut entry in self.history.iter_mut() {
            let original_len = entry.len();
            entry.retain(|e| e.created_at >= cutoff);
            removed += original_len - entry.len();
        }

        // Rebuild index after cleanup
        self.rebuild_index();

        removed
    }

    /// Rebuild the version index.
    fn rebuild_index(&self) {
        self.version_index.clear();

        for entry in self.history.iter() {
            let workflow_id = entry.key();
            for (idx, history_entry) in entry.iter().enumerate() {
                let index_key = format!("{}:{}", workflow_id, history_entry.version);
                self.version_index
                    .insert(index_key, (workflow_id.clone(), idx));
            }
        }
    }

    /// Get statistics for a workflow's history.
    pub fn get_statistics(&self, workflow_id: &str) -> HistoryStatistics {
        let entries = self.get_history(workflow_id);

        let mut stats = HistoryStatistics::default();
        stats.total_versions = entries.len();

        for entry in &entries {
            match entry.change_type {
                HistoryChangeType::Feature => stats.features += 1,
                HistoryChangeType::Fix | HistoryChangeType::Hotfix => stats.fixes += 1,
                HistoryChangeType::Breaking => stats.breaking_changes += 1,
                HistoryChangeType::Refactor => stats.refactors += 1,
                _ => {}
            }

            if entry.is_release {
                stats.releases += 1;
            }
        }

        // Calculate average time between versions
        if entries.len() >= 2 {
            let mut sorted = entries.clone();
            sorted.sort_by(|a, b| a.created_at.cmp(&b.created_at));

            let total_duration: i64 = sorted
                .windows(2)
                .map(|w| (w[1].created_at - w[0].created_at).num_seconds())
                .sum();

            stats.avg_time_between_versions_secs = total_duration / (entries.len() as i64 - 1);
        }

        stats
    }
}

/// Statistics for version history.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryStatistics {
    /// Total number of versions.
    pub total_versions: usize,
    /// Number of releases.
    pub releases: usize,
    /// Number of feature additions.
    pub features: usize,
    /// Number of bug fixes.
    pub fixes: usize,
    /// Number of breaking changes.
    pub breaking_changes: usize,
    /// Number of refactors.
    pub refactors: usize,
    /// Average time between versions in seconds.
    pub avg_time_between_versions_secs: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_entry(workflow_id: &str, version: &str, parent: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            workflow_id: workflow_id.to_string(),
            version: version.to_string(),
            parent_version: parent.map(String::from),
            created_at: Utc::now(),
            author: "test-author".to_string(),
            message: format!("Version {}", version),
            change_type: HistoryChangeType::Feature,
            tags: vec![],
            is_release: false,
            branch: Some("main".to_string()),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_add_and_get_entry() {
        let manager = VersionHistoryManager::new();

        let entry = create_test_entry("wf1", "1.0.0", None);
        manager.add_entry(entry.clone()).expect("Failed to add entry");

        let retrieved = manager.get_entry("wf1", "1.0.0");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.as_ref().map(|e| &e.version), Some(&"1.0.0".to_string()));
    }

    #[test]
    fn test_get_history() {
        let manager = VersionHistoryManager::new();

        manager.add_entry(create_test_entry("wf1", "1.0.0", None)).ok();
        manager.add_entry(create_test_entry("wf1", "1.1.0", Some("1.0.0"))).ok();
        manager.add_entry(create_test_entry("wf1", "1.2.0", Some("1.1.0"))).ok();

        let history = manager.get_history("wf1");
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_search() {
        let manager = VersionHistoryManager::new();

        let mut entry1 = create_test_entry("wf1", "1.0.0", None);
        entry1.author = "alice".to_string();
        manager.add_entry(entry1).ok();

        let mut entry2 = create_test_entry("wf1", "1.1.0", Some("1.0.0"));
        entry2.author = "bob".to_string();
        manager.add_entry(entry2).ok();

        let criteria = HistorySearchCriteria::new().with_author("alice");
        let results = manager.search("wf1", &criteria);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].author, "alice");
    }

    #[test]
    fn test_build_lineage() {
        let manager = VersionHistoryManager::new();

        manager.add_entry(create_test_entry("wf1", "1.0.0", None)).ok();
        manager.add_entry(create_test_entry("wf1", "1.1.0", Some("1.0.0"))).ok();
        manager.add_entry(create_test_entry("wf1", "2.0.0", Some("1.1.0"))).ok();

        let lineage = manager.build_lineage("wf1");

        assert_eq!(lineage.len(), 3);
        assert!(lineage.get("1.0.0").map(|n| n.parents.is_empty()).unwrap_or(false));
        assert!(lineage.get("2.0.0").map(|n| n.depth == 2).unwrap_or(false));
    }

    #[test]
    fn test_get_ancestry() {
        let manager = VersionHistoryManager::new();

        manager.add_entry(create_test_entry("wf1", "1.0.0", None)).ok();
        manager.add_entry(create_test_entry("wf1", "1.1.0", Some("1.0.0"))).ok();
        manager.add_entry(create_test_entry("wf1", "1.2.0", Some("1.1.0"))).ok();

        let ancestry = manager.get_ancestry("wf1", "1.2.0");

        assert_eq!(ancestry, vec!["1.2.0", "1.1.0", "1.0.0"]);
    }

    #[test]
    fn test_find_common_ancestor() {
        let manager = VersionHistoryManager::new();

        manager.add_entry(create_test_entry("wf1", "1.0.0", None)).ok();
        manager.add_entry(create_test_entry("wf1", "1.1.0", Some("1.0.0"))).ok();
        manager.add_entry(create_test_entry("wf1", "1.2.0", Some("1.0.0"))).ok();

        let ancestor = manager.find_common_ancestor("wf1", "1.1.0", "1.2.0");

        assert_eq!(ancestor, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_mark_as_release() {
        let manager = VersionHistoryManager::new();

        manager.add_entry(create_test_entry("wf1", "1.0.0", None)).ok();
        manager.mark_as_release("wf1", "1.0.0").ok();

        let entry = manager.get_entry("wf1", "1.0.0");
        assert!(entry.map(|e| e.is_release).unwrap_or(false));
    }

    #[test]
    fn test_get_statistics() {
        let manager = VersionHistoryManager::new();

        let mut entry1 = create_test_entry("wf1", "1.0.0", None);
        entry1.change_type = HistoryChangeType::Initial;
        manager.add_entry(entry1).ok();

        let mut entry2 = create_test_entry("wf1", "1.1.0", Some("1.0.0"));
        entry2.change_type = HistoryChangeType::Feature;
        manager.add_entry(entry2).ok();

        let mut entry3 = create_test_entry("wf1", "1.1.1", Some("1.1.0"));
        entry3.change_type = HistoryChangeType::Fix;
        manager.add_entry(entry3).ok();

        let stats = manager.get_statistics("wf1");

        assert_eq!(stats.total_versions, 3);
        assert_eq!(stats.features, 1);
        assert_eq!(stats.fixes, 1);
    }
}
