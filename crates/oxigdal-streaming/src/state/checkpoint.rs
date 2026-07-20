//! Checkpointing for fault tolerance.

use crate::error::{Result, StreamingError};
use crate::state::operator_state::DynOperatorState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Checkpoint metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    /// Checkpoint ID
    pub id: u64,

    /// Checkpoint timestamp
    pub timestamp: DateTime<Utc>,

    /// Checkpoint size in bytes
    pub size_bytes: usize,

    /// State of operators
    pub operator_states: HashMap<String, Vec<u8>>,

    /// Success status
    pub success: bool,

    /// Duration to complete
    pub duration: Duration,
}

/// Checkpoint barrier.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CheckpointBarrier {
    /// Checkpoint ID
    pub id: u64,

    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

impl CheckpointBarrier {
    /// Create a new checkpoint barrier.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            timestamp: Utc::now(),
        }
    }
}

/// Checkpoint configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    /// Checkpoint interval
    pub interval: Duration,

    /// Minimum pause between checkpoints
    pub min_pause: Duration,

    /// Maximum concurrent checkpoints
    pub max_concurrent: usize,

    /// Enable unaligned checkpoints
    pub unaligned: bool,

    /// Checkpoint timeout
    pub timeout: Duration,

    /// Storage path
    pub storage_path: Option<PathBuf>,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            min_pause: Duration::from_secs(10),
            max_concurrent: 1,
            unaligned: false,
            timeout: Duration::from_secs(300),
            storage_path: None,
        }
    }
}

/// Checkpoint storage.
pub trait CheckpointStorage: Send + Sync {
    /// Store a checkpoint.
    fn store(&self, checkpoint: &Checkpoint) -> Result<()>;

    /// Load a checkpoint.
    fn load(&self, checkpoint_id: u64) -> Result<Option<Checkpoint>>;

    /// Delete a checkpoint.
    fn delete(&self, checkpoint_id: u64) -> Result<()>;

    /// List all checkpoints.
    fn list(&self) -> Result<Vec<u64>>;

    /// Get the latest checkpoint ID.
    fn latest(&self) -> Result<Option<u64>>;
}

/// File-system backed checkpoint storage.
///
/// Persists each [`Checkpoint`] to its own file under a root directory using
/// the Pure-Rust `oxicode` binary codec (no C FFI, no external services). This
/// is a real, durable [`CheckpointStorage`] implementation suitable for local
/// fault-tolerant recovery.
pub struct FileCheckpointStorage {
    root: PathBuf,
}

impl FileCheckpointStorage {
    /// Create a new file-backed checkpoint store rooted at `root`.
    ///
    /// The directory (and any missing parents) is created if it does not exist.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Path of the on-disk file for a given checkpoint id.
    fn checkpoint_path(&self, checkpoint_id: u64) -> PathBuf {
        self.root.join(format!("checkpoint-{checkpoint_id}.ckpt"))
    }

    /// Parse a checkpoint id out of a stored file name, if it is one of ours.
    fn parse_id(name: &Path) -> Option<u64> {
        let name = name.file_name()?.to_str()?;
        let rest = name.strip_prefix("checkpoint-")?;
        let id = rest.strip_suffix(".ckpt")?;
        id.parse::<u64>().ok()
    }
}

