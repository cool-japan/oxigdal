//! Checkpoint-based stream recovery for fault tolerance.
//!
//! A checkpoint captures the processing state at a given sequence number.
//! On restart, processing resumes from the last checkpoint, replaying only
//! the events since that point (minimal-replay guarantee).
//!
//! # Components
//!
//! - [`CheckpointId`]: unique identity of a checkpoint (stream + sequence number).
//! - [`CheckpointState`]: serialisable snapshot of all operator states, source
//!   offsets, watermark, and event count.
//! - [`CheckpointStore`]: storage abstraction over a durable (or in-memory)
//!   backend, so [`CheckpointManager`] can be parameterized over where
//!   checkpoints live.
//! - [`InMemoryCheckpointStore`]: bounded in-memory store (useful for testing
//!   and for single-process use).
//! - [`FileCheckpointStore`]: durable file-system-backed store that survives a
//!   process restart — the scenario checkpointing exists to recover from.
//! - [`CheckpointManager`]: drives periodic checkpointing and recovery.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::StreamingError;

// ─── CheckpointId ─────────────────────────────────────────────────────────────

/// Unique identifier for a checkpoint within a named stream.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CheckpointId {
    /// Logical stream name.
    pub stream_id: String,
    /// Sequence number of the last event included in this checkpoint.
    pub sequence: u64,
    /// Wall-clock time at which the checkpoint was created.
    pub created_at: SystemTime,
}

impl CheckpointId {
    /// Construct a new `CheckpointId` timestamped *now*.
    pub fn new(stream_id: impl Into<String>, sequence: u64) -> Self {
        Self {
            stream_id: stream_id.into(),
            sequence,
            created_at: SystemTime::now(),
        }
    }
}

// ─── CheckpointState ─────────────────────────────────────────────────────────

/// Serialisable snapshot captured at a checkpoint.
///
/// # Binary format
///
/// ```text
/// [8 bytes]  sequence          (little-endian u64)
/// [8 bytes]  watermark_ns      (little-endian u64)
/// [8 bytes]  event_count       (little-endian u64)
/// [4 bytes]  n_operators       (little-endian u32)
/// for each operator:
///   [4 bytes] name_len         (little-endian u32)
///   [name_len bytes] name      (UTF-8)
///   [4 bytes] state_len        (little-endian u32)
///   [state_len bytes] state    (opaque bytes)
/// [4 bytes]  n_sources         (little-endian u32)
/// for each source:
///   [4 bytes] name_len         (little-endian u32)
///   [name_len bytes] name      (UTF-8)
///   [8 bytes] offset           (little-endian u64)
/// ```
#[derive(Debug, Clone)]
pub struct CheckpointState {
    /// Identity of this checkpoint.
    pub id: CheckpointId,
    /// Per-operator opaque state blobs.  Key: operator name.
    pub operator_states: HashMap<String, Vec<u8>>,
    /// Per-source byte/record offsets.  Key: source identifier.
    pub source_offsets: HashMap<String, u64>,
    /// Maximum processed event time expressed as nanoseconds since the Unix epoch.
    pub watermark_ns: u64,
    /// Number of events processed up to and including this checkpoint.
    pub event_count: u64,
    /// Arbitrary key→value metadata.
    pub metadata: HashMap<String, String>,
}

impl CheckpointState {
    /// Create an empty state for the given checkpoint ID.
    pub fn new(id: CheckpointId) -> Self {
        Self {
            id,
            operator_states: HashMap::new(),
            source_offsets: HashMap::new(),
            watermark_ns: 0,
            event_count: 0,
            metadata: HashMap::new(),
        }
    }

    /// Store an operator's serialised state.
    pub fn set_operator_state(&mut self, operator: impl Into<String>, state: Vec<u8>) {
        self.operator_states.insert(operator.into(), state);
    }

    /// Record the byte/record offset for a source.
    pub fn set_source_offset(&mut self, source: impl Into<String>, offset: u64) {
        self.source_offsets.insert(source.into(), offset);
    }

