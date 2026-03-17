# TODO: oxigdal-gateway

## High Priority
- [ ] Implement actual HTTP request proxying in handle_connection (currently no-op)
- [ ] Add Axum/Hyper-based router for dispatching to backend services
- [ ] Implement JWT validation with JWKS endpoint discovery and key rotation
- [ ] Wire rate limiter to actual request middleware (currently standalone data structures)
- [ ] Add route configuration with path-based and header-based routing rules
- [ ] Implement health check polling for backend services
- [ ] Add request/response logging middleware with structured JSON output

## Medium Priority
- [ ] Implement OAuth2 authorization code flow with PKCE
- [ ] Add API key rotation and revocation support
- [ ] Implement GraphQL query depth limiting and cost analysis
- [ ] Add WebSocket upgrade handling and message forwarding to backends
- [ ] Implement response caching with configurable TTL per route
- [ ] Add request transformation (header injection, path rewriting, body mapping)
- [ ] Implement circuit breaker integration with load balancer failover
- [ ] Add API versioning via URL prefix, header, or query parameter
- [ ] Implement IP allowlist/blocklist filtering

## Low Priority / Future
- [ ] Add OpenAPI/Swagger documentation auto-generation from route config
- [ ] Implement gRPC-Web proxying for gRPC backend services
- [ ] Add mTLS (mutual TLS) support for backend service communication
- [ ] Implement request deduplication for identical concurrent requests
- [ ] Add plugin system for custom middleware (WASM-based)
- [ ] Implement traffic shadowing (mirror requests to secondary backend)
- [ ] Add A/B testing support with weighted traffic splitting
