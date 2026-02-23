# OxiGDAL Gateway

[![Crates.io](https://img.shields.io/crates/v/oxigdal-gateway.svg)](https://crates.io/crates/oxigdal-gateway)
[![Documentation](https://docs.rs/oxigdal-gateway/badge.svg)](https://docs.rs/oxigdal-gateway)
[![License](https://img.shields.io/crates/l/oxigdal-gateway.svg)](LICENSE)

Enterprise-grade API gateway for geospatial services with comprehensive features including rate limiting, authentication, GraphQL support, and WebSocket handling. Built in pure Rust for high performance and reliability.

## Features

- **Rate Limiting**: Multiple algorithms (token bucket, leaky bucket, fixed/sliding window) with memory and distributed Redis backends
- **Authentication**: API keys, JWT tokens, OAuth2/OIDC integration, session management, and multi-factor authentication (MFA)
- **API Versioning**: Support for multiple API versions with content negotiation, migration, and deprecation handling
- **GraphQL Support**: Full GraphQL server with queries, mutations, subscriptions, and schema management
- **WebSocket**: Real-time bidirectional communication with connection multiplexing and message routing
- **Middleware Stack**: CORS, response compression (gzip/brotli), response caching, structured logging, and metrics collection
- **Load Balancing**: Multiple strategies (round-robin, least connections, weighted) with health checks and circuit breaker patterns
- **Request/Response Transformation**: Format adaptation and data transformation pipelines
- **Authorization**: Role-based access control (RBAC) with fine-grained permission management
- **Pure Rust**: 100% Pure Rust implementation with zero C/Fortran dependencies
- **Error Handling**: Comprehensive error types with proper HTTP status codes and retry semantics

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-gateway = "0.1"
tokio = { version = "1", features = ["full"] }
```

### Feature Flags

```toml
# In-memory rate limiting (default)
oxigdal-gateway = { version = "0.1", features = ["memory"] }

# Distributed rate limiting with Redis
oxigdal-gateway = { version = "0.1", features = ["redis"] }
```

## Quick Start

### Basic Gateway Setup

```rust
use oxigdal_gateway::{Gateway, GatewayConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create default configuration
    let config = GatewayConfig::default();

    // Initialize gateway
    let gateway = Gateway::new(config)?;

    // Start listening on port 8080
    gateway.serve("0.0.0.0:8080").await?;

    Ok(())
}
```

### With Custom Configuration

```rust
use oxigdal_gateway::{Gateway, GatewayConfig};
use oxigdal_gateway::auth::AuthConfig;
use oxigdal_gateway::rate_limit::RateLimitConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = GatewayConfig::default();

    // Configure authentication
    config.auth = AuthConfig {
        enable_api_key: true,
        enable_jwt: true,
        enable_oauth2: false,
        enable_session: true,
        require_mfa: false,
        jwt_secret: Some("your-secret-key".to_string()),
        jwt_expiration: 3600,
        session_timeout: 1800,
        ..Default::default()
    };

    // Configure rate limiting
    config.rate_limit = RateLimitConfig {
        enabled: true,
        ..Default::default()
    };

    // Configure gateway limits
    config.max_body_size = 50 * 1024 * 1024; // 50MB
    config.request_timeout = 60;
    config.enable_graphql = true;
    config.enable_websocket = true;

    let gateway = Gateway::new(config)?;
    gateway.serve("0.0.0.0:8080").await?;

    Ok(())
}
```

## Usage

### Authentication

#### API Key Authentication

```rust
use oxigdal_gateway::auth::api_key::ApiKeyAuthenticator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let authenticator = ApiKeyAuthenticator::new();

    // Generate a new API key for a user
    let api_key = authenticator.generate_key(
        "user123".to_string(),
        "production-key".to_string(),
        vec!["read".to_string(), "write".to_string()],
    )?;

    println!("API Key: {}", api_key);

    // Authenticate with the key
    let context = authenticator.authenticate(&api_key).await?;
    println!("Authenticated as: {}", context.identity.user_id);

    Ok(())
}
```

#### JWT Authentication

```rust
use oxigdal_gateway::auth::{jwt::JwtAuthenticator, Identity};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let secret = b"your-secret-key-at-least-32-chars";
    let expiration = 3600; // 1 hour

    let authenticator = JwtAuthenticator::new(secret, expiration);

    // Create identity
    let mut identity = Identity::new("user456".to_string());
    identity.roles.insert("admin".to_string());

    // Generate JWT token
    let token = authenticator.create_token(&identity)?;
    println!("JWT Token: {}", token);

    // Verify token
    let context = authenticator.authenticate(&token).await?;
    assert_eq!(context.identity.user_id, "user456");

    Ok(())
}
```

#### OAuth2 Integration

```rust
use oxigdal_gateway::auth::{AuthConfig, oauth2::OAuth2Provider};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AuthConfig {
        enable_oauth2: true,
        oauth2_client_id: Some("your-client-id".to_string()),
        oauth2_client_secret: Some("your-client-secret".to_string()),
        oauth2_auth_url: Some("https://provider.com/oauth/authorize".to_string()),
        oauth2_token_url: Some("https://provider.com/oauth/token".to_string()),
        ..Default::default()
    };

    // OAuth2 provider will handle redirect flow
    let provider = OAuth2Provider::new(config)?;

    Ok(())
}
```

### Rate Limiting

#### Token Bucket Algorithm

```rust
use oxigdal_gateway::rate_limit::{RateLimiter, Algorithm, TokenBucket, RateLimitKey};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let algorithm = TokenBucket::new(
        100,  // capacity: 100 requests
        10,   // refill_rate: 10 requests per second
    );

    let limiter = RateLimiter::new(algorithm);
    let key = RateLimitKey::new("user123").with_resource("/api/users");

    // Check if request is allowed
    let decision = limiter.check(&key).await?;

    match decision {
        oxigdal_gateway::rate_limit::Decision::Allowed => {
            println!("Request allowed");
        }
        oxigdal_gateway::rate_limit::Decision::Limited { retry_after, limit, current } => {
            println!("Rate limited. Retry after {:?}", retry_after);
            println!("Limit: {}, Current: {}", limit, current);
        }
    }

    Ok(())
}
```

#### Distributed Rate Limiting with Redis

```rust
use oxigdal_gateway::rate_limit::{
    RateLimiter, Algorithm, TokenBucket, RedisStorage, RateLimitKey
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Requires "redis" feature
    let redis_storage = RedisStorage::connect("redis://localhost:6379").await?;
    let algorithm = TokenBucket::new(1000, 100);

    let limiter = RateLimiter::with_storage(algorithm, redis_storage);
    let key = RateLimitKey::new("org:acme").with_namespace("api-calls");

    let decision = limiter.check(&key).await?;

    Ok(())
}
```

### GraphQL

```rust
use oxigdal_gateway::graphql::{GraphQLServer, Schema};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // GraphQL server with subscriptions support
    let server = GraphQLServer::new()?;

    // Server handles queries, mutations, and subscriptions
    // Configure schema and resolvers for your geospatial data

    Ok(())
}
```

### WebSocket Support

```rust
use oxigdal_gateway::websocket::{WebSocketRouter, MessageHandler};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let router = WebSocketRouter::new();

    // Register handlers for different message types
    // Supports connection multiplexing and routing

    Ok(())
}
```

### Middleware

```rust
use oxigdal_gateway::middleware::{MiddlewareConfig, CompressionLevel};

