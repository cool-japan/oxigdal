# TODO: oxigeo-kinesis

> **Purpose:** AWS Kinesis Data Streams + Firehose + Analytics + CloudWatch integration for OxiGeo.
> **Status (2026-07-28):** 5,501 Rust LoC (tokei, `src/`) · 101 tests with `--all-features`, 2 with default features (`streams`/`firehose`/`analytics`/`monitoring` are default-on but the AWS-SDK-touching tests are behind additional non-default features) · Firehose `LambdaTransformer` and the enhanced fan-out `SubscribeToShard` consumer are now real (see below); `KinesisClient::with_streams` remains a stub
> **Roadmap:** v0.1.7 → v0.2.0 → v0.2.1 (current) → v1.0.0

## High Priority (verified gaps)
- [ ] Wire `KinesisClient::with_streams` to a real `aws-sdk-kinesis::Client` instead of returning `None`
  - **Verified gap:** `src/lib.rs:197-201` — literal:
    `pub fn with_streams(mut self, _stream_name: impl Into<String>) -> Self { // This would be initialized with actual AWS client in real usage  self.streams = None; // Placeholder  self }`
  - **Goal:** `with_streams` returns a `KinesisClient` whose `streams()` accessor yields `Some(&KinesisStreams)` ready to call `PutRecord` / `PutRecords` / `GetRecords` per the Kinesis Data Streams API (https://docs.aws.amazon.com/kinesis/latest/APIReference/Welcome.html).
  - **Design:** Lazy AWS-config load via `aws_config::load_defaults(BehaviorVersion::latest())` (matches `streams::KinesisStreams::from_env` at `src/streams/mod.rs:47-51`). Accept an optional region override and credential provider. Mirror `from_env` semantics but synchronous-builder + `.connect()` returns `Result<Self>`. Apply the same fix to firehose/analytics/monitoring `with_*` builders.
  - **Files:** `src/lib.rs` (rewrite `with_streams` line 197 and any sibling `with_*` if present).
  - **Tests:** (proposed) `test_with_streams_returns_some_after_connect`, `test_with_streams_propagates_region_override`, `test_kinesis_client_builder_chain_produces_usable_client`.
  - **Risk:** AWS-config load is async — `with_streams` becomes `async`, which is a breaking signature change; gate behind `async` feature or expose `with_streams_blocking` + `with_streams_async`.
  - **Prerequisites:** None.

- [x] Replace `LambdaTransformer::transform` echo-stub with real Lambda invocation
  - **Verified gap:** `src/firehose/transform.rs:56-60` — literal:
    `async fn transform(&self, data: &[u8]) -> Result<TransformResult> { // In a real implementation, this would invoke Lambda  // For now, this is a placeholder  Ok(TransformResult::Ok(Bytes::copy_from_slice(data))) }`
  - **Goal:** Invoke the configured Lambda ARN per the Firehose data-transformation contract (https://docs.aws.amazon.com/firehose/latest/dev/data-transformation.html): payload is a JSON envelope `{recordId, approximateArrivalTimestamp, data}` (base64), response is `{records: [{recordId, result, data}]}` where `result ∈ Ok|Dropped|ProcessingFailed`.
  - **Design:** Add `aws-sdk-lambda` (workspace-pinned, optional, feature `firehose-transform`). `LambdaTransformer { client: aws_sdk_lambda::Client, lambda_arn }`. `transform()` builds the JSON envelope (`base64::encode` via the existing `base64` workspace dep, or `oxiarc-base64` if available), calls `client.invoke().function_name(&self.lambda_arn).payload(blob).send().await`, decodes the response, maps `result` → `TransformResult::{Ok, Dropped, Failed}`. Retry on `TooManyRequestsException` with jittered exponential backoff (already in `src/streams/checkpoint.rs` style).
  - **Files:** `src/firehose/transform.rs:40-61`; `Cargo.toml` (add `aws-sdk-lambda` optional dep behind new `firehose-transform` feature gating).
  - **Tests:** (proposed) `test_lambda_transform_ok_response_passes_data_through`, `test_lambda_transform_dropped_record`, `test_lambda_transform_processing_failed_returned_as_failed`, `test_lambda_transform_throttled_retries_with_backoff`.
  - **Risk:** Lambda response size limit 6 MB invocation payload — chunk if exceeded, surface as error.
  - **Prerequisites:** None.
  - **Done:** verified fixed as of 2026-07-28. `src/firehose/transform.rs::LambdaTransformer` now holds a real `aws_sdk_lambda::Client`, and `Transformer::transform` calls `.invoke().function_name(&self.lambda_arn).invocation_type(RequestResponse).payload(...).send().await`, mapping a function-level error to a failed record rather than passing the input through unchanged. Matches this item's goal (real invocation replacing the "echo the input back" stub); retry/backoff on throttling not independently confirmed.

- [x] HTTP/2 streaming consumer for enhanced fan-out via `SubscribeToShard`
  - **Verified gap:** `Cargo.toml:30` — feature `enhanced-fanout = ["streams"]` exists but no `SubscribeToShard` call in `src/streams/consumer.rs` (370 LoC; uses polling `GetRecords` only per a grep of the file).
  - **Goal:** Replace the 5 RPS / 2 MB/s shard-share limit of `GetRecords` with the 20 MB/s per-consumer HTTP/2 push stream provided by `SubscribeToShard` (Kinesis Data Streams API v2017-11-23).
  - **Design:** `EnhancedFanOutConsumer` struct gated under `enhanced-fanout`. Pre-flight: `RegisterStreamConsumer` (idempotent on ARN). Open stream: `subscribe_to_shard(consumer_arn, shard_id, starting_position)` returns an `EventStream<SubscribeToShardEvent>`. Per event, dispatch records and update checkpoint. Auto-renew every 5 minutes (AWS hard limit). Backoff and re-subscribe on `ExpiredIteratorException`.
  - **Files:** `src/streams/consumer.rs` (new `EnhancedFanOutConsumer`), re-export from `src/streams/mod.rs:12` (already conditionally re-exports `EnhancedFanOutConsumer` — type currently missing).
  - **Tests:** (proposed) `test_register_consumer_idempotent`, `test_subscribe_to_shard_decodes_record_event`, `test_resubscribe_on_5min_renewal`, `test_resubscribe_on_expired_iterator`.
  - **Risk:** AWS event-stream framing handled by SDK; verify error mapping for `KMSAccessDenied`, `KMSDisabled`, `KMSInvalidStateException` shard-shutdown reasons.
  - **Prerequisites:** Item 1 (real client wiring) so `streams()` returns `Some(_)`.
  - **Done:** verified fixed as of 2026-07-28. `src/streams/consumer.rs::EnhancedFanOutConsumer` (gated `#[cfg(feature = "enhanced-fanout")]`) now exists with a real `register_consumer` (calls `describe_stream_consumer` then falls back to `register_stream_consumer`, i.e. idempotent registration) and is exported from `src/streams/mod.rs`. Built independently of Item 1 — it constructs its own `KinesisClient` rather than depending on `with_streams()`.

## Medium Priority
- [ ] DynamoDB-based checkpoint store (lease ownership + worker heartbeat)
  - **Goal:** Cross-process consumer coordination compatible with KCL 2.x lease schema (`leaseKey, leaseOwner, leaseCounter, checkpoint, ownerSwitchesSinceCheckpoint`).
  - **Files:** `src/streams/checkpoint.rs` (already 22.8 KB; current stores are in-memory).
  - **Why deferred:** Requires `aws-sdk-dynamodb` (already optional behind `checkpointing` feature) — needs lease-renewal scheduler.

- [ ] KPL-compatible record aggregation (protobuf-encoded sub-records inside one Kinesis record)
  - **Goal:** Pack up to 1 MB / 500 sub-records per Kinesis envelope per the KPL Aggregated Record Format spec (kinesis-kpl-aggregated-data-format.md).
  - **Files:** new `src/streams/aggregator.rs`.
  - **Why deferred:** Cost optimisation; only matters at high-throughput producer workloads.

- [ ] KPL deaggregation in consumer path (parse aggregated magic bytes `0xF3 0x89 0x9A 0xC2`)
  - **Goal:** Transparently split aggregated records on the consumer side.
  - **Files:** `src/streams/consumer.rs`.
  - **Why deferred:** Pair with aggregator (above).

- [ ] CloudWatch metrics auto-publishing (PutMetricData) for monitor module
  - **Goal:** Wire `src/monitoring/cloudwatch.rs` (9.2K, has `cloudwatch.rs.builders` / `.final` artefacts suggesting in-progress work) to push `Records.IncomingBytes`, `Records.OutgoingBytes`, `IteratorAgeMilliseconds`.
  - **Files:** `src/monitoring/cloudwatch.rs`.
  - **Why deferred:** Build artefacts (`.builders`, `.final`, `.final2`) need cleanup first.

- [ ] Geospatial partition-key selection (hash by geohash / S2 cell ID for shard locality)
  - **Goal:** Reduce cross-region shuffle by colocating spatially adjacent records.
  - **Files:** new `src/streams/partition_key.rs`.
  - **Why deferred:** Awaiting concrete user.

- [ ] Cleanup stale build artefacts in `src/streams/` and `src/monitoring/`
  - **Goal:** Remove `*.final`, `*.final2`, `*.final3`, `*.noclone2`, `*.builders` sibling files left in the tree (visible from `ls`: `mod.rs.final`, `mod.rs.final2`, `mod.rs.final3`, `producer.rs.noclone2`, `cloudwatch.rs.builders`, `cloudwatch.rs.final`, `cloudwatch.rs.final2`).
  - **Files:** filesystem hygiene.
  - **Why deferred:** Cosmetic; no code impact but pollutes diffs.

## Low Priority / Future (one-liners)
- [ ] Pre-signed `PutRecord` URLs for direct browser-to-Kinesis ingestion
- [ ] Cross-region active-active stream replication
- [ ] Kinesis Video Streams integration for geospatial video feeds
- [ ] Adaptive batching (size & latency feedback from `ProvisionedThroughputExceededException`)
- [ ] Local in-memory Kinesis mock for unit-test environments
- [ ] Firehose dynamic partitioning (route records to S3 prefix by JMESPath / Lambda)
- [ ] Cost estimator for shard-hours + PUT payload units

## Cross-crate dependencies
- **Blocks:** oxigeo-streaming (durable AWS sink/source)
- **Blocked by:** oxigeo-cloud / aws-config (already integrated via `aws-config` workspace dep)

## Recently completed (verbatim)
- *(none in this slice)*

---
*Last audited: 2026-07-28*
