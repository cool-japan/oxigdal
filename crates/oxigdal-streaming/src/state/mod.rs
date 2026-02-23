//! State management for stateful stream processing.

mod backend;
mod checkpoint;
mod keyed_state;
mod operator_state;

#[cfg(feature = "rocksdb-backend")]
pub use backend::RocksDBStateBackend;
pub use backend::{MemoryStateBackend, StateBackend};
pub use checkpoint::{
    Checkpoint, CheckpointBarrier, CheckpointConfig, CheckpointCoordinator, CheckpointMetadata,
    CheckpointStorage,
};
pub use keyed_state::{
    AggregatingState, KeyedState, ListState, MapState, ReducingState, ValueState,
};
pub use operator_state::{BroadcastState, ListCheckpointed, OperatorState, UnionListState};
