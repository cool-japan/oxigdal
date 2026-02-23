//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use opentelemetry::trace::SpanKind;
use opentelemetry::{Context, KeyValue, global};
use std::collections::HashMap;

use super::types::{ExtractedContext, OtelHeaderExtractor, OtelHeaderInjector, SamplingResult};

/// Trait for custom samplers.
pub trait Sampler: Send + Sync {
    /// Make a sampling decision.
    fn should_sample(
        &self,
        parent_context: Option<&ExtractedContext>,
        trace_id: &str,
        name: &str,
        span_kind: SpanKind,
        attributes: &[KeyValue],
    ) -> SamplingResult;
    /// Get a description of this sampler.
    fn description(&self) -> &str;
}
/// Extract OpenTelemetry context from headers using global propagator.
pub fn extract_otel_context(headers: &HashMap<String, String>) -> Context {
    global::get_text_map_propagator(|propagator| {
        propagator.extract(&OtelHeaderExtractor::new(headers))
    })
}
/// Inject OpenTelemetry context into headers using global propagator.
pub fn inject_otel_context(ctx: &Context, headers: &mut HashMap<String, String>) {
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(ctx, &mut OtelHeaderInjector::new(headers))
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracing::distributed::{
        AlwaysOffSampler, AlwaysOnSampler, B3TraceContext, BaggageManager, BaggageMetadata,
        ContextPropagator, DistributedTraceManager, GeospatialAttributes, HeadBasedSampler,
        InjectionContext, JaegerTraceContext, SamplingDecision, SpanHandle, TraceStats,
        W3CTraceContext,
    };
    use chrono::Utc;
    #[test]
    fn test_w3c_trace_context_parse() {
        let traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let ctx = W3CTraceContext::parse(traceparent).expect("Should parse");
        assert_eq!(ctx.version, "00");
        assert_eq!(ctx.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(ctx.parent_id, "b7ad6b7169203331");
        assert_eq!(ctx.trace_flags, "01");
        assert!(ctx.is_sampled());
    }
    #[test]
    fn test_w3c_trace_context_roundtrip() {
        let original = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let ctx = W3CTraceContext::parse(original).expect("Should parse");
        let header = ctx.to_header();
        assert_eq!(original, header);
    }
    #[test]
    fn test_b3_single_parse() {
        let b3 = "80f198ee56343ba864fe8b2a57d3eff7-e457b5a2e4d86bd1-1";
        let ctx = B3TraceContext::parse_single(b3).expect("Should parse");
        assert_eq!(ctx.trace_id, "80f198ee56343ba864fe8b2a57d3eff7");
        assert_eq!(ctx.span_id, "e457b5a2e4d86bd1");
        assert_eq!(ctx.sampled, Some("1".to_string()));
    }
    #[test]
    fn test_b3_multi_parse() {
        let mut headers = HashMap::new();
        headers.insert(
            "x-b3-traceid".to_string(),
            "80f198ee56343ba864fe8b2a57d3eff7".to_string(),
        );
        headers.insert("x-b3-spanid".to_string(), "e457b5a2e4d86bd1".to_string());
        headers.insert("x-b3-sampled".to_string(), "1".to_string());
        let ctx = B3TraceContext::parse_multi(&headers).expect("Should parse");
        assert_eq!(ctx.trace_id, "80f198ee56343ba864fe8b2a57d3eff7");
    }
    #[test]
    fn test_jaeger_trace_context_parse() {
        let uber_trace_id = "abc123:def456:0:1";
        let ctx = JaegerTraceContext::parse(uber_trace_id).expect("Should parse");
        assert_eq!(ctx.trace_id, "abc123");
        assert_eq!(ctx.span_id, "def456");
        assert_eq!(ctx.parent_span_id, "0");
        assert!(ctx.is_sampled());
    }
    #[test]
    fn test_context_propagator_extract_w3c() {
        let propagator = ContextPropagator::new();
        let mut headers = HashMap::new();
        headers.insert(
            "traceparent".to_string(),
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string(),
        );
        let ctx = propagator.extract(&headers).expect("Should extract");
        assert_eq!(ctx.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert!(ctx.sampled);
    }
    #[test]
    fn test_context_propagator_inject_w3c() {
        let propagator = ContextPropagator::new();
        let ctx = InjectionContext::new(
            "0af7651916cd43dd8448eb211c80319c".to_string(),
            "b7ad6b7169203331".to_string(),
            true,
        );
        let mut headers: HashMap<String, String> = HashMap::new();
        propagator.inject(&ctx, &mut headers);
        assert!(headers.contains_key("traceparent"));
        assert!(
            headers
                .get("traceparent")
                .expect("Should have traceparent")
                .contains("0af7651916cd43dd8448eb211c80319c")
        );
    }
    #[test]
    fn test_baggage_manager() {
        let baggage = BaggageManager::new();
        baggage.set("user_id", "12345").expect("Should set");
        baggage.set("tenant", "acme").expect("Should set");
        assert_eq!(baggage.get("user_id"), Some("12345".to_string()));
        assert_eq!(baggage.len(), 2);
        let items = baggage.get_propagation_items();
        assert_eq!(items.len(), 2);
    }
    #[test]
    fn test_baggage_ttl() {
        let baggage = BaggageManager::new();
        let metadata = BaggageMetadata {
            propagate: true,
            source_service: Some("test".to_string()),
            ttl_seconds: 0,
        };
        baggage
            .set_with_metadata("key", "value", metadata)
            .expect("Should set");
        let item = baggage.get_with_metadata("key").expect("Should get");
        assert_eq!(item.metadata.source_service, Some("test".to_string()));
    }
    #[test]
    fn test_head_based_sampler() {
        let sampler = HeadBasedSampler::new(1.0);
        let result = sampler.should_sample(None, "abc123", "test_span", SpanKind::Server, &[]);
        assert_eq!(result.decision, SamplingDecision::Sample);
    }
    #[test]
    fn test_always_on_sampler() {
        let sampler = AlwaysOnSampler;
        let result = sampler.should_sample(None, "abc123", "test_span", SpanKind::Server, &[]);
        assert_eq!(result.decision, SamplingDecision::Sample);
    }
    #[test]
    fn test_always_off_sampler() {
        let sampler = AlwaysOffSampler;
        let result = sampler.should_sample(None, "abc123", "test_span", SpanKind::Server, &[]);
        assert_eq!(result.decision, SamplingDecision::Drop);
    }
    #[test]
    fn test_geospatial_attributes() {
        let attrs = GeospatialAttributes::new()
            .with_bbox(-180.0, -90.0, 180.0, 90.0)
            .with_crs(4326)
            .with_geometry_type("Polygon")
            .with_feature_count(1000)
            .with_driver("GeoTIFF");
        let kvs = attrs.to_key_values();
        assert!(kvs.iter().any(|kv| kv.key.as_str() == "geo.crs.epsg"));
        assert!(kvs.iter().any(|kv| kv.key.as_str() == "geo.driver"));
    }
    #[test]
    fn test_distributed_trace_manager() {
        let manager = DistributedTraceManager::new("test-service");
        let trace = manager.start_trace("test_operation");
        assert!(!trace.trace_id.is_empty());
        assert!(!trace.span_id.is_empty());
        let span = manager.create_span(&trace.trace_id, &trace.span_id, "child_span", true);
        assert_eq!(span.trace_id, trace.trace_id);
        assert_eq!(span.parent_span_id, trace.span_id);
    }
    #[test]
    fn test_trace_stats() {
        let stats = TraceStats::new();
        assert_eq!(stats.traces_started(), 0);
        assert_eq!(stats.spans_created(), 0);
        assert_eq!(stats.sampling_rate(), 0.0);
    }
    #[test]
    fn test_injection_context_with_baggage() {
        let ctx = InjectionContext::new("trace123".to_string(), "span456".to_string(), true)
            .with_baggage("user_id", "789")
            .with_baggage("tenant", "acme");
        assert_eq!(ctx.baggage.len(), 2);
        assert_eq!(ctx.baggage.get("user_id"), Some(&"789".to_string()));
    }
    #[test]
    fn test_span_handle_events() {
        let mut span = SpanHandle {
            trace_id: "trace123".to_string(),
            span_id: "span456".to_string(),
            parent_span_id: "parent789".to_string(),
            name: "test_span".to_string(),
            sampled: true,
            start_time: Utc::now(),
            attributes: Vec::new(),
            events: Vec::new(),
        };
        span.add_attribute("key", "value");
        span.add_event("event1");
        span.add_event_with_attributes("event2", vec![("attr".to_string(), "val".to_string())]);
        assert_eq!(span.attributes.len(), 1);
        assert_eq!(span.events.len(), 2);
    }
}