impl CheckpointStorage for FileCheckpointStorage {
    fn store(&self, checkpoint: &Checkpoint) -> Result<()> {
        // serde_json (Pure Rust) is used here rather than the oxicode binary
        // codec because the checkpoint carries `chrono::DateTime`/`Duration`
        // fields that lack `oxicode::Encode` impls; JSON round-trips them via
        // their serde implementations.
        let bytes = serde_json::to_vec(checkpoint)?;
        // Write to a temp file then rename for atomic replacement.
        let final_path = self.checkpoint_path(checkpoint.id());
        let tmp_path = final_path.with_extension("ckpt.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }

    fn load(&self, checkpoint_id: u64) -> Result<Option<Checkpoint>> {
        let path = self.checkpoint_path(checkpoint_id);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let checkpoint: Checkpoint = serde_json::from_slice(&bytes)?;
                Ok(Some(checkpoint))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, checkpoint_id: u64) -> Result<()> {
        let path = self.checkpoint_path(checkpoint_id);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self) -> Result<Vec<u64>> {
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if let Some(id) = Self::parse_id(&entry.path()) {
                ids.push(id);
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }

    fn latest(&self) -> Result<Option<u64>> {
        Ok(self.list()?.into_iter().max())
    }
}

/// In-memory checkpoint implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Metadata
    pub metadata: CheckpointMetadata,

    /// Actual checkpoint data
    pub data: Vec<u8>,
}

impl Checkpoint {
    /// Create a new checkpoint.
    pub fn new(id: u64, data: Vec<u8>) -> Self {
        let size_bytes = data.len();
        Self {
            metadata: CheckpointMetadata {
                id,
                timestamp: Utc::now(),
                size_bytes,
                operator_states: HashMap::new(),
                success: true,
                duration: Duration::ZERO,
            },
            data,
        }
    }

    /// Get the checkpoint ID.
    pub fn id(&self) -> u64 {
        self.metadata.id
    }

    /// Get the checkpoint size.
    pub fn size(&self) -> usize {
        self.metadata.size_bytes
    }
}

/// Checkpoint coordinator.
pub struct CheckpointCoordinator {
    config: CheckpointConfig,
    next_checkpoint_id: Arc<RwLock<u64>>,
    active_checkpoints: Arc<RwLock<HashMap<u64, CheckpointMetadata>>>,
    completed_checkpoints: Arc<RwLock<Vec<u64>>>,
    last_checkpoint_time: Arc<RwLock<Option<DateTime<Utc>>>>,
    /// Registered operator states captured on every checkpoint, keyed by name.
    operators: Arc<RwLock<HashMap<String, Arc<dyn DynOperatorState>>>>,
    /// Optional durable storage for persisting checkpoints.
    storage: Option<Arc<dyn CheckpointStorage>>,
}

impl CheckpointCoordinator {
    /// Create a new checkpoint coordinator without durable storage.
    ///
    /// Checkpoints still capture registered operator state into their metadata
    /// but are not persisted to disk. Use [`CheckpointCoordinator::with_storage`]
    /// (or a `storage_path` in the config) for durable, recoverable checkpoints.
    pub fn new(config: CheckpointConfig) -> Self {
        Self {
            config,
            next_checkpoint_id: Arc::new(RwLock::new(0)),
            active_checkpoints: Arc::new(RwLock::new(HashMap::new())),
            completed_checkpoints: Arc::new(RwLock::new(Vec::new())),
            last_checkpoint_time: Arc::new(RwLock::new(None)),
            operators: Arc::new(RwLock::new(HashMap::new())),
            storage: None,
        }
    }

    /// Create a coordinator backed by a durable [`CheckpointStorage`].
    pub fn with_storage(config: CheckpointConfig, storage: Arc<dyn CheckpointStorage>) -> Self {
        let mut coordinator = Self::new(config);
        coordinator.storage = Some(storage);
        coordinator
    }

    /// Create a coordinator whose storage is derived from `config.storage_path`.
    ///
    /// When `config.storage_path` is set, a [`FileCheckpointStorage`] rooted at
    /// that path is created (the directory is created if missing). When it is
    /// `None`, this is equivalent to [`CheckpointCoordinator::new`].
    pub fn from_config(config: CheckpointConfig) -> Result<Self> {
        match &config.storage_path {
            Some(path) => {
                let storage = Arc::new(FileCheckpointStorage::new(path.clone())?);
                Ok(Self::with_storage(config, storage))
            }
            None => Ok(Self::new(config)),
        }
    }

    /// Register an operator state to be captured on every checkpoint.
    ///
    /// The `name` uniquely identifies the operator across snapshot/restore; the
    /// same name must be used when restoring so the correct bytes are routed
    /// back to the operator. Registering an existing name replaces it.
    pub async fn register_operator(
        &self,
        name: impl Into<String>,
        state: Arc<dyn DynOperatorState>,
    ) {
        self.operators.write().await.insert(name.into(), state);
    }

    /// Number of registered operators.
    pub async fn operator_count(&self) -> usize {
        self.operators.read().await.len()
    }

    /// Trigger a new checkpoint, capturing the state of all registered operators.
    ///
    /// This snapshots every registered [`DynOperatorState`], aggregating the
    /// captured bytes into the checkpoint metadata (`operator_states`) and
    /// recording the real total `size_bytes`. The checkpoint is left *active*
    /// (not yet completed/persisted); call
    /// [`CheckpointCoordinator::complete_checkpoint`] to finalize it, or use the
    /// combined [`CheckpointCoordinator::checkpoint`] which triggers, persists,
    /// and completes in one step.
    ///
    /// If any operator snapshot fails, no active checkpoint is registered and
    /// the error is propagated — a failed capture never masquerades as success.
    pub async fn trigger_checkpoint(&self) -> Result<u64> {
        let now = Utc::now();
        let last_time = *self.last_checkpoint_time.read().await;

        if let Some(last) = last_time {
            let min_pause_chrono = match chrono::Duration::from_std(self.config.min_pause) {
                Ok(duration) => duration,
                Err(_) => chrono::Duration::zero(),
            };

            if now - last < min_pause_chrono {
                return Err(StreamingError::CheckpointError(
                    "Minimum pause not elapsed".to_string(),
                ));
            }
        }

        let active_count = self.active_checkpoints.read().await.len();
        if active_count >= self.config.max_concurrent {
            return Err(StreamingError::CheckpointError(
                "Too many concurrent checkpoints".to_string(),
            ));
        }

        // Capture the state of every registered operator. Clone the handles out
        // of the registry first so we do not hold the lock across the snapshot
        // awaits (which could block operator registration or deadlock).
        let handles: Vec<(String, Arc<dyn DynOperatorState>)> = {
            let operators = self.operators.read().await;
            operators
                .iter()
                .map(|(name, state)| (name.clone(), state.clone()))
                .collect()
        };

        let mut operator_states = HashMap::with_capacity(handles.len());
        let mut size_bytes = 0usize;
        for (name, state) in handles {
            let bytes = state.snapshot_boxed().await?;
            size_bytes = size_bytes.saturating_add(bytes.len());
            operator_states.insert(name, bytes);
        }

        let mut next_id = self.next_checkpoint_id.write().await;
        let checkpoint_id = *next_id;
        *next_id += 1;

        let metadata = CheckpointMetadata {
            id: checkpoint_id,
            timestamp: now,
            size_bytes,
            operator_states,
            success: false,
            duration: Duration::ZERO,
        };

        self.active_checkpoints
            .write()
            .await
            .insert(checkpoint_id, metadata);

        *self.last_checkpoint_time.write().await = Some(now);

        Ok(checkpoint_id)
    }

    /// Persist a currently-active checkpoint via the configured storage.
    ///
    /// Serializes the captured operator states into a [`Checkpoint`] and writes
    /// it through [`CheckpointStorage`]. When no storage is configured this is a
    /// no-op (in-memory checkpointing). The blocking store call runs on a
    /// blocking thread so it never stalls the async runtime.
    async fn persist_checkpoint(&self, checkpoint_id: u64) -> Result<()> {
        let Some(storage) = self.storage.clone() else {
            return Ok(());
        };

        let metadata = {
            let active = self.active_checkpoints.read().await;
            active.get(&checkpoint_id).cloned().ok_or_else(|| {
                StreamingError::CheckpointError(format!("Checkpoint {checkpoint_id} not found"))
            })?
        };

        let data = oxicode::encode_to_vec(&metadata.operator_states)
            .map_err(|e| StreamingError::SerializationError(e.to_string()))?;
        let checkpoint = Checkpoint { metadata, data };

        tokio::task::spawn_blocking(move || storage.store(&checkpoint))
            .await
            .map_err(|e| {
                StreamingError::CheckpointError(format!("checkpoint store task failed: {e}"))
            })??;

        Ok(())
    }

    /// Trigger, persist, and complete a checkpoint in one durable step.
    ///
    /// Captures registered operator state, persists it through the configured
    /// storage, and only marks the checkpoint successful once the state has been
    /// durably written. If capture or persistence fails, the checkpoint is
    /// completed as a failure (removed from the active set, *not* added to the
    /// completed set) and the error is returned — there is no silent success.
    pub async fn checkpoint(&self) -> Result<u64> {
        let checkpoint_id = self.trigger_checkpoint().await?;

        if let Err(e) = self.persist_checkpoint(checkpoint_id).await {
            // Best-effort mark-as-failed; ignore the (only possible) "not found"
            // error so the original cause is what surfaces to the caller.
            let _ = self.complete_checkpoint(checkpoint_id, false).await;
            return Err(e);
        }

        self.complete_checkpoint(checkpoint_id, true).await?;
        Ok(checkpoint_id)
    }

    /// Restore registered operators from a persisted checkpoint.
    ///
    /// Loads the checkpoint through the configured storage and routes each
    /// stored operator-state blob back to the operator registered under the
    /// same name. Operators without a stored blob are left untouched.
    pub async fn restore_checkpoint(&self, checkpoint_id: u64) -> Result<()> {
        let Some(storage) = self.storage.clone() else {
            return Err(StreamingError::CheckpointError(
                "no checkpoint storage configured".to_string(),
            ));
        };

        let checkpoint = tokio::task::spawn_blocking(move || storage.load(checkpoint_id))
            .await
            .map_err(|e| {
                StreamingError::CheckpointError(format!("checkpoint load task failed: {e}"))
            })??
            .ok_or_else(|| {
                StreamingError::CheckpointError(format!(
                    "Checkpoint {checkpoint_id} not found in storage"
                ))
            })?;

        let handles: Vec<(String, Arc<dyn DynOperatorState>)> = {
            let operators = self.operators.read().await;
            operators
                .iter()
                .map(|(name, state)| (name.clone(), state.clone()))
                .collect()
        };

        for (name, state) in handles {
            if let Some(bytes) = checkpoint.metadata.operator_states.get(&name) {
                state.restore_boxed(bytes).await?;
            }
        }

        Ok(())
    }

    /// Complete a checkpoint.
    pub async fn complete_checkpoint(&self, checkpoint_id: u64, success: bool) -> Result<()> {
        let mut active = self.active_checkpoints.write().await;

        if let Some(mut metadata) = active.remove(&checkpoint_id) {
            metadata.success = success;
            metadata.duration = match (Utc::now() - metadata.timestamp).to_std() {
                Ok(duration) => duration,
                Err(_) => Duration::ZERO,
            };

            if success {
                self.completed_checkpoints.write().await.push(checkpoint_id);
            }

            Ok(())
        } else {
            Err(StreamingError::CheckpointError(format!(
                "Checkpoint {} not found",
                checkpoint_id
            )))
        }
    }

    /// Get active checkpoint count.
    pub async fn active_count(&self) -> usize {
        self.active_checkpoints.read().await.len()
    }

    /// Get a copy of an active (triggered-but-not-completed) checkpoint's
    /// metadata, if present.
    pub async fn active_metadata(&self, checkpoint_id: u64) -> Option<CheckpointMetadata> {
        self.active_checkpoints
            .read()
            .await
            .get(&checkpoint_id)
            .cloned()
    }

    /// Get completed checkpoint count.
    pub async fn completed_count(&self) -> usize {
        self.completed_checkpoints.read().await.len()
    }

    /// Get the latest completed checkpoint ID.
    pub async fn latest_checkpoint(&self) -> Option<u64> {
        self.completed_checkpoints.read().await.last().copied()
    }

    /// Clear old checkpoints.
    pub async fn clear_old_checkpoints(&self, keep_count: usize) {
        let mut completed = self.completed_checkpoints.write().await;

        if completed.len() > keep_count {
            let to_remove = completed.len() - keep_count;
            completed.drain(0..to_remove);
        }
    }

    /// Start periodic checkpointing.
    ///
    /// On every `config.interval`, this runs the full capture → persist →
    /// complete pipeline via [`CheckpointCoordinator::checkpoint`], bounded by
    /// `config.timeout`. A checkpoint is only reported as successful once the
    /// registered operator state has actually been captured (and, when storage
    /// is configured, durably written). Failures and timeouts are logged and
    /// the loop continues; a checkpoint is never marked successful without real
    /// state having been captured.
    pub async fn start_periodic_checkpointing(self: Arc<Self>) {
        let interval = self.config.interval;
        let timeout = self.config.timeout;

        tokio::spawn(async move {
            loop {
                sleep(interval).await;

                match tokio::time::timeout(timeout, self.checkpoint()).await {
                    Ok(Ok(id)) => {
                        tracing::info!("Completed checkpoint {}", id);
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("Checkpoint failed: {}", e);
                    }
                    Err(_) => {
                        tracing::error!("Checkpoint timed out after {:?}", timeout);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::operator_state::{BroadcastState, OperatorState};

    /// Operator state whose snapshot always fails.
    struct FailingOperator;

    impl OperatorState for FailingOperator {
        async fn snapshot(&self) -> Result<Vec<u8>> {
            Err(StreamingError::StateError("snapshot failed".to_string()))
        }

        async fn restore(&self, _snapshot: &[u8]) -> Result<()> {
            Ok(())
        }
    }

    /// Storage whose `store` always fails.
    struct FailingStorage;

    impl CheckpointStorage for FailingStorage {
        fn store(&self, _checkpoint: &Checkpoint) -> Result<()> {
            Err(StreamingError::CheckpointError("store failed".to_string()))
        }
        fn load(&self, _checkpoint_id: u64) -> Result<Option<Checkpoint>> {
            Ok(None)
        }
        fn delete(&self, _checkpoint_id: u64) -> Result<()> {
            Ok(())
        }
        fn list(&self) -> Result<Vec<u64>> {
            Ok(Vec::new())
        }
        fn latest(&self) -> Result<Option<u64>> {
            Ok(None)
        }
    }

    fn no_pause_config() -> CheckpointConfig {
        CheckpointConfig {
            min_pause: Duration::ZERO,
            max_concurrent: 4,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_checkpoint_captures_operator_state() {
        let coordinator = CheckpointCoordinator::new(no_pause_config());

        let state = Arc::new(BroadcastState::new());
        state.put(vec![1], vec![42]).await;
        state.put(vec![2], vec![43]).await;
        coordinator
            .register_operator("op1", state.clone() as Arc<dyn DynOperatorState>)
            .await;

        assert_eq!(coordinator.operator_count().await, 1);

        let id = coordinator
            .trigger_checkpoint()
            .await
            .expect("trigger should succeed");

        let metadata = coordinator
            .active_metadata(id)
            .await
            .expect("active metadata should exist");

        // Real state must have been captured — not an empty map / zero bytes.
        assert!(metadata.operator_states.contains_key("op1"));
        assert!(!metadata.operator_states["op1"].is_empty());
        assert!(metadata.size_bytes > 0);
    }

    #[tokio::test]
    async fn test_checkpoint_persist_and_restore() {
        let dir = tempfile::tempdir().expect("temp dir");
        let storage: Arc<dyn CheckpointStorage> =
            Arc::new(FileCheckpointStorage::new(dir.path().to_path_buf()).expect("storage"));

        let coordinator = CheckpointCoordinator::with_storage(no_pause_config(), storage.clone());

        let state = Arc::new(BroadcastState::new());
        state.put(vec![7], vec![99]).await;
        coordinator
            .register_operator("opA", state as Arc<dyn DynOperatorState>)
            .await;

        let id = coordinator
            .checkpoint()
            .await
            .expect("durable checkpoint should succeed");

        // Success is only reported after real persistence.
        assert_eq!(coordinator.completed_count().await, 1);
        assert_eq!(coordinator.active_count().await, 0);

        // The checkpoint must be on disk and loadable.
        let loaded = storage.load(id).expect("load").expect("checkpoint present");
        assert_eq!(loaded.metadata.operator_states["opA"], {
            let tmp = BroadcastState::new();
            tmp.put(vec![7], vec![99]).await;
            tmp.snapshot().await.expect("snapshot")
        });

        // Restore into a fresh coordinator + empty operator.
        let recovered = CheckpointCoordinator::with_storage(no_pause_config(), storage.clone());
        let restored_state = Arc::new(BroadcastState::new());
        recovered
            .register_operator("opA", restored_state.clone() as Arc<dyn DynOperatorState>)
            .await;
        recovered
            .restore_checkpoint(id)
            .await
            .expect("restore should succeed");

        assert_eq!(restored_state.get(&[7]).await, Some(vec![99]));
    }

    #[tokio::test]
    async fn test_checkpoint_store_failure_reports_failure() {
        let coordinator =
            CheckpointCoordinator::with_storage(no_pause_config(), Arc::new(FailingStorage));

        let state = Arc::new(BroadcastState::new());
        state.put(vec![1], vec![1]).await;
        coordinator
            .register_operator("op1", state as Arc<dyn DynOperatorState>)
            .await;

        let result = coordinator.checkpoint().await;
        assert!(result.is_err(), "store failure must surface as an error");

        // No silent success: nothing recorded as completed, nothing left active.
        assert_eq!(coordinator.completed_count().await, 0);
        assert_eq!(coordinator.active_count().await, 0);
    }

    #[tokio::test]
    async fn test_checkpoint_snapshot_failure_reports_failure() {
        let coordinator = CheckpointCoordinator::new(no_pause_config());
        coordinator
            .register_operator(
                "bad",
                Arc::new(FailingOperator) as Arc<dyn DynOperatorState>,
            )
            .await;

        let result = coordinator.checkpoint().await;
        assert!(result.is_err(), "snapshot failure must surface as an error");

        assert_eq!(coordinator.completed_count().await, 0);
        assert_eq!(coordinator.active_count().await, 0);
    }

    #[tokio::test]
    async fn test_file_checkpoint_storage_roundtrip() {
        let dir = tempfile::tempdir().expect("temp dir");
        let storage = FileCheckpointStorage::new(dir.path().to_path_buf()).expect("storage");

        assert_eq!(storage.list().expect("list"), Vec::<u64>::new());
        assert_eq!(storage.latest().expect("latest"), None);
        assert!(storage.load(0).expect("load missing").is_none());

        let checkpoint = Checkpoint::new(3, vec![1, 2, 3, 4]);
        storage.store(&checkpoint).expect("store");

        assert_eq!(storage.list().expect("list"), vec![3]);
        assert_eq!(storage.latest().expect("latest"), Some(3));

        let loaded = storage.load(3).expect("load").expect("present");
        assert_eq!(loaded.id(), 3);
        assert_eq!(loaded.data, vec![1, 2, 3, 4]);

        storage.delete(3).expect("delete");
        assert!(storage.load(3).expect("load after delete").is_none());
        assert_eq!(storage.list().expect("list"), Vec::<u64>::new());
    }

    #[tokio::test]
    async fn test_checkpoint_creation() {
        let data = vec![1, 2, 3, 4];
        let checkpoint = Checkpoint::new(1, data.clone());

        assert_eq!(checkpoint.id(), 1);
        assert_eq!(checkpoint.size(), 4);
        assert_eq!(checkpoint.data, data);
    }

    #[tokio::test]
    async fn test_checkpoint_barrier() {
        let barrier = CheckpointBarrier::new(1);
        assert_eq!(barrier.id, 1);
    }

    #[tokio::test]
    async fn test_checkpoint_coordinator() {
        let config = CheckpointConfig {
            min_pause: Duration::ZERO, // Allow immediate consecutive checkpoints
            max_concurrent: 2,         // Allow 2 concurrent checkpoints
            ..Default::default()
        };
        let coordinator = CheckpointCoordinator::new(config);

        let id1 = coordinator
            .trigger_checkpoint()
            .await
            .expect("First checkpoint trigger should succeed");
        assert_eq!(id1, 0);

        let id2 = coordinator
            .trigger_checkpoint()
            .await
            .expect("Second checkpoint trigger should succeed");
        assert_eq!(id2, 1);

        assert_eq!(coordinator.active_count().await, 2);

        coordinator
            .complete_checkpoint(id1, true)
            .await
            .expect("Checkpoint completion should succeed");
        assert_eq!(coordinator.active_count().await, 1);
        assert_eq!(coordinator.completed_count().await, 1);
    }

    #[tokio::test]
    async fn test_checkpoint_min_pause() {
        let config = CheckpointConfig {
            min_pause: Duration::from_secs(60),
            ..Default::default()
        };

        let coordinator = CheckpointCoordinator::new(config);

        coordinator
            .trigger_checkpoint()
            .await
            .expect("First checkpoint should trigger successfully");
        let result = coordinator.trigger_checkpoint().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_clear_old_checkpoints() {
        let config = CheckpointConfig {
            min_pause: Duration::ZERO, // Allow rapid consecutive checkpoints
            ..Default::default()
        };
        let coordinator = CheckpointCoordinator::new(config);

        for _ in 0..5 {
            let id = coordinator
                .trigger_checkpoint()
                .await
                .expect("Checkpoint trigger should succeed in loop");
            coordinator
                .complete_checkpoint(id, true)
                .await
                .expect("Checkpoint completion should succeed in loop");
        }

        assert_eq!(coordinator.completed_count().await, 5);

        coordinator.clear_old_checkpoints(2).await;
        assert_eq!(coordinator.completed_count().await, 2);
    }
}
