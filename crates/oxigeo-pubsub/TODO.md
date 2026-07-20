# TODO: oxigeo-pubsub

> **Purpose:** Google Cloud Pub/Sub integration for OxiGeo — Pure Rust publisher/subscriber with schema validation and monitoring.
> **Status (2026-05-16):** 4,618 LoC · 175 tests · 1 real-code stub
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Replace synthetic batched-publish ID with the broker-assigned `messageId` once flush completes
  - **Verified gap:** `src/publisher.rs:479-481` — literal:
    `// For batched publishing, we return a placeholder ID  // In a real implementation, this would track the actual message ID  Ok(format!("batched-{}", uuid::Uuid::new_v4()))`
  - **Goal:** `Publisher::publish(message)` returns the real Google-Pub/Sub `messageId` (per Pub/Sub REST API `PublishResponse.messageIds`, https://cloud.google.com/pubsub/docs/reference/rest/v1/projects.topics/publish) regardless of whether batching is on. Callers that need at-least-once delivery confirmation must not get a synthetic `batched-<uuid>` value.
  - **Design:** Convert `publish_batched` from fire-and-forget to per-message future. Each `publish_batched` returns `oneshot::Receiver<String>` keyed by an internal sequence number. `flush_batch` issues the batch via `GcpPublisher::publish(messages)` (already wired at `src/publisher.rs:344` per the `google-cloud-pubsub::client::Publisher` import on line 10), zips `PublishResponse.messageIds` with the queued senders, and resolves them in order. The public signature `pub async fn publish(...) -> Result<String>` becomes a wrapper that awaits the oneshot.
  - **Files:** `src/publisher.rs:456-503` (rewrite `publish_batched`, `flush_batch`).
  - **Tests:** (proposed) `test_publish_batched_returns_broker_messageid_after_flush`, `test_publish_batched_propagates_publish_error_to_waiters`, `test_publish_batched_preserves_per_message_id_ordering_within_batch`, `test_flush_drains_pending_oneshots_on_drop`.
  - **Risk:** Pub/Sub guarantees `messageIds` are returned in request order; the GCP SDK preserves that. Be wary of mid-batch failures — partial success returns no `messageIds`, all waiters must error.
  - **Prerequisites:** None.

## Medium Priority
- [ ] OAuth2 service-account credentials with automatic token refresh
  - **Goal:** Cleanly surface refresh-token / workload-identity-federation use cases per `google-cloud-auth`'s `CredentialsFile` discovery order.
  - **Files:** new `src/auth.rs` (feature `auth` already declared at `Cargo.toml:42`).
  - **Why deferred:** `google-cloud-auth` workspace dep already provides default ADC; explicit overrides only needed for workload-identity-federation.

- [ ] Push-subscription endpoint registration + webhook verification (`X-Goog-Channel-Id`, JWT verification)
  - **Goal:** Allow OxiGeo services to receive push deliveries instead of pull-only.
  - **Files:** new `src/subscriber/push.rs` (currently `subscriber.rs` is monolithic 28 KB pull-only).
  - **Why deferred:** Requires HTTP server integration; pair with oxigeo-services webhook path.

- [ ] Exactly-once delivery with `ordering_keys` + dedup window
  - **Goal:** Honour `EnableMessageOrdering` and `EnableExactlyOnceDelivery` subscription flags per the Pub/Sub Exactly-Once Delivery spec.
  - **Files:** `src/subscriber.rs` (extend ack handling to surface `AckError`).
  - **Why deferred:** Requires `modifyAckDeadline` deduplication-state tracking.

- [ ] Dead-letter routing for repeatedly-nacked messages
  - **Goal:** Configure `DeadLetterPolicy` topic + `MaxDeliveryAttempts`.
  - **Files:** `src/subscription.rs` (already exposes `DeadLetterPolicy` struct).
  - **Why deferred:** Surface present; wire-through to subscribe call missing.

- [ ] Avro schema validation gate on `Publisher::publish`
  - **Goal:** Reject payloads that fail validation under the topic-attached Avro schema before hitting the broker.
  - **Files:** `src/publisher.rs` (validate inside `publish_immediate`), `src/schema.rs:469` (currently `Placeholder module when schema feature is disabled`).
  - **Why deferred:** Awaits `schema` feature uplift in `Cargo.toml:27-31`.

- [ ] Protobuf schema (`prost`-based) support symmetrical to Avro
  - **Goal:** Same flow as Avro item but for Pub/Sub schemas with `encoding=PROTOBUF_FORMAT`.
  - **Files:** `src/schema.rs`.
  - **Why deferred:** Pair with Avro work.

- [ ] Flow-control settings (`max_outstanding_bytes` / `max_outstanding_messages`) honoured by subscriber pump
  - **Goal:** Honour `FlowControlSettings` constants already exposed at `src/lib.rs:127-131` (`DEFAULT_MAX_OUTSTANDING_BYTES`, `DEFAULT_MAX_OUTSTANDING_MESSAGES`).
  - **Files:** `src/subscriber.rs`.
  - **Why deferred:** Constants present; gating loop not wired.

- [ ] Cloud Monitoring metric export (publish latency, ack latency, backlog)
  - **Goal:** Push `PublisherMetrics` / `SubscriberMetrics` (`src/lib.rs:142-146`) to Cloud Monitoring.
  - **Files:** `src/monitoring.rs:615` (`Placeholder module when monitoring feature is disabled`).
  - **Why deferred:** Awaiting concrete exporter target.

## Low Priority / Future (one-liners)
- [ ] Snapshot + Seek (replay from timestamp or named snapshot)
- [ ] Subscription filter expressions (`hasPrefix(attributes.region, "us-")`)
- [ ] BigQuery subscription (direct ingestion path) registration helpers
- [ ] Pub/Sub Lite topics/subscriptions for cost-optimised throughput
- [ ] Schema evolution checker (`BACKWARD` / `FORWARD` / `FULL` compatibility)
- [ ] Pub/Sub emulator integration for local dev
- [ ] Cross-project topic/subscription manager (multi-project orchestration)
- [ ] Multi-region message routing with geo-affinity
- [ ] Bridge into oxigeo-streaming (`PubSubSource` with watermark)

## Cross-crate dependencies
- **Blocks:** oxigeo-streaming (durable cloud source/sink)
- **Blocked by:** None (`google-cloud-pubsub 0.33` already integrated at `src/publisher.rs:10`)

## Recently completed (verbatim)
- *(none in this slice)*

---
*Last audited: 2026-05-16*
