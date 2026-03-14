//! Streaming v2: production-quality additions to the OxiGDAL streaming framework.
//!
//! # Modules
//!
//! | Module | Purpose |
//! |---|---|
//! | [`backpressure`] | Credit-based flow control (producer/consumer credit pools) |
//! | [`session_window`] | Gap-detection session windows with configurable min-events and max-duration |
//! | [`stream_join`] | Temporal stream-to-stream joins (inner / left-outer / interval) |
//! | [`checkpoint`] | Serialisable checkpoint state with in-memory store and manager |

pub mod backpressure;
pub mod checkpoint;
pub mod session_window;
pub mod stream_join;

pub use backpressure::{BackpressureConsumer, BackpressureProducer, CreditPool, PendingItem};
pub use checkpoint::{CheckpointId, CheckpointManager, CheckpointState, InMemoryCheckpointStore};
pub use session_window::{SessionWindow, SessionWindowConfig, SessionWindowProcessor, StreamEvent};
pub use stream_join::{JoinEvent, JoinMode, JoinedPair, TemporalJoinConfig, TemporalJoiner};
