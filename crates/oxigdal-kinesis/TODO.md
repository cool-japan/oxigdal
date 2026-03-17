# TODO: oxigdal-kinesis

## High Priority
- [ ] Implement KPL-compatible record aggregation (sub-record batching within PutRecords)
- [ ] Add enhanced fan-out consumer with SubscribeToShard HTTP/2 streaming
- [ ] Implement DynamoDB-based checkpointing for consumer shard positions
- [ ] Wire Producer/Consumer to actual aws-sdk-kinesis client calls
- [ ] Add shard iterator management (TRIM_HORIZON, LATEST, AT_TIMESTAMP, AT_SEQUENCE_NUMBER)
- [ ] Implement automatic shard split/merge detection and consumer rebalancing
- [ ] Add Firehose delivery stream creation and management via aws-sdk-firehose

## Medium Priority
- [ ] Implement adaptive batching (adjust batch size based on throughput and latency)
- [ ] Add per-shard rate limiting to avoid ProvisionedThroughputExceeded errors
- [ ] Implement Kinesis Analytics SQL query submission and result consumption
- [ ] Add CloudWatch metrics publishing for custom stream monitoring
- [ ] Implement record deaggregation for consuming KPL-produced records
- [ ] Add Firehose data transformation Lambda integration
- [ ] Implement multi-stream fan-in (consume from multiple streams, merge events)
- [ ] Add geospatial partition key selection (partition by region/tile for data locality)

## Low Priority / Future
- [ ] Implement Kinesis Video Streams integration for geospatial video feeds
- [ ] Add cross-region stream replication for disaster recovery
- [ ] Implement stream resharding advisor (recommend split/merge based on load)
- [ ] Add Firehose dynamic partitioning (route records to different S3 prefixes)
- [ ] Implement Kinesis Data Analytics Flink application management
- [ ] Add cost estimation for stream provisioning (shard-hours, PUT payload units)
- [ ] Implement local development mode with in-memory Kinesis mock