let mut middleware_config = MiddlewareConfig::default();

// Enable CORS
middleware_config.enable_cors = true;
middleware_config.allowed_origins = vec!["https://example.com".to_string()];

// Configure compression
middleware_config.enable_compression = true;
middleware_config.compression_level = CompressionLevel::Best;

// Enable caching
middleware_config.enable_caching = true;
middleware_config.cache_ttl = 300; // 5 minutes

// Enable metrics
middleware_config.enable_metrics = true;
middleware_config.enable_logging = true;
```

### Load Balancing

```rust
use oxigdal_gateway::loadbalancer::{LoadBalancer, Strategy, HealthChecker};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backends = vec![
        "http://backend1:8080".to_string(),
        "http://backend2:8080".to_string(),
        "http://backend3:8080".to_string(),
    ];

    let strategy = Strategy::RoundRobin;
    let health_checker = HealthChecker::new(backends, strategy);

    // Load balancer automatically routes requests to healthy backends
    // Performs periodic health checks and circuit breaking

    Ok(())
}
```

### API Versioning

```rust
use oxigdal_gateway::versioning::{VersionNegotiator, VersionInfo};

let negotiator = VersionNegotiator::new(vec![
    VersionInfo::new("1.0".to_string()),
    VersionInfo::new("2.0".to_string()),
    VersionInfo::new("3.0".to_string()),
]);

