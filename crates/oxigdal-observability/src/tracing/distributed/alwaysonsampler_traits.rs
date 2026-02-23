//! # AlwaysOnSampler - Trait Implementations
//!
//! This module contains trait implementations for `AlwaysOnSampler`.
//!
//! ## Implemented Traits
//!
//! - `Sampler`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use opentelemetry::KeyValue;
use opentelemetry::trace::SpanKind;

use super::functions::Sampler;
use super::types::{AlwaysOnSampler, ExtractedContext, SamplingDecision, SamplingResult};

impl Sampler for AlwaysOnSampler {
    fn should_sample(
        &self,
        _parent_context: Option<&ExtractedContext>,
        _trace_id: &str,
        _name: &str,
        _span_kind: SpanKind,
        _attributes: &[KeyValue],
    ) -> SamplingResult {
        SamplingResult {
            decision: SamplingDecision::Sample,
            attributes: vec![],
            trace_state: None,
        }
    }
    fn description(&self) -> &str {
        "AlwaysOnSampler"
    }
}
