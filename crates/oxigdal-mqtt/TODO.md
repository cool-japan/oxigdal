# TODO: oxigdal-mqtt

## High Priority
- [ ] Implement MQTT 5.0 CONNECT/CONNACK with properties (session expiry, receive maximum)
- [ ] Add real TCP/TLS connection handling (currently wraps rumqttc without full control)
- [ ] Implement QoS 2 exactly-once delivery (PUBREC, PUBREL, PUBCOMP handshake)
- [ ] Add automatic reconnection with exponential backoff and session resumption
- [ ] Implement shared subscriptions ($share/group/topic) for load-balanced consumers
- [ ] Wire IoT publisher/subscriber to actual MQTT broker connections
- [ ] Add retained message support for last-known-good sensor values

## Medium Priority
- [ ] Implement MQTT 5.0 user properties for metadata propagation
- [ ] Add topic alias support (MQTT 5.0) for reduced bandwidth on repeated topics
- [ ] Implement will message (Last Will and Testament) for device disconnect detection
- [ ] Add message persistence to disk for QoS 1/2 offline delivery
- [ ] Implement bridge mode (forward messages between MQTT brokers)
- [ ] Add geospatial topic hierarchy convention (geo/{z}/{x}/{y}/sensor_type)
- [ ] Implement flow control with MQTT 5.0 receive maximum
- [ ] Add payload compression (gzip/zstd) for large sensor payloads
- [ ] Implement message deduplication based on message ID and timestamp

## Low Priority / Future
- [ ] Add MQTT-SN (Sensor Networks) support for constrained devices
- [ ] Implement embedded MQTT broker for edge computing scenarios
- [ ] Add Sparkplug B specification support for industrial IoT
- [ ] Implement WebSocket transport for browser-based MQTT clients
- [ ] Add device registry with auto-provisioning on first connect
- [ ] Implement topic-based access control lists (ACL) for multi-tenant deployments
- [ ] Add integration with OxiGDAL streaming for MQTT-to-stream bridge
