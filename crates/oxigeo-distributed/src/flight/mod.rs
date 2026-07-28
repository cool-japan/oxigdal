//! Arrow Flight RPC server and client implementations.
//!
//! This module provides zero-copy data transfer between distributed nodes using
//! Apache Arrow Flight protocol.

pub mod client;
pub mod server;
pub mod wire;

pub use client::FlightClient;
pub use server::FlightServer;
pub use wire::{EXECUTE_TASK_ACTION, ExecuteTaskRequest, ExecuteTaskResponse};
