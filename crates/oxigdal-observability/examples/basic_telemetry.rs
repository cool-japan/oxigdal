//! Basic telemetry example.

use oxigdal_observability::telemetry::{TelemetryConfig, init_with_config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure telemetry
    let config = TelemetryConfig::new("oxigdal-example")
        .with_service_version("1.0.0")
        .with_service_namespace("examples")
        .with_sampling_rate(1.0) // 100% sampling for demo
        .with_resource_attribute("environment", "development");

    // Initialize telemetry
    let provider = init_with_config(config).await?;

    println!("Telemetry initialized: {}", provider.is_initialized());

    // Your application logic here...

    // Shutdown telemetry
    provider.shutdown().await?;

    Ok(())
}
