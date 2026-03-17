# TODO: oxigdal-ws

## High Priority
- [ ] Implement actual TCP WebSocket server binding (currently structure only)
- [ ] Add TLS support for wss:// connections
- [ ] Implement tile streaming handler with COG byte-range integration
- [ ] Add feature streaming handler with incremental GeoJSON delivery
- [ ] Implement subscription manager with spatial/temporal/attribute filters
- [ ] Add backpressure controller with adaptive flow control

## Medium Priority
- [ ] Implement MessagePack binary protocol for compact message encoding
- [ ] Add delta encoder for tile updates (xor-based diff)
- [ ] Implement event streaming for progress updates and notifications
- [ ] Add client reconnection with message replay from sequence number
- [ ] Implement server-side message filtering to reduce bandwidth
- [ ] Add connection grouping for broadcast efficiency
- [ ] Implement WebSocket subprotocol negotiation
- [ ] Add configurable message compression (zstd, deflate)
- [ ] Implement health check endpoint alongside WebSocket upgrade

## Low Priority / Future
- [ ] Add load testing harness (simulate thousands of concurrent clients)
- [ ] Implement multi-server message bus (Redis/NATS pub/sub backend)
- [ ] Add protocol documentation generation (AsyncAPI spec)
- [ ] Implement client SDK for Python (via PyO3 bindings)
- [ ] Add WebSocket gateway for protocol translation (gRPC to WS)
- [ ] Implement observability integration (trace context in WS frames)
