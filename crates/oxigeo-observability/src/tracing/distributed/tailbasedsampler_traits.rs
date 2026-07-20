//! # TailBasedSampler - Trait Implementations
//!
//! This module contains trait implementations for `TailBasedSampler`.
//!
//! ## Implemented Traits
//!
//! - `Sampler`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use opentelemetry::KeyValue;
use opentelemetry::trace::SpanKind;

use super::functions::Sampler;
use super::types::{ExtractedContext, SamplingDecision, SamplingResult, TailBasedSampler};

impl Sampler for TailBasedSampler {
    fn should_sample(
        &self,
        parent_context: Option<&ExtractedContext>,
        trace_id: &str,
        name: &str,
        span_kind: SpanKind,
        attributes: &[KeyValue],
    ) -> SamplingResult {
        let inner_result =
            self.inner
                .should_sample(parent_context, trace_id, name, span_kind, attributes);
        if inner_result.decision == SamplingDecision::Sample {
            return inner_result;
        }
        SamplingResult {
            decision: SamplingDecision::RecordOnly,
            attributes: vec![KeyValue::new("sampler.type", "tail_based")],
            trace_state: None,
        }
    }
    fn description(&self) -> &str {
        &self.description
    }
}
