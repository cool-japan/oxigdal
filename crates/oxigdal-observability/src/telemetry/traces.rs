//! Distributed tracing with OpenTelemetry.

use crate::error::{ObservabilityError, Result};
use crate::telemetry::TelemetryConfig;
use opentelemetry::global;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{Config, RandomIdGenerator, Sampler, TracerProvider};

/// Initialize distributed tracing based on configuration.
pub async fn init_tracing(config: &TelemetryConfig, resource: Resource) -> Result<()> {
    let sampler = create_sampler(config.sampling_rate);

    let tracer_config = Config::default()
        .with_sampler(sampler)
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource);

    // Create tracer provider based on configured exporters
    let tracer_provider = if let Some(ref endpoint) = config.otlp_endpoint {
        #[cfg(feature = "otlp")]
        {
            create_otlp_tracer_provider(endpoint, tracer_config).await?
        }
        #[cfg(not(feature = "otlp"))]
        {
            return Err(ObservabilityError::ConfigError(
                "OTLP feature not enabled".to_string(),
            ));
        }
    } else if let Some(ref _jaeger_ep) = config.jaeger_endpoint {
        #[cfg(feature = "jaeger")]
        {
            create_jaeger_tracer_provider(_jaeger_ep, tracer_config)?
        }
        #[cfg(not(feature = "jaeger"))]
        {
            return Err(ObservabilityError::ConfigError(
                "Jaeger feature not enabled".to_string(),
            ));
        }
    } else {
        // Default to stdout exporter for development
        create_stdout_tracer_provider(tracer_config)?
    };

    // Set global tracer provider
    global::set_tracer_provider(tracer_provider);

    Ok(())
}

/// Create sampler based on sampling rate.
fn create_sampler(sampling_rate: f64) -> Sampler {
    if sampling_rate >= 1.0 {
        Sampler::AlwaysOn
    } else if sampling_rate <= 0.0 {
        Sampler::AlwaysOff
    } else {
        Sampler::TraceIdRatioBased(sampling_rate)
    }
}

/// Create OTLP tracer provider.
#[cfg(feature = "otlp")]
async fn create_otlp_tracer_provider(endpoint: &str, config: Config) -> Result<TracerProvider> {
    use opentelemetry_otlp::WithExportConfig;

    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint)
        .build_span_exporter()
        .map_err(|e| ObservabilityError::TraceExportFailed(e.to_string()))?;

    let provider = TracerProvider::builder()
        .with_config(config)
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .build();

    Ok(provider)
}

/// Create Jaeger tracer provider.
/// Note: opentelemetry-jaeger crate is deprecated. Using OTLP with Jaeger's native OTLP support
/// is now recommended. This function provides a fallback using the SDK's simple exporter.
#[cfg(feature = "jaeger")]
fn create_jaeger_tracer_provider(endpoint: &str, config: Config) -> Result<TracerProvider> {
    // The opentelemetry-jaeger crate is deprecated as of 2023-11
    // Modern Jaeger supports OTLP natively, so we recommend using OTLP instead.
    // For backwards compatibility, we create a provider with a stdout exporter
    // and warn that Jaeger agent support is deprecated.
    tracing::warn!(
        "Jaeger agent endpoint '{}' specified, but opentelemetry-jaeger is deprecated. \
         Consider using OTLP endpoint with Jaeger's native OTLP support instead.",
        endpoint
    );

    // Fall back to stdout for now - production should use OTLP
    let exporter = opentelemetry_stdout::SpanExporter::default();
    let provider = TracerProvider::builder()
        .with_config(config)
        .with_simple_exporter(exporter)
        .build();

    Ok(provider)
}

/// Create stdout tracer provider for development.
fn create_stdout_tracer_provider(config: Config) -> Result<TracerProvider> {
    let exporter = opentelemetry_stdout::SpanExporter::default();

    let provider = TracerProvider::builder()
        .with_config(config)
        .with_simple_exporter(exporter)
        .build();

    Ok(provider)
}

/// Span builder for creating custom spans.
pub struct SpanBuilder {
    name: String,
    attributes: Vec<(String, String)>,
}

