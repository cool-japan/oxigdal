# TODO: oxigdal-pubsub

## High Priority
- [ ] Wire Publisher to actual Google Cloud Pub/Sub REST/gRPC API calls
- [ ] Wire Subscriber to real pull subscription polling and ack/nack
- [ ] Implement OAuth2 service account authentication with token refresh
- [ ] Add message batching with configurable max_messages, max_bytes, and max_latency
- [ ] Implement exactly-once delivery with ordering keys and deduplication
- [ ] Add push subscription endpoint registration and verification
- [ ] Implement dead letter queue routing for repeatedly failed messages

## Medium Priority
- [ ] Add Avro schema validation on publish (validate before sending)
- [ ] Implement Protobuf schema support with prost-based encoding
- [ ] Add flow control (outstanding bytes/messages limits) for subscriber
- [ ] Implement snapshot and seek (replay from timestamp or snapshot)
- [ ] Add subscription filter expressions for server-side message filtering
- [ ] Implement Cloud Monitoring metrics export (publish latency, ack latency, backlog)
- [ ] Add topic retention policy configuration and management
- [ ] Implement BigQuery subscription (direct Pub/Sub to BigQuery ingestion)
- [ ] Add message transformation functions (Cloud Functions trigger on publish)

## Low Priority / Future
- [ ] Implement Pub/Sub Lite for cost-optimized high-throughput scenarios
- [ ] Add cross-project topic/subscription management
- [ ] Implement schema evolution with backward/forward compatibility checks
- [ ] Add Pub/Sub emulator integration for local development and testing
- [ ] Implement message replay with filtering (replay only matching messages)
- [ ] Add multi-region message routing with geo-affinity
- [ ] Implement Pub/Sub to OxiGDAL streaming bridge for real-time geospatial pipelines