    /// Serialise this state to a compact binary representation.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.extend_from_slice(&self.id.sequence.to_le_bytes());
        buf.extend_from_slice(&self.watermark_ns.to_le_bytes());
        buf.extend_from_slice(&self.event_count.to_le_bytes());

        // Operator states
        buf.extend_from_slice(&(self.operator_states.len() as u32).to_le_bytes());
        for (name, state) in &self.operator_states {
            let name_bytes = name.as_bytes();
            buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(name_bytes);
            buf.extend_from_slice(&(state.len() as u32).to_le_bytes());
            buf.extend_from_slice(state);
        }

        // Source offsets
        buf.extend_from_slice(&(self.source_offsets.len() as u32).to_le_bytes());
        for (name, offset) in &self.source_offsets {
            let name_bytes = name.as_bytes();
            buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(name_bytes);
            buf.extend_from_slice(&offset.to_le_bytes());
        }

        buf
    }

    /// Deserialise a `CheckpointState` from bytes previously produced by [`Self::serialize`].
    ///
    /// Returns [`StreamingError::DeserializationError`] if the data is truncated
    /// or otherwise malformed.
    pub fn deserialize(stream_id: &str, data: &[u8]) -> Result<Self, StreamingError> {
        const HEADER: usize = 24; // sequence(8) + watermark_ns(8) + event_count(8)
        if data.len() < HEADER {
            return Err(StreamingError::DeserializationError(
                "checkpoint data too short for header".into(),
            ));
        }

        let sequence = Self::read_u64(data, 0)?;
        let watermark_ns = Self::read_u64(data, 8)?;
        let event_count = Self::read_u64(data, 16)?;

        let id = CheckpointId::new(stream_id, sequence);
        let mut state = Self::new(id);
        state.watermark_ns = watermark_ns;
        state.event_count = event_count;

        let mut cursor = HEADER;

        // ── operator states ──
        let n_ops = Self::read_u32(data, cursor)? as usize;
        cursor += 4;
        for _ in 0..n_ops {
            let (name, advance) = Self::read_string(data, cursor)?;
            cursor += advance;
            let state_len = Self::read_u32(data, cursor)? as usize;
            cursor += 4;
            if cursor + state_len > data.len() {
                return Err(StreamingError::DeserializationError(
                    "truncated operator state bytes".into(),
                ));
            }
            let op_state = data[cursor..cursor + state_len].to_vec();
            cursor += state_len;
            state.operator_states.insert(name, op_state);
        }

        // ── source offsets ──
        if cursor + 4 > data.len() {
            // No source-offsets section present — treat as empty.
            return Ok(state);
        }
        let n_src = Self::read_u32(data, cursor)? as usize;
        cursor += 4;
        for _ in 0..n_src {
            let (name, advance) = Self::read_string(data, cursor)?;
            cursor += advance;
            let offset = Self::read_u64(data, cursor)?;
            cursor += 8;
            state.source_offsets.insert(name, offset);
        }

        Ok(state)
    }

    // ── byte-reading helpers ─────────────────────────────────────────────────

    fn read_u64(data: &[u8], offset: usize) -> Result<u64, StreamingError> {
        data.get(offset..offset + 8)
            .and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or_else(|| {
                StreamingError::DeserializationError(format!("cannot read u64 at offset {offset}"))
            })
    }

    fn read_u32(data: &[u8], offset: usize) -> Result<u32, StreamingError> {
        data.get(offset..offset + 4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| {
                StreamingError::DeserializationError(format!("cannot read u32 at offset {offset}"))
            })
    }

    /// Read a length-prefixed UTF-8 string from `data[cursor..]`.
    ///
    /// Returns `(string, bytes_consumed)` where `bytes_consumed` includes the
    /// 4-byte length prefix.
    fn read_string(data: &[u8], cursor: usize) -> Result<(String, usize), StreamingError> {
        let name_len = Self::read_u32(data, cursor)? as usize;
        let name_start = cursor + 4;
        let name_end = name_start + name_len;
        if name_end > data.len() {
            return Err(StreamingError::DeserializationError(
                "truncated string bytes".into(),
            ));
        }
        let name = String::from_utf8(data[name_start..name_end].to_vec()).map_err(|e| {
            StreamingError::DeserializationError(format!("invalid UTF-8 in field name: {e}"))
        })?;
        Ok((name, 4 + name_len))
    }
}

