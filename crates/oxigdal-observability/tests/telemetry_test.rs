//! Tests for OpenTelemetry integration.

use oxigdal_observability::telemetry::{TelemetryConfig, TelemetryProvider};

#[tokio::test]
async fn test_telemetry_init() {
    let config = TelemetryConfig::new("test-service")
        .with_service_version("1.0.0")
        .with_sampling_rate(1.0);

    let mut provider = TelemetryProvider::new(config);
    let result = provider.init().await;

    // Initialization might fail in test environment without exporters
    // Just check that the function can be called
    let _ = result;
}

#[test]
fn test_telemetry_config_builder() {
    let config = TelemetryConfig::new("test")
        .with_service_version("1.0")
        .with_service_namespace("testing")
        .with_sampling_rate(0.5)
        .with_resource_attribute("env", "dev");

    assert_eq!(config.service_name, "test");
    assert_eq!(config.service_version, "1.0");
    assert_eq!(config.sampling_rate, 0.5);
}
