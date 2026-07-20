//! Query result caching.

use crate::executor::scan::RecordBatch;
use crate::parser::ast::Statement;
use blake3::Hash;
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Query cache.
pub struct QueryCache {
    /// Cache entries.
    entries: DashMap<Hash, CacheEntry>,
    /// Configuration.
    config: CacheConfig,
    /// Statistics.
    stats: Arc<RwLock<CacheStatistics>>,
}

/// Cache configuration.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum cache size in bytes.
    pub max_size_bytes: usize,
    /// Time-to-live for cache entries.
    pub ttl: Duration,
    /// Enable cache.
    pub enabled: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: 1024 * 1024 * 1024, // 1 GB
            ttl: Duration::from_secs(300),      // 5 minutes
            enabled: true,
        }
    }
}

/// Cache entry.
#[derive(Clone)]
struct CacheEntry {
    /// Cached result.
    result: Arc<Vec<RecordBatch>>,
    /// Creation time.
    created_at: Instant,
    /// Size in bytes (approximate).
    size_bytes: usize,
    /// Access count.
    access_count: usize,
}

impl CacheEntry {
    fn new(result: Vec<RecordBatch>) -> Self {
        let size_bytes = Self::estimate_size(&result);
        Self {
            result: Arc::new(result),
            created_at: Instant::now(),
            size_bytes,
            access_count: 0,
        }
    }

    fn estimate_size(batches: &[RecordBatch]) -> usize {
        batches
            .iter()
            .map(|batch| batch.num_rows * 100)
            .sum::<usize>()
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }
}

impl QueryCache {
    /// Create a new query cache.
    pub fn new(config: CacheConfig) -> Self {
        Self {
            entries: DashMap::new(),
            config,
            stats: Arc::new(RwLock::new(CacheStatistics::default())),
        }
    }

    /// Get cached result.
    pub fn get(&self, query: &Statement) -> Option<Vec<RecordBatch>> {
        if !self.config.enabled {
            return None;
        }

        let key = self.compute_key(query);

        if let Some(mut entry) = self.entries.get_mut(&key) {
            if entry.is_expired(self.config.ttl) {
                drop(entry);
                self.entries.remove(&key);
                self.stats.write().misses += 1;
                return None;
            }

            entry.access_count += 1;
            let result = (*entry.result).clone();
            self.stats.write().hits += 1;
            Some(result)
        } else {
            self.stats.write().misses += 1;
            None
        }
    }

    /// Put result in cache.
    pub fn put(&self, query: &Statement, result: Vec<RecordBatch>) {
        if !self.config.enabled {
            return;
        }

        let key = self.compute_key(query);
        let entry = CacheEntry::new(result);

        // Check cache size limit
        self.evict_if_needed(entry.size_bytes);

        self.entries.insert(key, entry);
        self.stats.write().inserts += 1;
    }

    /// Invalidate cache entry.
    pub fn invalidate(&self, query: &Statement) {
        let key = self.compute_key(query);
        self.entries.remove(&key);
    }

    /// Clear all cache entries.
    pub fn clear(&self) {
        self.entries.clear();
        self.stats.write().clears += 1;
    }

    /// Get cache statistics.
    pub fn statistics(&self) -> CacheStatistics {
        *self.stats.read()
    }

    /// Compute cache key from query.
    fn compute_key(&self, query: &Statement) -> Hash {
        let query_string = format!("{:?}", query);
        blake3::hash(query_string.as_bytes())
    }

    /// Evict entries if cache is too large.
    fn evict_if_needed(&self, incoming_size: usize) {
        let mut current_size: usize = self
            .entries
            .iter()
            .map(|entry| entry.value().size_bytes)
            .sum();

        if current_size + incoming_size <= self.config.max_size_bytes {
            return;
        }

        // Evict least recently used entries
        let mut entries: Vec<_> = self
            .entries
            .iter()
            .map(|entry| {
                (
                    *entry.key(),
                    entry.value().created_at,
                    entry.value().access_count,
                    entry.value().size_bytes,
                )
            })
            .collect();

        entries.sort_by_key(|(_, created, access_count, _)| {
            (created.elapsed().as_secs(), *access_count)
        });

        for (key, _, _, size) in entries {
            self.entries.remove(&key);
            current_size -= size;
            self.stats.write().evictions += 1;

            if current_size + incoming_size <= self.config.max_size_bytes {
                break;
            }
        }
    }
}

/// Cache statistics.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CacheStatistics {
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of inserts.
    pub inserts: u64,
    /// Number of evictions.
    pub evictions: u64,
    /// Number of cache clears.
    pub clears: u64,
}

impl CacheStatistics {
    /// Get hit rate.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Get miss rate.
    pub fn miss_rate(&self) -> f64 {
        1.0 - self.hit_rate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::scan::{ColumnData, DataType, Field, Schema};
    use crate::parser::sql::parse_sql;

    #[test]
    fn test_cache_put_get() {
        let config = CacheConfig::default();
        let cache = QueryCache::new(config);

        let query = parse_sql("SELECT * FROM test").ok().unwrap_or_else(|| {
            Statement::Select(crate::parser::ast::SelectStatement {
                projection: vec![],
                from: None,
                selection: None,
                group_by: vec![],
                having: None,
                order_by: vec![],
                limit: None,
                offset: None,
            })
        });

        let schema = Arc::new(Schema::new(vec![Field::new(
            "id".to_string(),
            DataType::Int64,
            false,
        )]));

        let columns = vec![ColumnData::Int64(vec![Some(1), Some(2)])];
        let batch = RecordBatch::new(schema, columns, 2).ok();

        if let Some(batch) = batch {
            let result = vec![batch];

            cache.put(&query, result.clone());

            let cached = cache.get(&query);
            assert!(cached.is_some());
        }
    }

    #[test]
    fn test_cache_statistics() {
        let config = CacheConfig::default();
        let cache = QueryCache::new(config);

        let query = parse_sql("SELECT * FROM test").ok().unwrap_or_else(|| {
            Statement::Select(crate::parser::ast::SelectStatement {
                projection: vec![],
                from: None,
                selection: None,
                group_by: vec![],
                having: None,
                order_by: vec![],
                limit: None,
                offset: None,
            })
        });

        // Miss
        let _ = cache.get(&query);

        let stats = cache.statistics();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 1);
    }
}