// ─── CheckpointStore ──────────────────────────────────────────────────────────

/// Storage backend for stream checkpoints.
///
/// Implementations decide *where* checkpoint state lives. [`CheckpointManager`]
/// is generic over this trait so the same driver can target an in-memory store
/// ([`InMemoryCheckpointStore`]) for tests/single-process use or a durable
/// backend ([`FileCheckpointStore`]) that survives a crash/restart.
pub trait CheckpointStore {
    /// Persist a checkpoint. Later checkpoints for the same stream supersede
    /// earlier ones.
    fn save(&mut self, state: CheckpointState) -> Result<(), StreamingError>;

    /// Return the most recent checkpoint for `stream_id`, or `None` if the
    /// stream has no checkpoints.
    fn latest(&self, stream_id: &str) -> Result<Option<CheckpointState>, StreamingError>;
}

// ─── FileCheckpointStore ──────────────────────────────────────────────────────

/// Durable, file-system-backed checkpoint store.
///
/// Each checkpoint is written to `root/{stream}/checkpoint-{sequence}.ckpt`
/// using [`CheckpointState::serialize`] and an atomic temp-file + rename, so a
/// checkpoint that has been acknowledged is guaranteed to survive a process
/// crash or restart. Recovery scans the stream directory for the highest
/// sequence number. Pure Rust, no external services.
pub struct FileCheckpointStore {
    root: PathBuf,
    /// Maximum checkpoint files retained per stream (oldest pruned on save).
    max_per_stream: usize,
}

