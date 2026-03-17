# TODO: oxigdal-observability

## High Priority
- [ ] Implement actual OpenTelemetry SDK integration (traces, metrics, logs)
- [ ] Add Prometheus /metrics HTTP endpoint with geospatial-specific gauges
- [ ] Implement distributed trace context propagation (W3C Trace Context)
- [ ] Add structured logging with correlation IDs across crate boundaries
- [ ] Implement health check endpoints with dependency status (PostGIS, S3, etc.)
- [ ] Remove `#![allow(clippy::unwrap_used)]` and eliminate all unwraps

## Medium Priority
- [ ] Add Grafana dashboard JSON templates for raster/vector processing metrics
- [ ] Implement SLO burn rate alerting (multi-window error budget)
- [ ] Add anomaly detection on metric time series (z-score, EWMA)
- [ ] Implement alert routing (PagerDuty, Slack, email) via webhook
- [ ] Add StatsD exporter for legacy monitoring systems
- [ ] Implement profiling integration (flamegraph generation)
- [ ] Add metric cardinality management (label explosion prevention)
- [ ] Implement log sampling for high-throughput operations
- [ ] Add InfluxDB line protocol exporter

## Low Priority / Future
- [ ] Add CloudWatch Embedded Metrics Format exporter
- [ ] Implement distributed tracing visualization (Jaeger-compatible export)
- [ ] Add custom Grafana plugin for geospatial metric visualization
- [ ] Implement cost attribution per operation (cloud resource tracking)
- [ ] Add OpenTelemetry Collector configuration generation
- [ ] Implement continuous profiling (pprof-compatible)