// Automatically handles version negotiation from headers or URL path
// Supports deprecation warnings and migration paths
```

## API Overview

### Core Modules

| Module | Description |
|--------|-------------|
| `auth` | Authentication and authorization (API keys, JWT, OAuth2, sessions, MFA, RBAC) |
| `rate_limit` | Rate limiting with multiple algorithms and storage backends |
| `graphql` | GraphQL server with queries, mutations, subscriptions, and schema management |
| `websocket` | WebSocket support with multiplexing and message routing |
| `middleware` | HTTP middleware stack (CORS, compression, caching, logging, metrics) |
| `loadbalancer` | Load balancing with health checks and circuit breaker |
| `transform` | Request/response transformation and format adaptation |
| `versioning` | API versioning, negotiation, migration, and deprecation |
| `error` | Comprehensive error types and handling |

### Authentication Submodules

| Module | Description |
|--------|-------------|
| `auth::api_key` | API key generation and validation |
| `auth::jwt` | JWT token creation and verification |
| `auth::oauth2` | OAuth2/OIDC provider integration |
| `auth::session` | Session management and tracking |
| `auth::mfa` | Multi-factor authentication support |
| `auth::permissions` | Fine-grained permission management |
| `auth::rbac` | Role-based access control |

### Rate Limiting Submodules

| Module | Description |
|--------|-------------|
| `rate_limit::algorithms` | Token bucket, leaky bucket, fixed/sliding window |
| `rate_limit::rules` | Rule engine for complex rate limiting policies |
| `rate_limit::storage` | In-memory and Redis storage backends |

## Configuration

### GatewayConfig

```rust
pub struct GatewayConfig {
    pub rate_limit: RateLimitConfig,
    pub auth: AuthConfig,
    pub loadbalancer: LoadBalancerConfig,
    pub middleware: MiddlewareConfig,
    pub max_body_size: usize,           // Default: 10MB
    pub request_timeout: u64,           // Default: 30 seconds
    pub enable_graphql: bool,           // Default: true
    pub enable_websocket: bool,         // Default: true
}
```

## Error Handling

This library follows the "no unwrap" policy. All fallible operations return `Result<T, E>` with descriptive error types:

```rust
use oxigdal_gateway::{GatewayError, Result};

// GatewayError variants include:
// - RateLimitExceeded { message, retry_after }
// - AuthenticationFailed(String)
// - AuthorizationFailed(String)
// - InvalidApiKey
// - TokenExpired
// - InvalidToken(String)
// - GraphQLError(String)
// - WebSocketError(String)
// - UnsupportedVersion { version, supported }
// - BackendUnavailable(String)
// - CircuitBreakerOpen(String)
// - Timeout(String)
// ... and more

// Errors provide HTTP status codes and retry information
let error = GatewayError::RateLimitExceeded {
    message: "Quota exceeded".to_string(),
    retry_after: Some(60),
};

let status = error.status_code(); // HTTP 429
let retryable = error.is_retryable(); // true
let retry_after = error.retry_after(); // Some(60)
```

## Performance

The gateway is designed for high-performance, enterprise-scale deployments:

- **Asynchronous**: Built on Tokio for non-blocking I/O
- **Efficient Rate Limiting**: In-memory rate limiting with O(1) operations
- **Distributed Support**: Optional Redis backend for distributed deployments
- **Connection Pooling**: Manages backend connections efficiently
- **Compression**: Automatic response compression (gzip/brotli)
- **Caching**: Built-in response caching to reduce backend load

### Benchmarks

Run performance benchmarks with:

```bash
cargo bench --bench gateway_bench
```

Benchmark measurements on typical hardware:

| Operation | Time |
|-----------|------|
| Rate limit check | <1µs |
| API key validation | <10µs |
| JWT verification | <100µs |
| Request routing | <50µs |

## Examples

See the [tests](tests/) directory for integration examples:

- `auth_test.rs` - Authentication and authorization examples
- `graphql_test.rs` - GraphQL server integration
- `middleware_test.rs` - Middleware stack usage
- `websocket_test.rs` - WebSocket real-time communication

## Documentation

Full documentation is available at [docs.rs](https://docs.rs/oxigdal-gateway).

View locally with:

```bash
cargo doc --open
```

## Testing

Run the test suite:

```bash
# All tests
cargo test --all-features

# With specific features
cargo test --features redis

# Integration tests
cargo test --test '*' -- --ignored
```

## Pure Rust

This library is 100% Pure Rust with no C/Fortran dependencies. All functionality works out of the box without external libraries or system dependencies.

## OxiGDAL Ecosystem

This project is part of the OxiGDAL ecosystem for geospatial data processing:

- **OxiGDAL-Core**: Core geospatial data structures and operations
- **OxiGDAL-Algorithms**: Geospatial algorithms and transformations
- **OxiGDAL-Drivers**: File format readers/writers (GeoTIFF, GeoJSON, Shapefile, etc.)
- **OxiGDAL-Server**: HTTP server for geospatial services
- **OxiGDAL-Cloud**: Cloud deployment support

## Contributing

Contributions are welcome! This project follows:

- **No Unwrap Policy**: All fallible operations must use `Result` types
- **No Warnings**: Code must compile without warnings
- **Pure Rust**: No C/Fortran dependencies in default features
- **File Size**: Source files should be kept under 2000 lines

See contributing guidelines for more information.

## License

Licensed under the Apache License, Version 2.0.

## Related Projects

- [OxiGDAL-Core](https://github.com/cool-japan/oxigdal-core) - Core geospatial library
- [OxiGDAL-Server](https://github.com/cool-japan/oxigdal-server) - HTTP server
- [OxiGDAL-CLI](https://github.com/cool-japan/oxigdal-cli) - Command-line tools

---

Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem of pure Rust geospatial libraries and tools.
