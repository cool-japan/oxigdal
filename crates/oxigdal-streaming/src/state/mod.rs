//! State management for stateful stream processing.

mod backend;
mod checkpoint;
mod keyed_state;
mod operator_state;

#[cfg(feature = "kv-store")]
pub use backend::KvStateBackend;
pub use backend::{MemoryStateBackend, StateBackend};
pub use checkpoint::{
    Checkpoint, CheckpointBarrier, CheckpointConfig, CheckpointCoordinator, CheckpointMetadata,
    CheckpointStorage, FileCheckpointStorage,
};
pub use keyed_state::{
    AggregatingState, KeyedState, ListState, MapState, ReducingState, ValueState,
};
pub use operator_state::{
    BroadcastState, DynOperatorState, ListCheckpointed, OperatorState, UnionListState,
};
