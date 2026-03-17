# TODO: oxigdal-kafka

## High Priority
- [ ] Implement Kafka wire protocol (ApiVersions, Produce, Fetch) for Pure Rust client
- [ ] Add TCP connection management with broker discovery and metadata refresh
- [ ] Implement producer record batching with linger.ms and batch.size semantics
- [ ] Add consumer group coordination (JoinGroup, SyncGroup, Heartbeat, LeaveGroup)
- [ ] Implement offset commit and fetch for consumer position tracking
- [ ] Add SASL/PLAIN and SASL/SCRAM authentication
- [ ] Implement partition assignment strategies (Range, RoundRobin, Sticky)

## Medium Priority
- [ ] Add producer acknowledgment modes (acks=0, 1, all) with retry on failure
- [ ] Implement consumer rebalance listener for graceful partition reassignment
- [ ] Add Snappy and LZ4 compression for producer batches
- [ ] Implement schema registry client with Avro schema evolution (compatibility checks)
- [ ] Add transactional producer (init, begin, commit, abort) for exactly-once
- [ ] Implement idempotent producer (producer ID + sequence numbers)
- [ ] Add consumer lag monitoring (compare committed offset vs log-end offset)
- [ ] Implement geospatial-aware partitioner (partition by geohash or tile coordinates)

## Low Priority / Future
- [ ] Add Kafka Connect compatible source/sink connector interface
- [ ] Implement admin client (create/delete topics, describe cluster, alter configs)
- [ ] Add Kafka Streams-like DSL for stream processing on geospatial data
- [ ] Implement exactly-once across Kafka and external sinks (two-phase commit)
- [ ] Add dead letter topic handling for poison pill messages
- [ ] Implement header-based message routing for multi-tenant setups
- [ ] Add TLS/SSL encryption for broker connections
