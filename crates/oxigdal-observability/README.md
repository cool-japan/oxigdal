# oxigdal-observability

OpenTelemetry-based observability, monitoring, and alerting for OxiGDAL.

## Features

- **OpenTelemetry Integration**: Full support for distributed tracing, metrics, and structured logging
- **Custom Geospatial Metrics**: Specialized metrics for raster, vector, I/O, cache, query, GPU, and cluster operations
- **Distributed Tracing**: W3C Trace Context propagation across distributed services
- **Real-time Dashboards**: Pre-built Grafana dashboards and Prometheus recording rules
- **Anomaly Detection**: Statistical (Z-score, IQR) and ML-based (Isolation Forest, Autoencoder) anomaly detection
- **SLO/SLA Monitoring**: Service level objectives with error budget tracking and burn rate calculation
- **Alert Management**: Rule-based alerting with routing, deduplication, and escalation policies
- **Metric Exporters**: Support for Prometheus, StatsD, InfluxDB, and AWS CloudWatch

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-observability = "0.1.3"
```

## Quick Start

### Initialize Telemetry

```rust
use oxigdal_observability::telemetry::{TelemetryConfig, init_with_config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = TelemetryConfig::new("oxigdal")
        .with_service_version("1.0.0")
        .with_jaeger_endpoint("localhost:6831")
        .with_prometheus_endpoint("localhost:9090")
        .with_sampling_rate(0.1);
    
    let provider = init_with_config(config).await?;
    
    // Your application logic
    
    provider.shutdown().await?;
    Ok(())
}
```

### Collect Custom Metrics

```rust
use opentelemetry::global;
use oxigdal_observability::metrics::GeoMetrics;

let meter = global::meter("oxigdal");
let metrics = GeoMetrics::new(meter)?;

// Record raster operations
metrics.raster.record_read(125.5, 1048576, "GeoTIFF", true);
metrics.raster.record_write(89.3, 524288, "GeoTIFF", true);

// Record cache operations
metrics.cache.record_hit("tile_cache", 8192);
metrics.cache.record_miss("tile_cache");

// Record query operations
metrics.query.record_query(45.2, "spatial", 150, true);
```

### Set Up SLO Monitoring

```rust
use oxigdal_observability::slo::{
    budgets::BudgetTracker,
    objectives::{AvailabilitySlo, LatencySlo},
    SloMonitor,
};

let mut monitor = SloMonitor::new();
monitor.add_slo(AvailabilitySlo::three_nines());
monitor.add_slo(LatencySlo::p95_100ms());

let tracker = BudgetTracker::new();
for slo in monitor.slos() {
    tracker.track(slo.name.clone(), slo.error_budget.clone());
}
```

### Anomaly Detection

```rust
use oxigdal_observability::anomaly::{
    statistical::ZScoreDetector,
    AnomalyDetector,
    DataPoint,
};
use chrono::Utc;

let mut detector = ZScoreDetector::new(3.0);

// Establish baseline
let baseline = vec![/* your historical data */];
detector.update_baseline(&baseline)?;

// Detect anomalies
let test_data = vec![DataPoint::new(Utc::now(), 50.0)];
let anomalies = detector.detect(&test_data)?;
```

### Alert Management

```rust
use oxigdal_observability::alerting::{
    Alert, AlertManager, AlertSeverity,
    rules::AlertRule,
    routing::{Destination, Route},
};

let mut manager = AlertManager::new();

// Add alert rule
let rule = AlertRule {
    name: "high_error_rate".to_string(),
    condition: Arc::new(|| check_error_rate() > 0.05),
    severity: AlertSeverity::High,
    message: "Error rate exceeded 5%".to_string(),
};
manager.add_rule(rule);

// Add routing
let route = Route {
    matcher: Box::new(|alert| alert.severity == AlertSeverity::High),
    destinations: vec![
        Destination::Slack {
            webhook_url: "https://hooks.slack.com/...".to_string(),
        },
    ],
};
manager.add_route(route);

// Evaluate and route alerts
let alerts = manager.evaluate_rules().await?;
```

## Performance

- **Telemetry Overhead**: < 1% in production configurations
- **Metric Collection**: < 100μs per metric operation
- **Trace Sampling**: Configurable from 0.01% to 100%
- **Anomaly Detection**: Real-time processing with minimal latency

## Architecture

### Telemetry Stack

```
┌─────────────────────────────────────────┐
│         Application Code                │
└─────────────┬───────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────┐
│    oxigdal-observability                │
│  ┌──────────┐  ┌──────────┐            │
│  │ Metrics  │  │  Traces  │            │
│  └────┬─────┘  └────┬─────┘            │
│       │             │                   │
│  ┌────▼─────────────▼─────┐            │
│  │   OpenTelemetry SDK    │            │
│  └────┬───────────────────┘            │
└───────┼────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────┐
│  Exporters (Jaeger, Prometheus, etc) │
└───────────────────────────────────────┘
```

### Metrics Categories

- **Raster Metrics**: Read/write operations, compression, tiles, overviews
- **Vector Metrics**: Feature operations, spatial queries, geometryprocessing
- **I/O Metrics**: File/network I/O, cloud storage, throughput, latency
- **Cache Metrics**: Hit/miss rates, evictions, prefetch efficiency
- **Query Metrics**: Query execution, planning, complexity, results
- **GPU Metrics**: Utilization, memory, kernel execution, transfers
- **Cluster Metrics**: Node health, data distribution, consensus, replication

## Examples

See the `examples/` directory for complete working examples:

- `basic_telemetry.rs`: Initialize telemetry with OpenTelemetry
- `metrics_collection.rs`: Collect custom geospatial metrics
- `slo_monitoring.rs`: Set up SLO monitoring with error budgets
- `anomaly_detection.rs`: Detect anomalies in metric streams
- `alerting.rs`: Configure alerts with routing and escalation

## License

Licensed under the Apache License, Version 2.0.

## Contributing

Contributions are welcome! Please see the main OxiGDAL repository for contribution guidelines.
