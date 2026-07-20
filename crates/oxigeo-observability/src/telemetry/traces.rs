//! Distributed tracing with OpenTelemetry.

use crate::error::{ObservabilityError, Result};
use crate::telemetry::TelemetryConfig;
use opentelemetry::global;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{RandomIdGenerator, Sampler, SdkTracerProvider};

/// Initialize distributed tracing based on configuration.
pub async fn init_tracing(config: &TelemetryConfig, resource: Resource) -> Result<()> {
    let sampler = create_sampler(config.sampling_rate);

    // Create tracer provider based on configured exporters
    let tracer_provider = if let Some(ref endpoint) = config.otlp_endpoint {
        #[cfg(feature = "otlp")]
        {
            create_otlp_tracer_provider(endpoint, sampler, resource).await?
        }
        #[cfg(not(feature = "otlp"))]
        {
            return Err(ObservabilityError::ConfigError(
                "OTLP feature not enabled".to_string(),
            ));
        }
    } else if let Some(ref jaeger_endpoint) = config.jaeger_endpoint {
        #[cfg(feature = "jaeger")]
        {
            create_jaeger_tracer_provider(jaeger_endpoint, sampler, resource).await?
        }
        #[cfg(not(feature = "jaeger"))]
        {
            let _ = jaeger_endpoint;
            return Err(ObservabilityError::ConfigError(
                "Jaeger feature not enabled".to_string(),
            ));
        }
    } else {
        // Default to stdout exporter for development
        create_stdout_tracer_provider(sampler, resource)?
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
async fn create_otlp_tracer_provider(
    endpoint: &str,
    sampler: Sampler,
    resource: Resource,
) -> Result<SdkTracerProvider> {
    use opentelemetry_otlp::WithExportConfig;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .map_err(|e| ObservabilityError::TraceExportFailed(e.to_string()))?;

    let provider = SdkTracerProvider::builder()
        .with_sampler(sampler)
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    Ok(provider)
}

/// Create a tracer provider for a configured Jaeger endpoint.
///
/// The `opentelemetry-jaeger` crate is unmaintained (RUSTSEC-2025-0123) and has been removed
/// from this crate's dependency closure entirely, so there is no legacy Jaeger agent protocol
/// exporter available. Modern Jaeger (>= 1.35) ingests spans natively via OTLP, so when the
/// `otlp` feature is also enabled this delegates to the OTLP exporter pointed at the
/// configured endpoint -- point it at Jaeger's OTLP receiver (gRPC port 4317 by default)
/// rather than the legacy agent port.
///
/// Without the `otlp` feature there is no transport capable of reaching Jaeger at all, so this
/// fails loudly with a [`ObservabilityError::ConfigError`] instead of silently dumping spans to
/// stdout while reporting success.
#[cfg(feature = "jaeger")]
async fn create_jaeger_tracer_provider(
    endpoint: &str,
    sampler: Sampler,
    resource: Resource,
) -> Result<SdkTracerProvider> {
    #[cfg(feature = "otlp")]
    {
        tracing::warn!(
            "Jaeger agent endpoint '{}' specified, but the opentelemetry-jaeger crate is \
             deprecated/unmaintained (RUSTSEC-2025-0123) and is not a dependency of this build. \
             Routing spans through the OTLP exporter instead -- point '{}' at Jaeger's native \
             OTLP receiver (gRPC port 4317), not the legacy Jaeger agent port.",
            endpoint,
            endpoint
        );
        create_otlp_tracer_provider(endpoint, sampler, resource).await
    }

    #[cfg(not(feature = "otlp"))]
    {
        let _ = (sampler, resource);
        Err(ObservabilityError::ConfigError(format!(
            "Jaeger agent endpoint '{endpoint}' was configured, but the opentelemetry-jaeger \
             crate is deprecated/unmaintained (RUSTSEC-2025-0123) and has been removed from \
             this crate's dependencies -- there is no transport available to reach Jaeger. \
             Enable the 'otlp' feature so spans are routed to Jaeger's native OTLP receiver \
             instead of the legacy agent protocol."
        )))
    }
}

/// Create stdout tracer provider for development.
fn create_stdout_tracer_provider(
    sampler: Sampler,
    resource: Resource,
) -> Result<SdkTracerProvider> {
    let exporter = opentelemetry_stdout::SpanExporter::default();

    let provider = SdkTracerProvider::builder()
        .with_sampler(sampler)
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource)
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
            target: "oxigeo",
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

    #[cfg(all(feature = "jaeger", feature = "otlp"))]
    #[tokio::test]
    async fn test_jaeger_endpoint_routes_through_otlp_when_otlp_enabled() {
        // With both `jaeger` and `otlp` enabled, a configured Jaeger endpoint must be routed
        // through the OTLP exporter (Jaeger's native OTLP receiver) rather than silently
        // falling back to a stdout exporter.
        let resource = Resource::builder_empty().build();
        let provider =
            create_jaeger_tracer_provider("http://localhost:4317", Sampler::AlwaysOn, resource)
                .await;
        assert!(
            provider.is_ok(),
            "jaeger_endpoint should delegate to the OTLP exporter when the otlp feature is \
             enabled, not fail or silently use stdout"
        );
    }

    #[cfg(all(feature = "jaeger", not(feature = "otlp")))]
    #[tokio::test]
    async fn test_jaeger_endpoint_fails_loudly_without_otlp_feature() {
        // Without the otlp feature there is no transport that can reach Jaeger at all, so this
        // must fail loudly instead of silently dumping spans to stdout while reporting success.
        let resource = Resource::builder_empty().build();
        let result =
            create_jaeger_tracer_provider("http://localhost:4317", Sampler::AlwaysOn, resource)
                .await;
        assert!(result.is_err());
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