impl FileCheckpointStore {
    /// Create a store rooted at `root`, retaining at most `max_per_stream`
    /// checkpoint files per stream. The root directory is created if missing.
    pub fn new(root: impl Into<PathBuf>, max_per_stream: usize) -> Result<Self, StreamingError> {
        if max_per_stream == 0 {
            return Err(StreamingError::ConfigError(
                "max_per_stream must be at least 1".into(),
            ));
        }
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            max_per_stream,
        })
    }

    /// Sanitize a stream id so it is safe as a single path component.
    fn sanitize(stream_id: &str) -> String {
        stream_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn stream_dir(&self, stream_id: &str) -> PathBuf {
        self.root.join(Self::sanitize(stream_id))
    }

    fn checkpoint_file(&self, stream_id: &str, sequence: u64) -> PathBuf {
        // Zero-pad so lexical order matches numeric order.
        self.stream_dir(stream_id)
            .join(format!("checkpoint-{sequence:020}.ckpt"))
    }

    /// Parse the sequence number out of a stored checkpoint file name.
    fn parse_sequence(path: &Path) -> Option<u64> {
        let name = path.file_name()?.to_str()?;
        let rest = name.strip_prefix("checkpoint-")?;
        let seq = rest.strip_suffix(".ckpt")?;
        seq.parse::<u64>().ok()
    }

    /// All (sequence, path) pairs for a stream, sorted ascending by sequence.
    fn entries(&self, stream_id: &str) -> Result<Vec<(u64, PathBuf)>, StreamingError> {
        let dir = self.stream_dir(stream_id);
        let mut out = Vec::new();
        match std::fs::read_dir(&dir) {
            Ok(rd) => {
                for entry in rd {
                    let entry = entry?;
                    let path = entry.path();
                    if let Some(seq) = Self::parse_sequence(&path) {
                        out.push((seq, path));
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        }
        out.sort_by_key(|(seq, _)| *seq);
        Ok(out)
    }
}

impl CheckpointStore for FileCheckpointStore {
    fn save(&mut self, state: CheckpointState) -> Result<(), StreamingError> {
        let stream_id = state.id.stream_id.clone();
        let dir = self.stream_dir(&stream_id);
        std::fs::create_dir_all(&dir)?;

        let final_path = self.checkpoint_file(&stream_id, state.id.sequence);
        let tmp_path = final_path.with_extension("ckpt.tmp");
        let bytes = state.serialize();
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;

        // Prune oldest checkpoints beyond the retention bound.
        let entries = self.entries(&stream_id)?;
        if entries.len() > self.max_per_stream {
            let excess = entries.len() - self.max_per_stream;
            for (_, path) in entries.into_iter().take(excess) {
                std::fs::remove_file(&path).ok();
            }
        }
        Ok(())
    }

    fn latest(&self, stream_id: &str) -> Result<Option<CheckpointState>, StreamingError> {
        let entries = self.entries(stream_id)?;
        let Some((_, path)) = entries.into_iter().next_back() else {
            return Ok(None);
        };
        let bytes = std::fs::read(&path)?;
        let state = CheckpointState::deserialize(stream_id, &bytes)?;
        Ok(Some(state))
    }
}

// ─── InMemoryCheckpointStore ──────────────────────────────────────────────────

/// A bounded in-memory checkpoint store.
///
/// Each stream maintains its own sorted list of checkpoints.  When the number
/// of checkpoints for a stream exceeds `max_per_stream`, the **oldest** ones are
/// evicted automatically.
pub struct InMemoryCheckpointStore {
    /// stream_id → list of checkpoints, sorted ascending by sequence number.
    checkpoints: HashMap<String, Vec<CheckpointState>>,
    /// Maximum checkpoints retained per stream.
    max_per_stream: usize,
}

impl InMemoryCheckpointStore {
    /// Create a store that retains at most `max_per_stream` checkpoints per stream.
    pub fn new(max_per_stream: usize) -> Self {
        assert!(max_per_stream > 0, "max_per_stream must be at least 1");
        Self {
            checkpoints: HashMap::new(),
            max_per_stream,
        }
    }

    /// Save a checkpoint state.  The list is kept sorted by sequence number.
    pub fn save(&mut self, state: CheckpointState) -> Result<(), StreamingError> {
        let stream_id = state.id.stream_id.clone();
        let entry = self.checkpoints.entry(stream_id).or_default();
        entry.push(state);
        entry.sort_by_key(|s| s.id.sequence);
        // Trim to max_per_stream, evicting oldest (lowest sequence)
        if entry.len() > self.max_per_stream {
            let excess = entry.len() - self.max_per_stream;
            entry.drain(0..excess);
        }
        Ok(())
    }

    /// Return the most recent checkpoint for the given stream, or `None`.
    pub fn latest(&self, stream_id: &str) -> Option<&CheckpointState> {
        self.checkpoints.get(stream_id)?.last()
    }

    /// Return all checkpoints for the given stream, sorted ascending by sequence.
    pub fn list(&self, stream_id: &str) -> Vec<&CheckpointState> {
        self.checkpoints
            .get(stream_id)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Remove all checkpoints for `stream_id` with sequence number **less than** `sequence`.
    pub fn delete_before(&mut self, stream_id: &str, sequence: u64) {
        if let Some(entry) = self.checkpoints.get_mut(stream_id) {
            entry.retain(|s| s.id.sequence >= sequence);
        }
    }

    /// Number of checkpoints currently stored for the given stream.
    pub fn checkpoint_count(&self, stream_id: &str) -> usize {
        self.checkpoints
            .get(stream_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

impl CheckpointStore for InMemoryCheckpointStore {
    fn save(&mut self, state: CheckpointState) -> Result<(), StreamingError> {
        InMemoryCheckpointStore::save(self, state)
    }

    fn latest(&self, stream_id: &str) -> Result<Option<CheckpointState>, StreamingError> {
        Ok(InMemoryCheckpointStore::latest(self, stream_id).cloned())
    }
}

// ─── CheckpointManager ────────────────────────────────────────────────────────

/// Drives periodic checkpointing and provides recovery support.
///
/// Call [`Self::on_event`] after processing each event.  When the cumulative sequence
/// number reaches the next scheduled checkpoint, a new [`CheckpointState`] is
/// automatically saved to the underlying store.
pub struct CheckpointManager<S: CheckpointStore = InMemoryCheckpointStore> {
    store: S,
    /// Checkpoint every `checkpoint_interval` events.
    checkpoint_interval: u64,
    next_checkpoint_at: u64,
    total_checkpoints: u64,
}

impl<S: CheckpointStore> CheckpointManager<S> {
    /// Create a manager with the given store and interval.
    ///
    /// Use a durable store such as [`FileCheckpointStore`] for recovery that
    /// survives a process restart, or [`InMemoryCheckpointStore`] for tests and
    /// single-process, non-recoverable use.
    pub fn new(store: S, checkpoint_interval: u64) -> Self {
        assert!(
            checkpoint_interval > 0,
            "checkpoint_interval must be positive"
        );
        Self {
            store,
            checkpoint_interval,
            next_checkpoint_at: checkpoint_interval,
            total_checkpoints: 0,
        }
    }

    /// Called after each processed event.
    ///
    /// Returns `Ok(true)` if a checkpoint was taken, `Ok(false)` otherwise.
    pub fn on_event(
        &mut self,
        stream_id: &str,
        sequence: u64,
        watermark_ns: u64,
    ) -> Result<bool, StreamingError> {
        if sequence >= self.next_checkpoint_at {
            let id = CheckpointId::new(stream_id, sequence);
            let mut state = CheckpointState::new(id);
            state.watermark_ns = watermark_ns;
            state.event_count = sequence;
            self.store.save(state)?;
            self.next_checkpoint_at = sequence + self.checkpoint_interval;
            self.total_checkpoints += 1;
            return Ok(true);
        }
        Ok(false)
    }

    /// Return the sequence number from which to resume, or `None` if no
    /// checkpoint exists for the stream.
    ///
    /// With a durable store this reads the last checkpoint persisted before a
    /// crash, enabling minimal-replay recovery.
    pub fn recover(&self, stream_id: &str) -> Result<Option<u64>, StreamingError> {
        Ok(self.store.latest(stream_id)?.map(|s| s.id.sequence))
    }

    /// Total checkpoints taken since this manager was created.
    pub fn total_checkpoints(&self) -> u64 {
        self.total_checkpoints
    }

    /// Read-only access to the underlying store (for inspection / testing).
    pub fn store(&self) -> &S {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CheckpointState serialisation ────────────────────────────────────────

    #[test]
    fn test_serialize_deserialize_round_trip_empty() {
        let id = CheckpointId::new("stream-a", 42);
        let mut state = CheckpointState::new(id);
        state.watermark_ns = 999_000_000;
        state.event_count = 42;

        let bytes = state.serialize();
        let decoded = CheckpointState::deserialize("stream-a", &bytes)
            .expect("deserialization should succeed");

        assert_eq!(decoded.id.sequence, 42);
        assert_eq!(decoded.watermark_ns, 999_000_000);
        assert_eq!(decoded.event_count, 42);
        assert!(decoded.operator_states.is_empty());
        assert!(decoded.source_offsets.is_empty());
    }

    #[test]
    fn test_serialize_deserialize_with_operator_states() {
        let id = CheckpointId::new("s", 1);
        let mut state = CheckpointState::new(id);
        state.set_operator_state("agg_op", vec![1, 2, 3, 4]);
        state.set_operator_state("filter_op", vec![9, 8]);

        let bytes = state.serialize();
        let decoded = CheckpointState::deserialize("s", &bytes).expect("should succeed");
        assert_eq!(
            decoded.operator_states.get("agg_op"),
            Some(&vec![1, 2, 3, 4])
        );
        assert_eq!(decoded.operator_states.get("filter_op"), Some(&vec![9, 8]));
    }

    #[test]
    fn test_serialize_deserialize_with_source_offsets() {
        let id = CheckpointId::new("s", 7);
        let mut state = CheckpointState::new(id);
        state.set_source_offset("kafka-topic-0", 1_234_567);
        state.set_source_offset("file-source", 4_096);

        let bytes = state.serialize();
        let decoded = CheckpointState::deserialize("s", &bytes).expect("should succeed");
        assert_eq!(
            decoded.source_offsets.get("kafka-topic-0"),
            Some(&1_234_567)
        );
        assert_eq!(decoded.source_offsets.get("file-source"), Some(&4_096));
    }

    #[test]
    fn test_deserialize_truncated_data_returns_error() {
        let result = CheckpointState::deserialize("s", &[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_empty_slice_returns_error() {
        let result = CheckpointState::deserialize("s", &[]);
        assert!(result.is_err());
    }

    // ── InMemoryCheckpointStore ───────────────────────────────────────────────

    #[test]
    fn test_store_save_and_latest() {
        let mut store = InMemoryCheckpointStore::new(5);
        let id = CheckpointId::new("stream-x", 10);
        let state = CheckpointState::new(id);
        store.save(state).expect("save should succeed");
        let latest = store.latest("stream-x").expect("should be present");
        assert_eq!(latest.id.sequence, 10);
    }

    #[test]
    fn test_store_latest_none_when_empty() {
        let store = InMemoryCheckpointStore::new(5);
        assert!(store.latest("unknown").is_none());
    }

    #[test]
    fn test_store_trims_to_max_per_stream() {
        let mut store = InMemoryCheckpointStore::new(3);
        for i in 0u64..6 {
            let id = CheckpointId::new("s", i);
            store.save(CheckpointState::new(id)).expect("save ok");
        }
        assert_eq!(store.checkpoint_count("s"), 3);
        // The oldest should have been evicted; latest should be seq=5
        assert_eq!(
            store
                .latest("s")
                .expect("latest checkpoint for stream 's'")
                .id
                .sequence,
            5
        );
    }

    #[test]
    fn test_store_delete_before() {
        let mut store = InMemoryCheckpointStore::new(10);
        for i in 0u64..5 {
            let id = CheckpointId::new("s", i * 10);
            store.save(CheckpointState::new(id)).expect("save ok");
        }
        // Delete all checkpoints with sequence < 20
        store.delete_before("s", 20);
        let remaining = store.list("s");
        assert!(remaining.iter().all(|c| c.id.sequence >= 20));
    }

    #[test]
    fn test_store_multiple_streams_independent() {
        let mut store = InMemoryCheckpointStore::new(5);
        for seq in [1u64, 2, 3] {
            store
                .save(CheckpointState::new(CheckpointId::new("stream-a", seq)))
                .expect("ok");
            store
                .save(CheckpointState::new(CheckpointId::new(
                    "stream-b",
                    seq * 10,
                )))
                .expect("ok");
        }
        assert_eq!(store.checkpoint_count("stream-a"), 3);
        assert_eq!(store.checkpoint_count("stream-b"), 3);
        assert_eq!(
            store
                .latest("stream-a")
                .expect("latest checkpoint for stream-a")
                .id
                .sequence,
            3
        );
        assert_eq!(
            store
                .latest("stream-b")
                .expect("latest checkpoint for stream-b")
                .id
                .sequence,
            30
        );
    }

    // ── CheckpointManager ────────────────────────────────────────────────────

    #[test]
    fn test_manager_triggers_checkpoint_at_interval() {
        let store = InMemoryCheckpointStore::new(10);
        let mut mgr = CheckpointManager::new(store, 100);
        // Events 0-98: no checkpoint yet
        for seq in 0u64..99 {
            let triggered = mgr.on_event("s", seq, 0).expect("on_event ok");
            assert!(!triggered);
        }
        // Event 100: checkpoint fires
        let triggered = mgr.on_event("s", 100, 0).expect("on_event ok");
        assert!(triggered);
        assert_eq!(mgr.total_checkpoints(), 1);
    }

    #[test]
    fn test_manager_recover_returns_last_sequence() {
        let store = InMemoryCheckpointStore::new(10);
        let mut mgr = CheckpointManager::new(store, 50);
        mgr.on_event("s", 50, 0).expect("ok");
        mgr.on_event("s", 100, 0).expect("ok");
        let seq = mgr
            .recover("s")
            .expect("recover ok")
            .expect("should recover");
        assert_eq!(seq, 100);
    }

    #[test]
    fn test_manager_recover_none_before_first_checkpoint() {
        let store = InMemoryCheckpointStore::new(5);
        let mgr = CheckpointManager::new(store, 100);
        assert!(mgr.recover("s").expect("recover ok").is_none());
    }

    // ── FileCheckpointStore durability ────────────────────────────────────────

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "oxigeo_streaming_ckpt_{}_{}_{}",
            tag,
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn test_file_store_save_and_latest_roundtrip() {
        let dir = unique_dir("roundtrip");
        let mut store = FileCheckpointStore::new(&dir, 5).expect("store");
        assert!(store.latest("s").expect("latest").is_none());

        let mut state = CheckpointState::new(CheckpointId::new("s", 42));
        state.watermark_ns = 123_456_789;
        state.event_count = 42;
        state.set_operator_state("agg", vec![1, 2, 3]);
        state.set_source_offset("src-0", 9_999);
        store.save(state).expect("save");

        let loaded = store.latest("s").expect("latest").expect("present");
        assert_eq!(loaded.id.sequence, 42);
        assert_eq!(loaded.watermark_ns, 123_456_789);
        assert_eq!(loaded.event_count, 42);
        assert_eq!(loaded.operator_states.get("agg"), Some(&vec![1, 2, 3]));
        assert_eq!(loaded.source_offsets.get("src-0"), Some(&9_999));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_file_checkpoint_store_survives_restart() {
        let dir = unique_dir("restart");
        // First "process": drive events through a manager backed by the file store.
        {
            let store = FileCheckpointStore::new(&dir, 10).expect("store");
            let mut mgr = CheckpointManager::new(store, 10);
            for seq in 0u64..=25 {
                mgr.on_event("stream-a", seq, seq * 1_000)
                    .expect("on_event");
            }
            // Checkpoints fired at sequence 10 and 20.
            assert_eq!(mgr.total_checkpoints(), 2);
            // Manager (and its in-RAM counters) dropped here — simulating a crash.
        }

        // Second "process": a brand-new store + manager over the same directory
        // must recover the last durably-written checkpoint.
        let store = FileCheckpointStore::new(&dir, 10).expect("store");
        let mgr = CheckpointManager::new(store, 10);
        let resume = mgr
            .recover("stream-a")
            .expect("recover ok")
            .expect("should recover a checkpoint that survived the restart");
        assert_eq!(resume, 20);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_file_store_prunes_to_max_per_stream() {
        let dir = unique_dir("prune");
        let mut store = FileCheckpointStore::new(&dir, 3).expect("store");
        for seq in [10u64, 20, 30, 40, 50] {
            store
                .save(CheckpointState::new(CheckpointId::new("s", seq)))
                .expect("save");
        }
        // Only the 3 newest survive; latest is 50.
        let entries = store.entries("s").expect("entries");
        assert_eq!(entries.len(), 3);
        assert_eq!(
            store
                .latest("s")
                .expect("latest")
                .expect("present")
                .id
                .sequence,
            50
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_manager_total_checkpoints_counter() {
        let store = InMemoryCheckpointStore::new(10);
        let mut mgr = CheckpointManager::new(store, 10);
        for seq in (0u64..=50).step_by(1) {
            mgr.on_event("s", seq, 0).expect("ok");
        }
        // Checkpoints at seq 10, 20, 30, 40, 50 = 5
        assert_eq!(mgr.total_checkpoints(), 5);
    }

    #[test]
    fn test_checkpoint_state_full_round_trip() {
        let id = CheckpointId::new("full-test", 77);
        let mut state = CheckpointState::new(id);
        state.watermark_ns = 1_700_000_000_000_000_000;
        state.event_count = 77;
        state.set_operator_state("window_op", b"window_state_data".to_vec());
        state.set_source_offset("source-0", 8192);
        state.metadata.insert("app_version".into(), "1.2.3".into());

        let bytes = state.serialize();
        let decoded =
            CheckpointState::deserialize("full-test", &bytes).expect("round-trip should succeed");

        assert_eq!(decoded.id.sequence, 77);
        assert_eq!(decoded.watermark_ns, 1_700_000_000_000_000_000);
        assert_eq!(decoded.event_count, 77);
        assert_eq!(
            decoded.operator_states.get("window_op"),
            Some(&b"window_state_data".to_vec())
        );
        assert_eq!(decoded.source_offsets.get("source-0"), Some(&8192u64));
    }
}
