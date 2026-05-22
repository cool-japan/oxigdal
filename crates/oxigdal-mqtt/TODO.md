# TODO: oxigdal-mqtt

> **Purpose:** MQTT 3.1.1 / 5.0 client for OxiGDAL — IoT sensor ingestion, pub/sub, geospatial time-series.
> **Status (2026-05-16):** 5,620 LoC · 132 tests · 1 real-code stub
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Complete QoS 2 (exactly-once) handshake — currently PUBREC is logged but PUBREL is never issued
  - **Verified gap:** `src/client/mod.rs:346-352` — literal:
    `Packet::PubRec(pubrec) => { debug!("Received PUBREC for packet: {}", pubrec.pkid); } Packet::PubComp(pubcomp) => { debug!("Received PUBCOMP for packet: {}", pubcomp.pkid); inflight.remove(&pubcomp.pkid); }`
  - **Goal:** A QoS 2 publisher correctly completes the four-step handshake `PUBLISH → PUBREC → PUBREL → PUBCOMP` per MQTT 5.0 §4.3.3 (OASIS Standard, March 2019). Inflight tracking must not free the packet ID until `PUBCOMP` lands. A subscriber receiving QoS 2 must persist seen-packet-IDs across reconnects.
  - **Design:** Upgrade `inflight: DashMap<u16, ...>` to track per-packet phase `enum Qos2Phase { AwaitingPubRec, AwaitingPubComp }`. On `Packet::PubRec`, transition to `AwaitingPubComp` and send `Packet::PubRel { pkid, reason_code: 0x00 }` via `rumqttc::AsyncClient::ack`. Per MQTT 5.0 §4.4 (message-delivery retry), retry `PUBREL` (not `PUBLISH`) until `PUBCOMP` arrives. On reconnect with `Clean Start = 0` and Session Expiry > 0, retransmit any `AwaitingPubComp` packets with DUP flag.
  - **Files:** `src/client/mod.rs:346-352` (issue PUBREL on PubRec); `src/client/connection.rs` (extend `ClientInner` inflight map); `src/types.rs:39,48,383` (QoS::ExactlyOnce mapping already present).
  - **Tests:** (proposed) `test_qos2_publisher_sends_pubrel_after_pubrec`, `test_qos2_publisher_retries_pubrel_until_pubcomp`, `test_qos2_subscriber_dedupes_on_dup_pubrel`, `test_qos2_session_resumption_retransmits_awaiting_pubcomp_only`.
  - **Risk:** `rumqttc` 0.25 may already handle some QoS 2 internals — confirm by inspecting which `Packet` variants it auto-acks; the literal debug-only handlers above suggest it does not.
  - **Prerequisites:** None.

- [ ] Add MQTT 5.0 `CONNECT` Properties (Session Expiry Interval, Receive Maximum, Maximum Packet Size, Topic Alias Maximum)
  - **Verified gap:** `Cargo.toml:17,21-22` declares feature `mqtt5 = []` (default-on) but `src/types.rs` (13.1 KB) defines `ConnectionOptions` with no Properties fields — quote from inspection: `ConnectionOptions::new("mqtt://localhost", 1883, "publisher-1")` (constructor takes only host/port/client_id; no MQTT-5 properties surface).
  - **Goal:** First-class MQTT 5.0 property support on `CONNECT`/`CONNACK` per OASIS MQTT 5.0 §3.1.2.11 (CONNECT Properties) and §3.2.2.3 (CONNACK Properties). Producer & subscriber config must accept `session_expiry_interval`, `receive_maximum`, `maximum_packet_size`, `topic_alias_maximum`, `user_properties`.
  - **Design:** Add `Mqtt5Properties` struct in `src/types.rs`; thread through `ConnectionOptions::with_properties(props)`. Map onto `rumqttc::v5::mqttbytes::v5::ConnectProperties`. On CONNACK, capture server-assigned `Maximum QoS`, `Retain Available`, `Wildcard Subscription Available`, `Subscription Identifiers Available`, `Shared Subscription Available` and expose via `client.server_capabilities()`. Reject `mqtt3` callers from setting MQTT-5 props with a clear error.
  - **Files:** `src/types.rs` (add `Mqtt5Properties`, extend `ConnectionOptions`); `src/client/connection.rs` (forward to rumqttc v5 API).
  - **Tests:** (proposed) `test_mqtt5_connect_session_expiry_property_encoded`, `test_mqtt5_connack_server_keep_alive_overrides_client`, `test_mqtt5_user_properties_round_trip`, `test_mqtt3_caller_with_mqtt5_props_rejected`.
  - **Risk:** Mixing `rumqttc::v4` and `rumqttc::v5` paths cleanly — `rumqttc` exposes them under separate modules; pick at compile time via the `mqtt3` / `mqtt5` features.
  - **Prerequisites:** None.

