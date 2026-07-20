//! Client SDK for WebSocket connections
//!
//! This module provides client-side SDKs for connecting to the WebSocket server
//! including JavaScript/TypeScript client implementations.

pub mod javascript;

pub use javascript::{generate_javascript_client, generate_typescript_definitions};

/// Client SDK configuration
#[derive(Debug, Clone)]
pub struct ClientSdkConfig {
    /// Enable TypeScript definitions
    pub enable_typescript: bool,
    /// Enable reconnection logic
    pub enable_reconnection: bool,
    /// Enable client-side caching
    pub enable_caching: bool,
    /// Reconnection delay in milliseconds
    pub reconnection_delay_ms: u64,
    /// Maximum reconnection attempts
    pub max_reconnection_attempts: u32,
}

impl Default for ClientSdkConfig {
    fn default() -> Self {
        Self {
            enable_typescript: true,
            enable_reconnection: true,
            enable_caching: true,
            reconnection_delay_ms: 1000,
            max_reconnection_attempts: 10,
        }
    }
}