impl SpanBuilder {
    /// Create a new span builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: Vec::new(),
        }
    }

    /// Add an attribute to the span.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.push((key.into(), value.into()));
        self
    }

    /// Build and start the span.
    pub fn start(self) -> tracing::Span {
        // Note: tracing span names are compile-time static, so we use a fixed name
        // The user-provided name is stored as the "span.name" field
        let span = tracing::info_span!(
            target: "oxigdal",
            "custom_span",
            span.name = %self.name
        );

        // Record attributes
        for (key, value) in self.attributes {
            span.record(key.as_str(), tracing::field::display(&value));
        }

        span
    }
}

/// Context propagation utilities.
pub mod context {
    use opentelemetry::global;
    use opentelemetry::propagation::{Extractor, Injector};
    use std::collections::HashMap;

    /// Extract context from HTTP headers.
    pub fn extract_from_headers(headers: &HashMap<String, String>) -> opentelemetry::Context {
        global::get_text_map_propagator(|propagator| propagator.extract(&HeaderExtractor(headers)))
    }

    /// Inject context into HTTP headers.
    pub fn inject_to_headers(
        context: &opentelemetry::Context,
        headers: &mut HashMap<String, String>,
    ) {
        global::get_text_map_propagator(|propagator| {
            propagator.inject_context(context, &mut HeaderInjector(headers))
        })
    }

    struct HeaderExtractor<'a>(&'a HashMap<String, String>);

    impl<'a> Extractor for HeaderExtractor<'a> {
        fn get(&self, key: &str) -> Option<&str> {
            self.0.get(key).map(|v| v.as_str())
        }

        fn keys(&self) -> Vec<&str> {
            self.0.keys().map(|k| k.as_str()).collect()
        }
    }

    struct HeaderInjector<'a>(&'a mut HashMap<String, String>);

    impl<'a> Injector for HeaderInjector<'a> {
        fn set(&mut self, key: &str, value: String) {
            self.0.insert(key.to_string(), value);
        }
    }
}

/// Sampling strategies.
pub enum SamplingStrategy {
    /// Always sample (100%).
    Always,

    /// Never sample (0%).
    Never,

    /// Sample based on trace ID ratio.
    Probabilistic(f64),

    /// Rate-limited sampling (samples per second).
    RateLimited(u32),
}

impl SamplingStrategy {
    /// Convert to OpenTelemetry sampler.
    pub fn to_sampler(&self) -> Sampler {
        match self {
            SamplingStrategy::Always => Sampler::AlwaysOn,
            SamplingStrategy::Never => Sampler::AlwaysOff,
            SamplingStrategy::Probabilistic(rate) => {
                Sampler::TraceIdRatioBased(rate.clamp(0.0, 1.0))
            }
            SamplingStrategy::RateLimited(_rate) => {
                // OpenTelemetry SDK doesn't have built-in rate limiting
                // Use parent-based sampler as fallback
                Sampler::ParentBased(Box::new(Sampler::AlwaysOn))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sampler_creation() {
        let sampler = create_sampler(1.0);
        assert!(matches!(sampler, Sampler::AlwaysOn));

        let sampler = create_sampler(0.0);
        assert!(matches!(sampler, Sampler::AlwaysOff));

        let sampler = create_sampler(0.5);
        assert!(matches!(sampler, Sampler::TraceIdRatioBased(_)));
    }

    #[test]
    fn test_span_builder() {
        // Initialize a test subscriber so spans have metadata
        let _guard = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .finish(),
        );

        let span = SpanBuilder::new("test_span")
            .with_attribute("key1", "value1")
            .with_attribute("key2", "value2")
            .start();

        // Span metadata name is static (compile-time), so we check for the fixed name
        assert!(span.metadata().is_some());
        assert_eq!(
            span.metadata().expect("span should have metadata").name(),
            "custom_span"
        );
    }

    #[test]
    fn test_sampling_strategy() {
        let strategy = SamplingStrategy::Always;
        assert!(matches!(strategy.to_sampler(), Sampler::AlwaysOn));

        let strategy = SamplingStrategy::Never;
        assert!(matches!(strategy.to_sampler(), Sampler::AlwaysOff));

        let strategy = SamplingStrategy::Probabilistic(0.5);
        assert!(matches!(
            strategy.to_sampler(),
            Sampler::TraceIdRatioBased(_)
        ));
    }
}
