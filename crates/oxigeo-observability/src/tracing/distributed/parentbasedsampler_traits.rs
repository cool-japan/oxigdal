//! # ParentBasedSampler - Trait Implementations
//!
//! This module contains trait implementations for `ParentBasedSampler`.
//!
//! ## Implemented Traits
//!
//! - `Sampler`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use opentelemetry::KeyValue;
use opentelemetry::trace::SpanKind;

use super::functions::Sampler;
use super::types::{ExtractedContext, ParentBasedSampler, SamplingResult};

impl Sampler for ParentBasedSampler {
    fn should_sample(
        &self,
        parent_context: Option<&ExtractedContext>,
        trace_id: &str,
        name: &str,
        span_kind: SpanKind,
        attributes: &[KeyValue],
    ) -> SamplingResult {
        match parent_context {
            Some(parent) if parent.sampled => self.remote_parent_sampled.should_sample(
                Some(parent),
                trace_id,
                name,
                span_kind,
                attributes,
            ),
            Some(parent) => self.remote_parent_not_sampled.should_sample(
                Some(parent),
                trace_id,
                name,
                span_kind,
                attributes,
            ),
            None => self
                .root_sampler
                .should_sample(None, trace_id, name, span_kind, attributes),
        }
    }
    fn description(&self) -> &str {
        &self.description
    }
}
