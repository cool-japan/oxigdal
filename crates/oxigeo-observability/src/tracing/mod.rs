//! Distributed tracing infrastructure.
//!
//! This module provides comprehensive distributed tracing capabilities for
//! geospatial operations, including:
//!
//! - W3C Trace Context, B3, and Jaeger format propagation
//! - Span management for geospatial operations
//! - Baggage handling for cross-service context
//! - Sampling strategies (head-based, tail-based, adaptive)
//! - Custom geospatial attributes

pub mod context;
pub mod distributed;
pub mod exporter;
pub mod sampler;
pub mod span;

use opentelemetry::global;
use opentelemetry::global::BoxedTracer;

// Re-export distributed tracing types
pub use distributed::{
    AdaptiveSampler, AlwaysOffSampler, AlwaysOnSampler, B3TraceContext, BaggageItem,
    BaggageManager, BaggageMetadata, BufferedSpan, ContextPropagator, DistributedTraceManager,
    ExtractedContext, GeospatialAttributes, GeospatialSpanBuilder, HeadBasedSampler,
    InjectionContext, JaegerTraceContext, OtelHeaderExtractor, OtelHeaderInjector,
    ParentBasedSampler, PropagationFormat, Sampler, SamplingDecision, SamplingResult, SpanEvent,
    SpanHandle, TailBasedSampler, TraceBuffer, TraceHandle, TraceStats, W3CTraceContext,
};

// Re-export context functions
pub use distributed::{extract_otel_context, inject_otel_context};

/// Tracing manager for distributed tracing operations.
pub struct TracingManager {
    service_name: String,
}

impl TracingManager {
    /// Create a new tracing manager.
    pub fn new(service_name: String) -> Self {
        Self { service_name }
    }

    /// Get a tracer for this service.
    pub fn get_tracer(&self) -> BoxedTracer {
        global::tracer(self.service_name.clone())
    }

    /// Get the service name.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Create a distributed trace manager.
    pub fn distributed_manager(&self) -> DistributedTraceManager {
        DistributedTraceManager::new(&self.service_name)
    }
}
