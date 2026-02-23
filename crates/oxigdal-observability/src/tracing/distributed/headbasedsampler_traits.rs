//! # HeadBasedSampler - Trait Implementations
//!
//! This module contains trait implementations for `HeadBasedSampler`.
//!
//! ## Implemented Traits
//!
//! - `Sampler`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use opentelemetry::KeyValue;
use opentelemetry::trace::SpanKind;

use super::functions::Sampler;
use super::types::{ExtractedContext, HeadBasedSampler, SamplingDecision, SamplingResult};

impl Sampler for HeadBasedSampler {
    fn should_sample(
        &self,
        _parent_context: Option<&ExtractedContext>,
        trace_id: &str,
        _name: &str,
        _span_kind: SpanKind,
        _attributes: &[KeyValue],
    ) -> SamplingResult {
        let hash = self.hash_trace_id(trace_id);
        let decision = if hash < self.ratio {
            SamplingDecision::Sample
        } else {
            SamplingDecision::Drop
        };
        SamplingResult {
            decision,
            attributes: vec![KeyValue::new("sampler.type", "head_based")],
            trace_state: None,
        }
    }
    fn description(&self) -> &str {
        &self.description
    }
}
