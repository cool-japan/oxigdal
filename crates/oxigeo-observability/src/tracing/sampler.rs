//! Sampling strategies for distributed tracing.

use opentelemetry_sdk::trace::Sampler;

/// Sampling strategy configuration.
#[derive(Debug, Clone)]
pub enum SamplingStrategy {
    /// Always sample (100% of traces).
    AlwaysOn,

    /// Never sample (0% of traces).
    AlwaysOff,

    /// Sample based on trace ID ratio (0.0 to 1.0).
    TraceIdRatio(f64),

    /// Parent-based sampling.
    ParentBased,
}

impl SamplingStrategy {
    /// Convert to OpenTelemetry sampler.
    pub fn to_sampler(&self) -> Sampler {
        match self {
            SamplingStrategy::AlwaysOn => Sampler::AlwaysOn,
            SamplingStrategy::AlwaysOff => Sampler::AlwaysOff,
            SamplingStrategy::TraceIdRatio(ratio) => {
                Sampler::TraceIdRatioBased(ratio.clamp(0.0, 1.0))
            }
            SamplingStrategy::ParentBased => Sampler::ParentBased(Box::new(Sampler::AlwaysOn)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sampling_strategies() {
        let sampler = SamplingStrategy::AlwaysOn.to_sampler();
        assert!(matches!(sampler, Sampler::AlwaysOn));

        let sampler = SamplingStrategy::AlwaysOff.to_sampler();
        assert!(matches!(sampler, Sampler::AlwaysOff));

        let sampler = SamplingStrategy::TraceIdRatio(0.5).to_sampler();
        assert!(matches!(sampler, Sampler::TraceIdRatioBased(_)));
    }
}
