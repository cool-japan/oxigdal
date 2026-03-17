# TODO: oxigdal-websocket

## High Priority
- [ ] Implement actual WebSocket server using tokio-tungstenite
- [ ] Add connection authentication (JWT, API key validation on upgrade)
- [ ] Implement binary protocol for efficient tile data transfer
- [ ] Add per-connection backpressure with configurable buffer limits
- [ ] Implement heartbeat/ping-pong with automatic stale connection cleanup
- [ ] Add room-based pub/sub with spatial topic filtering (bbox subscriptions)

## Medium Priority
- [ ] Implement WebSocket compression (permessage-deflate extension)
- [ ] Add rate limiting per connection and per IP
- [ ] Implement message batching for high-frequency updates
- [ ] Add connection migration (seamless reconnect with state restore)
- [ ] Implement tile delta encoding (send only changed pixels)
- [ ] Add feature change stream with GeoJSON diff payloads
- [ ] Implement connection pool with load balancing across workers
- [ ] Add WebSocket metrics (connections, messages/sec, bytes transferred)
- [ ] Implement graceful shutdown with drain timeout

## Low Priority / Future
- [ ] Add Server-Sent Events (SSE) fallback for environments without WebSocket
- [ ] Implement WebTransport (HTTP/3) support when ecosystem matures
- [ ] Add JavaScript/TypeScript client SDK generation with type safety
- [ ] Implement message replay (replay missed messages on reconnect)
- [ ] Add WebSocket proxy support (X-Forwarded-For, proxy protocol)
- [ ] Implement multi-cluster WebSocket federation
