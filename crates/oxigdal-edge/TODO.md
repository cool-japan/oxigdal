# TODO: oxigdal-edge

## High Priority
- [ ] Implement actual HTTP transport layer for edge-to-cloud sync protocol
- [ ] Add persistent cache storage backend (SQLite or file-based)
- [ ] Implement real conflict resolution with CRDT merge for concurrent edits
- [ ] Add resource monitoring integration with actual OS metrics (CPU, memory, disk)
- [ ] Implement edge node discovery and mesh networking between nearby nodes

## Medium Priority
- [ ] Add delta compression for sync payloads to minimize bandwidth
- [ ] Implement priority-based sync queue (critical data syncs first)
- [ ] Add edge-side ML inference scheduling with model version management
- [ ] Implement data retention policy enforcement with configurable TTL
- [ ] Add bandwidth-aware sync scheduling (defer large uploads to WiFi)
- [ ] Implement write-ahead log for crash-safe local operations
- [ ] Add edge cluster coordination with leader election
- [ ] Implement adaptive compression that selects algorithm based on data type

## Low Priority / Future
- [ ] Add MQTT/AMQP message broker integration for event-driven sync
- [ ] Implement geographic sharding for multi-region edge deployments
- [ ] Add edge analytics with local aggregation before cloud upload
- [ ] Implement secure enclave integration for credential storage at the edge
- [ ] Add container/WASI runtime support for edge function deployment
- [ ] Implement predictive prefetch based on access patterns and time-of-day
- [ ] Add edge-to-edge direct data relay for disconnected cloud scenarios
