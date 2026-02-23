//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::error::{ObservabilityError, Result};
use chrono::{DateTime, Utc};
use opentelemetry::trace::{SpanKind, TraceContextExt, Tracer};
use opentelemetry::{Context, KeyValue, global};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::functions::Sampler;

/// Context for injection into outgoing request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionContext {
    /// Trace ID.
    pub trace_id: String,
    /// Span ID.
    pub span_id: String,
    /// Whether the trace is sampled.
    pub sampled: bool,
    /// Baggage items.
    pub baggage: HashMap<String, String>,
}
impl InjectionContext {
    /// Create a new injection context.
    pub fn new(trace_id: String, span_id: String, sampled: bool) -> Self {
        Self {
            trace_id,
            span_id,
            sampled,
            baggage: HashMap::new(),
        }
    }
    /// Add baggage item.
    pub fn with_baggage(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.baggage.insert(key.into(), value.into());
        self
    }
}
/// Always-on sampler.
pub struct AlwaysOnSampler;
/// B3 trace context format (Zipkin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct B3TraceContext {
    /// Trace ID (16 or 32 hex characters).
    pub trace_id: String,
    /// Span ID (16 hex characters).
    pub span_id: String,
    /// Parent span ID (optional).
    pub parent_span_id: Option<String>,
    /// Sampling decision (0, 1, or d for debug).
    pub sampled: Option<String>,
    /// Debug flag.
    pub debug: bool,
}
impl B3TraceContext {
    /// Parse from B3 single header format.
    pub fn parse_single(b3: &str) -> Result<Self> {
        let parts: Vec<&str> = b3.split('-').collect();
        if parts.is_empty() {
            return Err(ObservabilityError::ContextPropagationFailed(
                "Empty B3 header".to_string(),
            ));
        }
        let trace_id = parts.first().copied().unwrap_or_default().to_string();
        let span_id = parts.get(1).copied().unwrap_or_default().to_string();
        let sampled = parts.get(2).map(|s| s.to_string());
        let parent_span_id = parts.get(3).map(|s| s.to_string());
        let debug = sampled.as_deref() == Some("d");
        Ok(Self {
            trace_id,
            span_id,
            parent_span_id,
            sampled,
            debug,
        })
    }
    /// Parse from B3 multi-header format.
    pub fn parse_multi(headers: &HashMap<String, String>) -> Result<Self> {
        let trace_id = headers.get("x-b3-traceid").cloned().ok_or_else(|| {
            ObservabilityError::ContextPropagationFailed("Missing x-b3-traceid".to_string())
        })?;
        let span_id = headers.get("x-b3-spanid").cloned().ok_or_else(|| {
            ObservabilityError::ContextPropagationFailed("Missing x-b3-spanid".to_string())
        })?;
        let parent_span_id = headers.get("x-b3-parentspanid").cloned();
        let sampled = headers.get("x-b3-sampled").cloned();
        let debug = headers.get("x-b3-flags").is_some_and(|v| v == "1");
        Ok(Self {
            trace_id,
            span_id,
            parent_span_id,
            sampled,
            debug,
        })
    }
    /// Convert to B3 single header format.
    pub fn to_single_header(&self) -> String {
        let mut result = format!("{}-{}", self.trace_id, self.span_id);
        if let Some(ref sampled) = self.sampled {
            result.push('-');
            result.push_str(sampled);
        }
        if let Some(ref parent) = self.parent_span_id {
            result.push('-');
            result.push_str(parent);
        }
        result
    }
    /// Convert to B3 multi-header format.
    pub fn to_multi_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("x-b3-traceid".to_string(), self.trace_id.clone());
        headers.insert("x-b3-spanid".to_string(), self.span_id.clone());
        if let Some(ref parent) = self.parent_span_id {
            headers.insert("x-b3-parentspanid".to_string(), parent.clone());
        }
        if let Some(ref sampled) = self.sampled {
            headers.insert("x-b3-sampled".to_string(), sampled.clone());
        }
        if self.debug {
            headers.insert("x-b3-flags".to_string(), "1".to_string());
        }
        headers
    }
}
/// Statistics for distributed tracing.
#[derive(Debug, Default)]
pub struct TraceStats {
    /// Total traces started.
    traces_started: AtomicU64,
    /// Total spans created.
    spans_created: AtomicU64,
    /// Traces sampled.
    traces_sampled: AtomicU64,
    /// Traces dropped.
    traces_dropped: AtomicU64,
    /// Context extractions.
    context_extractions: AtomicU64,
    /// Context injections.
    context_injections: AtomicU64,
}
impl TraceStats {
    /// Create new stats.
    pub fn new() -> Self {
        Self::default()
    }
    /// Get total traces started.
    pub fn traces_started(&self) -> u64 {
        self.traces_started.load(Ordering::SeqCst)
    }
    /// Get total spans created.
    pub fn spans_created(&self) -> u64 {
        self.spans_created.load(Ordering::SeqCst)
    }
    /// Get sampling rate.
    pub fn sampling_rate(&self) -> f64 {
        let sampled = self.traces_sampled.load(Ordering::SeqCst);
        let dropped = self.traces_dropped.load(Ordering::SeqCst);
        let total = sampled + dropped;
        if total == 0 {
            0.0
        } else {
            sampled as f64 / total as f64
        }
    }
}
/// OpenTelemetry header extractor.
pub struct OtelHeaderExtractor<'a> {
    pub(super) headers: &'a HashMap<String, String>,
}
impl<'a> OtelHeaderExtractor<'a> {
    /// Create a new extractor.
    pub fn new(headers: &'a HashMap<String, String>) -> Self {
        Self { headers }
    }
}
/// A single baggage item with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaggageItem {
    /// The value.
    pub value: String,
    /// Metadata about propagation.
    pub metadata: BaggageMetadata,
    /// When the item was added.
    pub added_at: DateTime<Utc>,
}
/// Tail-based sampler that buffers spans and makes decisions at trace completion.
pub struct TailBasedSampler {
    /// Inner sampler for initial decisions.
    pub(super) inner: Box<dyn Sampler>,
    /// Buffer of trace data for deferred decisions.
    trace_buffer: Mutex<HashMap<String, TraceBuffer>>,
    /// Maximum buffer size per trace.
    max_buffer_size: usize,
    /// Error sampling rate (sample traces with errors at higher rate).
    error_sample_rate: f64,
    /// Latency threshold for sampling (in milliseconds).
    latency_threshold_ms: u64,
    /// Description.
    pub(super) description: String,
}
impl TailBasedSampler {
    /// Create a new tail-based sampler.
    pub fn new(inner: Box<dyn Sampler>) -> Self {
        Self {
            inner,
            trace_buffer: Mutex::new(HashMap::new()),
            max_buffer_size: 1000,
            error_sample_rate: 1.0,
            latency_threshold_ms: 1000,
            description: "TailBasedSampler".to_string(),
        }
    }
    /// Set error sample rate.
    pub fn with_error_sample_rate(mut self, rate: f64) -> Self {
        self.error_sample_rate = rate.clamp(0.0, 1.0);
        self
    }
    /// Set latency threshold.
    pub fn with_latency_threshold_ms(mut self, threshold: u64) -> Self {
        self.latency_threshold_ms = threshold;
        self
    }
    /// Record a span for potential sampling.
    pub fn record_span(&self, trace_id: &str, span: BufferedSpan) {
        let mut buffer = self.trace_buffer.lock();
        let trace = buffer
            .entry(trace_id.to_string())
            .or_insert_with(|| TraceBuffer {
                spans: Vec::new(),
                has_error: false,
                start_time: Utc::now(),
                duration_ms: 0,
            });
        if trace.spans.len() < self.max_buffer_size {
            if span.has_error {
                trace.has_error = true;
            }
            trace.duration_ms = trace.duration_ms.saturating_add(span.duration_ms);
            trace.spans.push(span);
        }
    }
    /// Finalize sampling decision for a trace.
    pub fn finalize_decision(&self, trace_id: &str) -> SamplingDecision {
        let buffer = self.trace_buffer.lock();
        if let Some(trace) = buffer.get(trace_id) {
            if trace.has_error && fastrand::f64() < self.error_sample_rate {
                return SamplingDecision::Sample;
            }
            if trace.duration_ms > self.latency_threshold_ms {
                return SamplingDecision::Sample;
            }
        }
        SamplingDecision::Drop
    }
    /// Clear a trace from the buffer.
    pub fn clear_trace(&self, trace_id: &str) {
        let mut buffer = self.trace_buffer.lock();
        buffer.remove(trace_id);
    }
}
/// An event recorded on a span.
#[derive(Debug, Clone)]
pub struct SpanEvent {
    /// Event name.
    pub name: String,
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,
    /// Event attributes.
    pub attributes: Vec<(String, String)>,
}
/// Supported propagation formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PropagationFormat {
    /// W3C Trace Context.
    #[default]
    W3CTraceContext,
    /// B3 Single Header.
    B3Single,
    /// B3 Multi Header.
    B3Multi,
    /// Jaeger.
    Jaeger,
}
/// Sampling decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingDecision {
    /// Sample this trace.
    Sample,
    /// Don't sample this trace.
    Drop,
    /// Record but don't sample (for debugging).
    RecordOnly,
}
/// Adaptive sampler that adjusts rate based on throughput.
pub struct AdaptiveSampler {
    /// Target samples per second.
    target_rate: f64,
    /// Current samples count.
    pub(super) samples_count: AtomicU64,
    /// Last reset time.
    last_reset: Mutex<DateTime<Utc>>,
    /// Description.
    pub(super) description: String,
}
impl AdaptiveSampler {
    /// Create a new adaptive sampler.
    pub fn new(target_rate: f64) -> Self {
        Self {
            target_rate,
            samples_count: AtomicU64::new(0),
            last_reset: Mutex::new(Utc::now()),
            description: format!("AdaptiveSampler(target_rate={})", target_rate),
        }
    }
    pub(crate) fn calculate_ratio(&self) -> f64 {
        let mut last_reset = self.last_reset.lock();
        let now = Utc::now();
        let elapsed = (now - *last_reset).num_milliseconds() as f64 / 1000.0;
        if elapsed >= 1.0 {
            self.samples_count.store(0, Ordering::SeqCst);
            *last_reset = now;
            1.0
        } else {
            let current_count = self.samples_count.load(Ordering::SeqCst);
            let expected = (self.target_rate * elapsed) as u64;
            if current_count >= expected {
                0.0
            } else {
                (expected - current_count) as f64 / self.target_rate
            }
        }
    }
}
/// Geospatial attributes for spans.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeospatialAttributes {
    /// Bounding box [min_x, min_y, max_x, max_y].
    pub bbox: Option<[f64; 4]>,
    /// Coordinate reference system (EPSG code).
    pub crs_epsg: Option<i32>,
    /// Geometry type (Point, Line, Polygon, etc.).
    pub geometry_type: Option<String>,
    /// Feature count.
    pub feature_count: Option<u64>,
    /// Raster dimensions [width, height].
    pub raster_dims: Option<[u32; 2]>,
    /// Raster band count.
    pub band_count: Option<u32>,
    /// Data size in bytes.
    pub data_size_bytes: Option<u64>,
    /// Driver name (GeoTIFF, Shapefile, etc.).
    pub driver: Option<String>,
    /// Layer name.
    pub layer_name: Option<String>,
    /// Resolution [x_res, y_res].
    pub resolution: Option<[f64; 2]>,
    /// Tile coordinates [z, x, y].
    pub tile_coords: Option<[u32; 3]>,
    /// Processing stage.
    pub processing_stage: Option<String>,
    /// Custom attributes.
    pub custom: HashMap<String, String>,
}
impl GeospatialAttributes {
    /// Create new geospatial attributes.
    pub fn new() -> Self {
        Self::default()
    }
    /// Set bounding box.
    pub fn with_bbox(mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        self.bbox = Some([min_x, min_y, max_x, max_y]);
        self
    }
    /// Set CRS EPSG code.
    pub fn with_crs(mut self, epsg: i32) -> Self {
        self.crs_epsg = Some(epsg);
        self
    }
    /// Set geometry type.
    pub fn with_geometry_type(mut self, geom_type: impl Into<String>) -> Self {
        self.geometry_type = Some(geom_type.into());
        self
    }
    /// Set feature count.
    pub fn with_feature_count(mut self, count: u64) -> Self {
        self.feature_count = Some(count);
        self
    }
    /// Set raster dimensions.
    pub fn with_raster_dims(mut self, width: u32, height: u32) -> Self {
        self.raster_dims = Some([width, height]);
        self
    }
    /// Set band count.
    pub fn with_band_count(mut self, count: u32) -> Self {
        self.band_count = Some(count);
        self
    }
    /// Set data size.
    pub fn with_data_size(mut self, bytes: u64) -> Self {
        self.data_size_bytes = Some(bytes);
        self
    }
    /// Set driver name.
    pub fn with_driver(mut self, driver: impl Into<String>) -> Self {
        self.driver = Some(driver.into());
        self
    }
    /// Set layer name.
    pub fn with_layer(mut self, layer: impl Into<String>) -> Self {
        self.layer_name = Some(layer.into());
        self
    }
    /// Set resolution.
    pub fn with_resolution(mut self, x_res: f64, y_res: f64) -> Self {
        self.resolution = Some([x_res, y_res]);
        self
    }
    /// Set tile coordinates.
    pub fn with_tile(mut self, z: u32, x: u32, y: u32) -> Self {
        self.tile_coords = Some([z, x, y]);
        self
    }
    /// Set processing stage.
    pub fn with_processing_stage(mut self, stage: impl Into<String>) -> Self {
        self.processing_stage = Some(stage.into());
        self
    }
    /// Add custom attribute.
    pub fn with_custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom.insert(key.into(), value.into());
        self
    }
    /// Convert to OpenTelemetry KeyValue pairs.
    pub fn to_key_values(&self) -> Vec<KeyValue> {
        let mut kvs = Vec::new();
        if let Some(bbox) = &self.bbox {
            kvs.push(KeyValue::new("geo.bbox.min_x", bbox[0]));
            kvs.push(KeyValue::new("geo.bbox.min_y", bbox[1]));
            kvs.push(KeyValue::new("geo.bbox.max_x", bbox[2]));
            kvs.push(KeyValue::new("geo.bbox.max_y", bbox[3]));
        }
        if let Some(epsg) = self.crs_epsg {
            kvs.push(KeyValue::new("geo.crs.epsg", i64::from(epsg)));
        }
        if let Some(ref geom) = self.geometry_type {
            kvs.push(KeyValue::new("geo.geometry_type", geom.clone()));
        }
        if let Some(count) = self.feature_count {
            kvs.push(KeyValue::new("geo.feature_count", count as i64));
        }
        if let Some(dims) = &self.raster_dims {
            kvs.push(KeyValue::new("geo.raster.width", i64::from(dims[0])));
            kvs.push(KeyValue::new("geo.raster.height", i64::from(dims[1])));
        }
        if let Some(bands) = self.band_count {
            kvs.push(KeyValue::new("geo.raster.bands", i64::from(bands)));
        }
        if let Some(size) = self.data_size_bytes {
            kvs.push(KeyValue::new("geo.data_size_bytes", size as i64));
        }
        if let Some(ref driver) = self.driver {
            kvs.push(KeyValue::new("geo.driver", driver.clone()));
        }
        if let Some(ref layer) = self.layer_name {
            kvs.push(KeyValue::new("geo.layer", layer.clone()));
        }
        if let Some(res) = &self.resolution {
            kvs.push(KeyValue::new("geo.resolution.x", res[0]));
            kvs.push(KeyValue::new("geo.resolution.y", res[1]));
        }
        if let Some(tile) = &self.tile_coords {
            kvs.push(KeyValue::new("geo.tile.z", i64::from(tile[0])));
            kvs.push(KeyValue::new("geo.tile.x", i64::from(tile[1])));
            kvs.push(KeyValue::new("geo.tile.y", i64::from(tile[2])));
        }
        if let Some(ref stage) = self.processing_stage {
            kvs.push(KeyValue::new("geo.processing_stage", stage.clone()));
        }
        for (k, v) in &self.custom {
            kvs.push(KeyValue::new(format!("geo.custom.{}", k), v.clone()));
        }
        kvs
    }
}
/// Jaeger trace context format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JaegerTraceContext {
    /// Trace ID.
    pub trace_id: String,
    /// Span ID.
    pub span_id: String,
    /// Parent span ID.
    pub parent_span_id: String,
    /// Flags.
    pub flags: u8,
}
impl JaegerTraceContext {
    /// Parse from uber-trace-id header.
    pub fn parse(uber_trace_id: &str) -> Result<Self> {
        let parts: Vec<&str> = uber_trace_id.split(':').collect();
        if parts.len() != 4 {
            return Err(ObservabilityError::ContextPropagationFailed(
                "Invalid uber-trace-id format: expected 4 parts".to_string(),
            ));
        }
        let flags = u8::from_str_radix(parts[3], 16).map_err(|e| {
            ObservabilityError::ContextPropagationFailed(format!("Invalid flags: {}", e))
        })?;
        Ok(Self {
            trace_id: parts[0].to_string(),
            span_id: parts[1].to_string(),
            parent_span_id: parts[2].to_string(),
            flags,
        })
    }
    /// Convert to uber-trace-id header.
    pub fn to_header(&self) -> String {
        format!(
            "{}:{}:{}:{:x}",
            self.trace_id, self.span_id, self.parent_span_id, self.flags
        )
    }
    /// Check if sampled.
    pub fn is_sampled(&self) -> bool {
        self.flags & 0x01 != 0
    }
    /// Check if debug.
    pub fn is_debug(&self) -> bool {
        self.flags & 0x02 != 0
    }
}
/// Handle to an active span.
#[derive(Debug, Clone)]
pub struct SpanHandle {
    /// Trace ID.
    pub trace_id: String,
    /// Span ID.
    pub span_id: String,
    /// Parent span ID.
    pub parent_span_id: String,
    /// Span name.
    pub name: String,
    /// Whether sampled.
    pub sampled: bool,
    /// Start time.
    pub start_time: DateTime<Utc>,
    /// Attributes.
    pub attributes: Vec<(String, String)>,
    /// Events.
    pub events: Vec<SpanEvent>,
}
impl SpanHandle {
    /// Add an attribute.
    pub fn add_attribute(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attributes.push((key.into(), value.into()));
    }
    /// Add an event.
    pub fn add_event(&mut self, name: impl Into<String>) {
        self.events.push(SpanEvent {
            name: name.into(),
            timestamp: Utc::now(),
            attributes: Vec::new(),
        });
    }
    /// Add an event with attributes.
    pub fn add_event_with_attributes(
        &mut self,
        name: impl Into<String>,
        attributes: Vec<(String, String)>,
    ) {
        self.events.push(SpanEvent {
            name: name.into(),
            timestamp: Utc::now(),
            attributes,
        });
    }
    /// Get duration.
    pub fn duration(&self) -> chrono::Duration {
        Utc::now() - self.start_time
    }
}
/// Baggage manager for cross-service context propagation.
pub struct BaggageManager {
    /// Current baggage items.
    items: RwLock<HashMap<String, BaggageItem>>,
    /// Maximum number of items.
    max_items: usize,
    /// Maximum total size in bytes.
    max_size: usize,
}
impl BaggageManager {
    /// Create a new baggage manager with default limits.
    pub fn new() -> Self {
        Self {
            items: RwLock::new(HashMap::new()),
            max_items: 64,
            max_size: 8192,
        }
    }
    /// Create with custom limits.
    pub fn with_limits(max_items: usize, max_size: usize) -> Self {
        Self {
            items: RwLock::new(HashMap::new()),
            max_items,
            max_size,
        }
    }
    /// Set a baggage item.
    pub fn set(&self, key: impl Into<String>, value: impl Into<String>) -> Result<()> {
        self.set_with_metadata(key, value, BaggageMetadata::default())
    }
    /// Set a baggage item with metadata.
    pub fn set_with_metadata(
        &self,
        key: impl Into<String>,
        value: impl Into<String>,
        metadata: BaggageMetadata,
    ) -> Result<()> {
        let key = key.into();
        let value = value.into();
        let mut items = self.items.write();
        if !items.contains_key(&key) && items.len() >= self.max_items {
            return Err(ObservabilityError::InvalidConfig(format!(
                "Baggage item limit exceeded: {}",
                self.max_items
            )));
        }
        let current_size: usize = items.iter().map(|(k, v)| k.len() + v.value.len()).sum();
        let new_size = current_size + key.len() + value.len();
        if new_size > self.max_size {
            return Err(ObservabilityError::InvalidConfig(format!(
                "Baggage size limit exceeded: {} > {}",
                new_size, self.max_size
            )));
        }
        items.insert(
            key,
            BaggageItem {
                value,
                metadata,
                added_at: Utc::now(),
            },
        );
        Ok(())
    }
    /// Get a baggage item.
    pub fn get(&self, key: &str) -> Option<String> {
        let items = self.items.read();
        items.get(key).map(|item| item.value.clone())
    }
    /// Get a baggage item with metadata.
    pub fn get_with_metadata(&self, key: &str) -> Option<BaggageItem> {
        let items = self.items.read();
        items.get(key).cloned()
    }
    /// Remove a baggage item.
    pub fn remove(&self, key: &str) -> Option<String> {
        let mut items = self.items.write();
        items.remove(key).map(|item| item.value)
    }
    /// Get all items for propagation.
    pub fn get_propagation_items(&self) -> HashMap<String, String> {
        let items = self.items.read();
        let now = Utc::now();
        items
            .iter()
            .filter(|(_, item)| {
                if !item.metadata.propagate {
                    return false;
                }
                if item.metadata.ttl_seconds > 0 {
                    let elapsed = (now - item.added_at).num_seconds() as u64;
                    if elapsed > item.metadata.ttl_seconds {
                        return false;
                    }
                }
                true
            })
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect()
    }
    /// Clear all baggage items.
    pub fn clear(&self) {
        let mut items = self.items.write();
        items.clear();
    }
    /// Get the number of items.
    pub fn len(&self) -> usize {
        self.items.read().len()
    }
    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.items.read().is_empty()
    }
}
/// W3C Trace Context format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct W3CTraceContext {
    /// Version (always 00 for current spec).
    pub version: String,
    /// 32 hex character trace ID.
    pub trace_id: String,
    /// 16 hex character parent span ID.
    pub parent_id: String,
    /// Trace flags (sampled = 01).
    pub trace_flags: String,
}
impl W3CTraceContext {
    /// Parse from traceparent header value.
    pub fn parse(traceparent: &str) -> Result<Self> {
        let parts: Vec<&str> = traceparent.split('-').collect();
        if parts.len() != 4 {
            return Err(ObservabilityError::ContextPropagationFailed(
                "Invalid traceparent format: expected 4 parts".to_string(),
            ));
        }
        let version = parts[0].to_string();
        if version != "00" {
            return Err(ObservabilityError::ContextPropagationFailed(format!(
                "Unsupported traceparent version: {}",
                version
            )));
        }
        if parts[1].len() != 32 {
            return Err(ObservabilityError::ContextPropagationFailed(
                "Invalid trace_id length: expected 32 hex characters".to_string(),
            ));
        }
        if parts[2].len() != 16 {
            return Err(ObservabilityError::ContextPropagationFailed(
                "Invalid parent_id length: expected 16 hex characters".to_string(),
            ));
        }
        Ok(Self {
            version,
            trace_id: parts[1].to_string(),
            parent_id: parts[2].to_string(),
            trace_flags: parts[3].to_string(),
        })
    }
    /// Convert to traceparent header value.
    pub fn to_header(&self) -> String {
        format!(
            "{}-{}-{}-{}",
            self.version, self.trace_id, self.parent_id, self.trace_flags
        )
    }
    /// Check if the trace is sampled.
    pub fn is_sampled(&self) -> bool {
        if let Ok(flags) = u8::from_str_radix(&self.trace_flags, 16) {
            flags & 0x01 != 0
        } else {
            false
        }
    }
    /// Create a new trace context with a new span ID.
    pub fn with_new_span(&self) -> Self {
        let new_span_id = format!("{:016x}", fastrand::u64(..));
        Self {
            version: self.version.clone(),
            trace_id: self.trace_id.clone(),
            parent_id: new_span_id,
            trace_flags: self.trace_flags.clone(),
        }
    }
}
/// Extracted context from incoming request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedContext {
    /// Trace ID.
    pub trace_id: String,
    /// Span ID.
    pub span_id: String,
    /// Whether the trace is sampled.
    pub sampled: bool,
    /// Format the context was extracted from.
    pub format: PropagationFormat,
    /// Baggage items.
    pub baggage: HashMap<String, String>,
}
/// Result of a sampling decision.
#[derive(Debug, Clone)]
pub struct SamplingResult {
    /// The decision.
    pub decision: SamplingDecision,
    /// Attributes to add to the span.
    pub attributes: Vec<KeyValue>,
    /// Trace state modifications.
    pub trace_state: Option<String>,
}
/// Always-off sampler.
pub struct AlwaysOffSampler;
/// Manager for distributed tracing operations.
pub struct DistributedTraceManager {
    /// Service name.
    service_name: String,
    /// Context propagator.
    propagator: ContextPropagator,
    /// Baggage manager.
    baggage: BaggageManager,
    /// Active sampler.
    sampler: Arc<dyn Sampler>,
    /// Trace statistics.
    stats: TraceStats,
}
impl DistributedTraceManager {
    /// Create a new distributed trace manager.
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            propagator: ContextPropagator::new(),
            baggage: BaggageManager::new(),
            sampler: Arc::new(HeadBasedSampler::new(1.0)),
            stats: TraceStats::new(),
        }
    }
    /// Set the context propagator.
    pub fn with_propagator(mut self, propagator: ContextPropagator) -> Self {
        self.propagator = propagator;
        self
    }
    /// Set the sampler.
    pub fn with_sampler(mut self, sampler: Arc<dyn Sampler>) -> Self {
        self.sampler = sampler;
        self
    }
    /// Get the service name.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
    /// Get the baggage manager.
    pub fn baggage(&self) -> &BaggageManager {
        &self.baggage
    }
    /// Get trace statistics.
    pub fn stats(&self) -> &TraceStats {
        &self.stats
    }
    /// Extract trace context from headers.
    pub fn extract_context(&self, headers: &HashMap<String, String>) -> Option<ExtractedContext> {
        self.stats
            .context_extractions
            .fetch_add(1, Ordering::SeqCst);
        self.propagator.extract(headers)
    }
    /// Inject trace context into headers.
    pub fn inject_context(&self, ctx: &InjectionContext, headers: &mut HashMap<String, String>) {
        self.stats.context_injections.fetch_add(1, Ordering::SeqCst);
        self.propagator.inject(ctx, headers);
    }
    /// Create a new trace.
    pub fn start_trace(&self, name: impl Into<String>) -> TraceHandle {
        let trace_id = format!("{:032x}", fastrand::u128(..));
        let span_id = format!("{:016x}", fastrand::u64(..));
        let name = name.into();
        self.stats.traces_started.fetch_add(1, Ordering::SeqCst);
        let result = self
            .sampler
            .should_sample(None, &trace_id, &name, SpanKind::Server, &[]);
        match result.decision {
            SamplingDecision::Sample => {
                self.stats.traces_sampled.fetch_add(1, Ordering::SeqCst);
            }
            SamplingDecision::Drop => {
                self.stats.traces_dropped.fetch_add(1, Ordering::SeqCst);
            }
            SamplingDecision::RecordOnly => {}
        }
        TraceHandle {
            trace_id,
            span_id,
            name,
            sampled: result.decision == SamplingDecision::Sample,
            start_time: Utc::now(),
        }
    }
    /// Continue a trace from extracted context.
    pub fn continue_trace(&self, ctx: &ExtractedContext, name: impl Into<String>) -> TraceHandle {
        let span_id = format!("{:016x}", fastrand::u64(..));
        let name = name.into();
        self.stats.traces_started.fetch_add(1, Ordering::SeqCst);
        self.stats.spans_created.fetch_add(1, Ordering::SeqCst);
        if ctx.sampled {
            self.stats.traces_sampled.fetch_add(1, Ordering::SeqCst);
        } else {
            self.stats.traces_dropped.fetch_add(1, Ordering::SeqCst);
        }
        TraceHandle {
            trace_id: ctx.trace_id.clone(),
            span_id,
            name,
            sampled: ctx.sampled,
            start_time: Utc::now(),
        }
    }
    /// Create a new span within an existing trace.
    pub fn create_span(
        &self,
        trace_id: &str,
        parent_span_id: &str,
        name: impl Into<String>,
        sampled: bool,
    ) -> SpanHandle {
        let span_id = format!("{:016x}", fastrand::u64(..));
        self.stats.spans_created.fetch_add(1, Ordering::SeqCst);
        SpanHandle {
            trace_id: trace_id.to_string(),
            span_id,
            parent_span_id: parent_span_id.to_string(),
            name: name.into(),
            sampled,
            start_time: Utc::now(),
            attributes: Vec::new(),
            events: Vec::new(),
        }
    }
    /// Create a geospatial span builder.
    pub fn geospatial_span(&self, name: impl Into<String>) -> GeospatialSpanBuilder {
        GeospatialSpanBuilder::new(name)
    }
}
/// Handle to an active trace.
#[derive(Debug, Clone)]
pub struct TraceHandle {
    /// Trace ID.
    pub trace_id: String,
    /// Current span ID.
    pub span_id: String,
    /// Trace name.
    pub name: String,
    /// Whether sampled.
    pub sampled: bool,
    /// Start time.
    pub start_time: DateTime<Utc>,
}
impl TraceHandle {
    /// Create injection context from this trace.
    pub fn to_injection_context(&self) -> InjectionContext {
        InjectionContext {
            trace_id: self.trace_id.clone(),
            span_id: self.span_id.clone(),
            sampled: self.sampled,
            baggage: HashMap::new(),
        }
    }
    /// Get duration since start.
    pub fn duration(&self) -> chrono::Duration {
        Utc::now() - self.start_time
    }
}
/// OpenTelemetry header injector.
pub struct OtelHeaderInjector<'a> {
    pub(crate) headers: &'a mut HashMap<String, String>,
}
impl<'a> OtelHeaderInjector<'a> {
    /// Create a new injector.
    pub fn new(headers: &'a mut HashMap<String, String>) -> Self {
        Self { headers }
    }
}
/// Head-based sampler using trace ID ratio.
pub struct HeadBasedSampler {
    /// Sampling ratio (0.0 to 1.0).
    pub(super) ratio: f64,
    /// Description.
    pub(super) description: String,
}
impl HeadBasedSampler {
    /// Create a new head-based sampler.
    pub fn new(ratio: f64) -> Self {
        let clamped = ratio.clamp(0.0, 1.0);
        Self {
            ratio: clamped,
            description: format!("HeadBasedSampler(ratio={})", clamped),
        }
    }
    pub(crate) fn hash_trace_id(&self, trace_id: &str) -> f64 {
        let mut hash: u64 = 0;
        for byte in trace_id.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
        }
        (hash as f64) / (u64::MAX as f64)
    }
}
/// Buffer for a single trace.
#[derive(Debug, Clone)]
pub struct TraceBuffer {
    /// Spans in this trace.
    pub spans: Vec<BufferedSpan>,
    /// Whether an error has been recorded.
    pub has_error: bool,
    /// Start time of the trace.
    pub start_time: DateTime<Utc>,
    /// Total duration so far.
    pub duration_ms: u64,
}
/// Builder for creating geospatial operation spans.
pub struct GeospatialSpanBuilder {
    /// Span name.
    name: String,
    /// Span kind.
    kind: SpanKind,
    /// Geospatial attributes.
    geo_attrs: GeospatialAttributes,
    /// Additional attributes.
    attributes: Vec<KeyValue>,
    /// Parent context.
    parent_context: Option<Context>,
}
impl GeospatialSpanBuilder {
    /// Create a new geospatial span builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: SpanKind::Internal,
            geo_attrs: GeospatialAttributes::new(),
            attributes: Vec::new(),
            parent_context: None,
        }
    }
    /// Set span kind.
    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }
    /// Set geospatial attributes.
    pub fn with_geo_attributes(mut self, attrs: GeospatialAttributes) -> Self {
        self.geo_attrs = attrs;
        self
    }
    /// Add a custom attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes
            .push(KeyValue::new(key.into(), value.into()));
        self
    }
    /// Set parent context.
    pub fn with_parent(mut self, context: Context) -> Self {
        self.parent_context = Some(context);
        self
    }
    /// Build and start the span.
    pub fn start(self) -> Context {
        let tracer = global::tracer("oxigdal");
        let mut all_attributes = self.geo_attrs.to_key_values();
        all_attributes.extend(self.attributes);
        let mut builder = tracer.span_builder(self.name);
        builder.span_kind = Some(self.kind);
        builder.attributes = Some(all_attributes);
        let span = if let Some(parent) = self.parent_context {
            tracer.build_with_context(builder, &parent)
        } else {
            builder.start(&tracer)
        };
        Context::current().with_span(span)
    }
}
/// Multi-format context propagator.
pub struct ContextPropagator {
    /// Primary format for injection.
    primary_format: PropagationFormat,
    /// Additional formats to inject.
    inject_formats: Vec<PropagationFormat>,
    /// Formats to attempt extraction from.
    extract_formats: Vec<PropagationFormat>,
}
impl ContextPropagator {
    /// Create a new context propagator with default W3C format.
    pub fn new() -> Self {
        Self {
            primary_format: PropagationFormat::W3CTraceContext,
            inject_formats: vec![],
            extract_formats: vec![
                PropagationFormat::W3CTraceContext,
                PropagationFormat::B3Single,
                PropagationFormat::B3Multi,
                PropagationFormat::Jaeger,
            ],
        }
    }
    /// Set the primary format for injection.
    pub fn with_primary_format(mut self, format: PropagationFormat) -> Self {
        self.primary_format = format;
        self
    }
    /// Add additional formats to inject.
    pub fn with_inject_formats(mut self, formats: Vec<PropagationFormat>) -> Self {
        self.inject_formats = formats;
        self
    }
    /// Set formats to attempt extraction from.
    pub fn with_extract_formats(mut self, formats: Vec<PropagationFormat>) -> Self {
        self.extract_formats = formats;
        self
    }
    /// Extract trace context from headers.
    pub fn extract(&self, headers: &HashMap<String, String>) -> Option<ExtractedContext> {
        for format in &self.extract_formats {
            if let Some(ctx) = self.extract_format(headers, *format) {
                return Some(ctx);
            }
        }
        None
    }
    fn extract_format(
        &self,
        headers: &HashMap<String, String>,
        format: PropagationFormat,
    ) -> Option<ExtractedContext> {
        match format {
            PropagationFormat::W3CTraceContext => headers.get("traceparent").and_then(|v| {
                W3CTraceContext::parse(v).ok().map(|ctx| {
                    let sampled = ctx.is_sampled();
                    ExtractedContext {
                        trace_id: ctx.trace_id,
                        span_id: ctx.parent_id,
                        sampled,
                        format,
                        baggage: self.extract_baggage(headers),
                    }
                })
            }),
            PropagationFormat::B3Single => headers.get("b3").and_then(|v| {
                B3TraceContext::parse_single(v)
                    .ok()
                    .map(|ctx| ExtractedContext {
                        trace_id: ctx.trace_id,
                        span_id: ctx.span_id,
                        sampled: ctx.sampled.as_deref() == Some("1") || ctx.debug,
                        format,
                        baggage: self.extract_baggage(headers),
                    })
            }),
            PropagationFormat::B3Multi => {
                if headers.contains_key("x-b3-traceid") {
                    B3TraceContext::parse_multi(headers)
                        .ok()
                        .map(|ctx| ExtractedContext {
                            trace_id: ctx.trace_id,
                            span_id: ctx.span_id,
                            sampled: ctx.sampled.as_deref() == Some("1") || ctx.debug,
                            format,
                            baggage: self.extract_baggage(headers),
                        })
                } else {
                    None
                }
            }
            PropagationFormat::Jaeger => headers.get("uber-trace-id").and_then(|v| {
                JaegerTraceContext::parse(v).ok().map(|ctx| {
                    let sampled = ctx.is_sampled();
                    ExtractedContext {
                        trace_id: ctx.trace_id,
                        span_id: ctx.span_id,
                        sampled,
                        format,
                        baggage: self.extract_baggage(headers),
                    }
                })
            }),
        }
    }
    fn extract_baggage(&self, headers: &HashMap<String, String>) -> HashMap<String, String> {
        let mut baggage = HashMap::new();
        if let Some(baggage_header) = headers.get("baggage") {
            for item in baggage_header.split(',') {
                if let Some((key, value)) = item.trim().split_once('=') {
                    baggage.insert(key.to_string(), value.to_string());
                }
            }
        }
        for (key, value) in headers {
            if let Some(baggage_key) = key.strip_prefix("uberctx-") {
                baggage.insert(baggage_key.to_string(), value.clone());
            }
        }
        baggage
    }
    /// Inject trace context into headers.
    pub fn inject(&self, ctx: &InjectionContext, headers: &mut HashMap<String, String>) {
        self.inject_format(ctx, headers, self.primary_format);
        for format in &self.inject_formats {
            if *format != self.primary_format {
                self.inject_format(ctx, headers, *format);
            }
        }
        self.inject_baggage(&ctx.baggage, headers);
    }
    fn inject_format(
        &self,
        ctx: &InjectionContext,
        headers: &mut HashMap<String, String>,
        format: PropagationFormat,
    ) {
        match format {
            PropagationFormat::W3CTraceContext => {
                let trace_flags = if ctx.sampled { "01" } else { "00" };
                let traceparent = format!("00-{}-{}-{}", ctx.trace_id, ctx.span_id, trace_flags);
                headers.insert("traceparent".to_string(), traceparent);
            }
            PropagationFormat::B3Single => {
                let sampled = if ctx.sampled { "1" } else { "0" };
                let b3 = format!("{}-{}-{}", ctx.trace_id, ctx.span_id, sampled);
                headers.insert("b3".to_string(), b3);
            }
            PropagationFormat::B3Multi => {
                headers.insert("x-b3-traceid".to_string(), ctx.trace_id.clone());
                headers.insert("x-b3-spanid".to_string(), ctx.span_id.clone());
                headers.insert(
                    "x-b3-sampled".to_string(),
                    if ctx.sampled { "1" } else { "0" }.to_string(),
                );
            }
            PropagationFormat::Jaeger => {
                let flags = if ctx.sampled { 1 } else { 0 };
                let uber_trace_id = format!("{}:{}:0:{:x}", ctx.trace_id, ctx.span_id, flags);
                headers.insert("uber-trace-id".to_string(), uber_trace_id);
            }
        }
    }
    fn inject_baggage(
        &self,
        baggage: &HashMap<String, String>,
        headers: &mut HashMap<String, String>,
    ) {
        if baggage.is_empty() {
            return;
        }
        let baggage_str: Vec<String> = baggage
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        headers.insert("baggage".to_string(), baggage_str.join(","));
        for (k, v) in baggage {
            headers.insert(format!("uberctx-{}", k), v.clone());
        }
    }
}
/// Parent-based sampler that follows parent's sampling decision.
pub struct ParentBasedSampler {
    /// Sampler for root spans.
    pub(super) root_sampler: Box<dyn Sampler>,
    /// Sampler for remote parent sampled.
    pub(super) remote_parent_sampled: Box<dyn Sampler>,
    /// Sampler for remote parent not sampled.
    pub(super) remote_parent_not_sampled: Box<dyn Sampler>,
    /// Description.
    pub(super) description: String,
}
impl ParentBasedSampler {
    /// Create a new parent-based sampler.
    pub fn new(root_sampler: Box<dyn Sampler>) -> Self {
        Self {
            root_sampler,
            remote_parent_sampled: Box::new(AlwaysOnSampler),
            remote_parent_not_sampled: Box::new(AlwaysOffSampler),
            description: "ParentBasedSampler".to_string(),
        }
    }
    /// Set sampler for remote parent sampled case.
    pub fn with_remote_parent_sampled(mut self, sampler: Box<dyn Sampler>) -> Self {
        self.remote_parent_sampled = sampler;
        self
    }
    /// Set sampler for remote parent not sampled case.
    pub fn with_remote_parent_not_sampled(mut self, sampler: Box<dyn Sampler>) -> Self {
        self.remote_parent_not_sampled = sampler;
        self
    }
}
/// A buffered span for tail-based sampling.
#[derive(Debug, Clone)]
pub struct BufferedSpan {
    /// Span ID.
    pub span_id: String,
    /// Span name.
    pub name: String,
    /// Span kind.
    pub kind: SpanKind,
    /// Attributes.
    pub attributes: Vec<KeyValue>,
    /// Whether this span has an error.
    pub has_error: bool,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}
/// Metadata for baggage items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaggageMetadata {
    /// Whether this item should be propagated to downstream services.
    pub propagate: bool,
    /// Service that added this item.
    pub source_service: Option<String>,
    /// TTL in seconds (0 = no expiry).
    pub ttl_seconds: u64,
}
