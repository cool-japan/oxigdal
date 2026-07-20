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
//! - [`InMemoryCheckpointStore`]: bounded in-memory store (useful for testing
//!   and for single-process use; production typically uses disk or object storage).
//! - [`CheckpointManager`]: drives periodic checkpointing and recovery.

use std::collections::HashMap;
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

// ─── CheckpointManager ────────────────────────────────────────────────────────

/// Drives periodic checkpointing and provides recovery support.
///
/// Call [`Self::on_event`] after processing each event.  When the cumulative sequence
/// number reaches the next scheduled checkpoint, a new [`CheckpointState`] is
/// automatically saved to the underlying store.
pub struct CheckpointManager {
    store: InMemoryCheckpointStore,
    /// Checkpoint every `checkpoint_interval` events.
    checkpoint_interval: u64,
    next_checkpoint_at: u64,
    total_checkpoints: u64,
}

impl CheckpointManager {
    /// Create a manager with the given store and interval.
    pub fn new(store: InMemoryCheckpointStore, checkpoint_interval: u64) -> Self {
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
    pub fn recover(&self, stream_id: &str) -> Option<u64> {
        self.store.latest(stream_id).map(|s| s.id.sequence)
    }

    /// Total checkpoints taken since this manager was created.
    pub fn total_checkpoints(&self) -> u64 {
        self.total_checkpoints
    }

    /// Read-only access to the underlying store (for inspection / testing).
    pub fn store(&self) -> &InMemoryCheckpointStore {
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
        let seq = mgr.recover("s").expect("should recover");
        assert_eq!(seq, 100);
    }

    #[test]
    fn test_manager_recover_none_before_first_checkpoint() {
        let store = InMemoryCheckpointStore::new(5);
        let mgr = CheckpointManager::new(store, 100);
        assert!(mgr.recover("s").is_none());
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