## Medium Priority
- [ ] Shared subscriptions (`$share/{group}/{topic}`) for load-balanced consumers
  - **Goal:** MQTT 5.0 §4.8.2 shared subscription routing — broker fan-out to one group member per delivery.
  - **Files:** `src/subscriber/router.rs:269-273` (currently treats `$share/...` as plain topic).
  - **Why deferred:** Broker-side concern; client mostly needs validation + topic-filter recognition.

- [ ] Topic alias support (MQTT 5.0 §3.3.2.3.4) to compress repeated topic strings
  - **Goal:** Negotiate `Topic Alias Maximum` and reuse 2-byte aliases for high-frequency publishes.
  - **Files:** `src/publisher/mod.rs`.
  - **Why deferred:** Bandwidth optimisation; default 0 (disabled) per MQTT 5.0.

- [ ] Last Will and Testament (Will Message + Will Properties)
  - **Goal:** Configure LWT for disconnect detection — required for IoT device fleets.
  - **Files:** `src/types.rs::ConnectionOptions`, `src/client/connection.rs`.
  - **Why deferred:** Surface-area addition; needs property encoding from MQTT-5 work above.

- [ ] On-disk persistence for QoS 1/2 (sled backend already optional)
  - **Goal:** Replay `AwaitingPubAck` / `AwaitingPubComp` messages across crashes.
  - **Files:** `src/publisher/persistence.rs` (10.1 KB scaffolded).
  - **Why deferred:** Sled is a `RUSTSEC-2025-0057` / `RUSTSEC-2024-0384` transitive risk noted in `Cargo.toml:54-59`; consider redb migration before promoting.

- [ ] Geospatial topic-hierarchy helper (`geo/{z}/{x}/{y}/{sensor_type}`)
  - **Goal:** First-class builder + filter for the tile-based topic convention used by OxiGDAL.
  - **Files:** `src/iot/geospatial.rs` (already 11.3 KB).
  - **Why deferred:** Convention not yet finalised across the stack.

- [ ] Exponential reconnection backoff with jitter — verify `ReconnectStrategy` actually backs off
  - **Goal:** Avoid thundering-herd reconnects across a fleet.
  - **Files:** `src/client/reconnect.rs:9.4K`.
  - **Why deferred:** Audit needed — current implementation may already be correct.

## Low Priority / Future (one-liners)
- [ ] MQTT-SN (Sensor Networks) v1.2 transport for UDP / 802.15.4 constrained devices
- [ ] Sparkplug B namespace adapter for industrial IoT
- [ ] WebSocket transport (`ws://` / `wss://`) for browser MQTT clients
- [ ] Bridge mode (forward between two brokers with rewrite rules)
- [ ] Embedded MQTT broker for edge gateways
- [ ] Topic-based ACL evaluator for multi-tenant ingestion
- [ ] Native bridge into oxigdal-streaming (`MqttSource` + watermark generation)

## Cross-crate dependencies
- **Blocks:** oxigdal-sensors (ingestion path), oxigdal-streaming (stream source)
- **Blocked by:** None

## Recently completed (verbatim)
- *(none in this slice)*

---
*Last audited: 2026-05-16*
