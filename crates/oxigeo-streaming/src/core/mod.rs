//! Core streaming abstractions and traits.

mod backpressure;
mod flow_control;
mod operators;
mod recovery;
pub mod stream;

pub use backpressure::{
    BackpressureConfig, BackpressureManager, BackpressureStrategy, LoadMetrics,
};
pub use flow_control::{FlowControlConfig, FlowControlMetrics, FlowController};
pub use operators::{
    FilterOperator, FlatMapOperator, LoggingSink, MapOperator, SinkOperator, SourceOperator,
    StreamOperator, TransformOperator,
};
pub use recovery::{FailureRecord, RecoveryConfig, RecoveryManager, RecoveryStrategy};
pub use stream::{
    Stream, StreamConfig, StreamElement, StreamMessage, StreamMetadata, StreamSink, StreamSource,
};
