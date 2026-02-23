//! # AdaptiveSampler - Trait Implementations
//!
//! This module contains trait implementations for `AdaptiveSampler`.
//!
//! ## Implemented Traits
//!
//! - `Sampler`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use opentelemetry::KeyValue;
use opentelemetry::trace::SpanKind;
use std::sync::atomic::Ordering;

use super::functions::Sampler;
use super::types::{AdaptiveSampler, ExtractedContext, SamplingDecision, SamplingResult};

impl Sampler for AdaptiveSampler {
    fn should_sample(
        &self,
        _parent_context: Option<&ExtractedContext>,
        _trace_id: &str,
        _name: &str,
        _span_kind: SpanKind,
        _attributes: &[KeyValue],
    ) -> SamplingResult {
        let ratio = self.calculate_ratio();
        let sample = fastrand::f64() < ratio;
        if sample {
            self.samples_count.fetch_add(1, Ordering::SeqCst);
        }
        SamplingResult {
            decision: if sample {
                SamplingDecision::Sample
            } else {
                SamplingDecision::Drop
            },
            attributes: vec![KeyValue::new("sampler.type", "adaptive")],
            trace_state: None,
        }
    }
    fn description(&self) -> &str {
        &self.description
    }
}
