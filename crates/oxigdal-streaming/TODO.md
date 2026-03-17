# TODO: oxigdal-streaming

## High Priority
- [ ] Implement exactly-once semantics with transactional checkpointing
- [ ] Add watermark propagation across multi-stream joins
- [ ] Implement event-time session windows with gap detection
- [ ] Connect cloud module to real object store I/O (currently pure data model)
- [ ] Add Arrow IPC integration tests with actual RecordBatch round-trips
- [ ] Implement RocksDB state backend (currently placeholder, needs real persistence)
- [ ] Add backpressure propagation across pipeline stages (source -> transform -> sink)

## Medium Priority
- [ ] Implement sliding window aggregation with incremental eviction
- [ ] Add late-event handling with configurable allowed-lateness and side output
- [ ] Implement stream-to-stream temporal join (interval join)
- [ ] Add CDC (Change Data Capture) source connector for PostGIS
- [ ] Implement mmap-based zero-copy raster tile streaming for large GeoTIFFs
- [ ] Add adaptive I/O coalescing threshold tuning based on latency percentiles
- [ ] Implement stream savepoints (snapshot + resume from arbitrary point)
- [ ] Add Prometheus/OpenTelemetry metrics exporter for throughput and latency
- [ ] Implement dead-letter queue for failed stream records

## Low Priority / Future
- [ ] Add stream replay from checkpoint for debugging and reprocessing
- [ ] Implement dynamic stream repartitioning without pipeline restart
- [ ] Add schema evolution support (Avro/Arrow schema migration in-flight)
- [ ] Implement stream-table join with external lookup cache
- [ ] Add WASM-based UDF support for custom stream transformations
- [ ] Implement tiered storage spill (memory -> disk -> cloud) for large windows
- [ ] Add stream lineage tracking (which source records contributed to each output)
