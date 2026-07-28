# oxigeo-db-connectors

Database connectors for OxiGeo with support for multiple database systems.

## Features

The crate supports multiple database backends, each feature-gated for flexibility:

- **MySQL/MariaDB** (`mysql` feature) - Spatial extensions with WKT/WKB support (⚠️ pulls `libz-sys`, a C dependency)
- **SQLite** (`sqlite` feature) - Embedded spatial database via the pure-Rust `oxisql-sqlite-compat` (limbo engine) — no SpatiaLite C extension support
- **MongoDB** (`mongodb` feature) - Native GeoJSON support with geospatial queries (⚠️ pulls `ring`, hand-written assembly)
- **ClickHouse** (`clickhouse` feature) - Massive-scale spatial analytics
- **TimescaleDB** (`postgres` feature) - Time-series geospatial data
- **Cassandra/ScyllaDB** (`cassandra` feature) - Distributed spatial data storage (⚠️ pulls `aws-lc-sys` via Scylla's rustls, C/asm)

### Default Features

By default, the following features are enabled — all on a 100% Pure Rust dependency closure:
- `postgres` (TimescaleDB)
- `sqlite` (pure-Rust `oxisql-sqlite-compat` limbo engine)
- `clickhouse`

**Note:** `mysql`, `mongodb`, and `cassandra` are **NOT** included in default features because each pulls a non-Pure-Rust C/asm dependency (`libz-sys`, `ring`, and `aws-lc-sys` respectively), in compliance with the COOLJAPAN Pure Rust Policy. SQLite used to carry this warning in older releases (`libsqlite3-sys`/rusqlite); the crate has since migrated to the pure-Rust `oxisql-sqlite-compat` engine, so SQLite is Pure Rust and on by default.

## Database Support

### MySQL/MariaDB

```rust
use oxigeo_db_connectors::mysql::{MySqlConfig, MySqlConnector};
use geo_types::point;

let config = MySqlConfig::default();
let connector = MySqlConnector::new(config)?;

// Create spatial table
connector.create_spatial_table(
    "locations",
    "geometry",
    "POINT",
    4326,
    &[("name".to_string(), "VARCHAR(255)".to_string())]
).await?;
```

### SQLite

The `sqlite` feature is on by default (see [Default Features](#default-features)); disable default features and re-enable it explicitly only if you're trimming down to a custom backend set.

```toml
[dependencies]
oxigeo-db-connectors = { version = "0.2", features = ["sqlite"] }
```

```rust
use oxigeo_db_connectors::sqlite::{SqliteConfig, SqliteConnector};

let connector = SqliteConnector::memory()?;

// Create spatial table
connector.create_spatial_table(
    "places",
    "geometry",
    "POINT",
    4326,
    &[]
)?;
```

**Note on SpatiaLite:** this backend is pure-Rust `oxisql-sqlite-compat` (the limbo engine), which cannot load the SpatiaLite C shared-library extension. `SqliteConnector::has_spatialite()` always returns `false`, and `SqliteConfig::spatialite` is a no-op kept only for source compatibility. Spatial tables/queries use a pure-Rust WKT/WKB fallback instead of SpatiaLite's native geometry functions.

### MongoDB

```rust
use oxigeo_db_connectors::mongodb::{MongoDbConfig, MongoDbConnector};

let config = MongoDbConfig::default();
let connector = MongoDbConnector::new(config).await?;

// Create 2dsphere index for geospatial queries
connector.create_geo_index("locations", "geometry").await?;
```

### ClickHouse

```rust
use oxigeo_db_connectors::clickhouse::{ClickHouseConfig, ClickHouseConnector};

let config = ClickHouseConfig::default();
let connector = ClickHouseConnector::new(config)?;

// Create table with spatial columns
connector.create_spatial_table(
    "events",
    &[],
    "MergeTree() ORDER BY id"
).await?;
```

### TimescaleDB

```rust
use oxigeo_db_connectors::timescale::{TimescaleConfig, TimescaleConnector};

let config = TimescaleConfig::default();
let connector = TimescaleConnector::new(config)?;

// Create hypertable for time-series data
connector.create_hypertable("sensor_data", "time", Some("1 hour")).await?;
```

### Cassandra/ScyllaDB

```rust
use oxigeo_db_connectors::cassandra::{CassandraConfig, CassandraConnector};

let config = CassandraConfig::default();
let connector = CassandraConnector::new(config).await?;

// Create spatial table
connector.create_spatial_table(
    "locations",
    "id",
    Some("timestamp"),
    &[]
).await?;
```

## Connection Management

### Connection String Parsing

```rust
use oxigeo_db_connectors::connection::ConnectionString;

let conn_str = "mysql://user:pass@localhost:3306/gis";
let parsed = ConnectionString::parse(conn_str)?;

println!("Database: {}", parsed.database_type());
println!("Host: {:?}", parsed.host());
println!("Port: {:?}", parsed.port());
```

### Connection Pooling

```rust
use oxigeo_db_connectors::connection::pool::PoolConfig;
use std::time::Duration;

let config = PoolConfig::new()
    .with_min_connections(5)
    .with_max_connections(20)
    .with_connection_timeout(Duration::from_secs(30));
```

### Health Checking

```rust
use oxigeo_db_connectors::connection::health::{HealthCheckConfig, HealthTracker};

let config = HealthCheckConfig::new()
    .with_interval(Duration::from_secs(30))
    .with_failure_threshold(3);

let mut tracker = HealthTracker::new(config);
```

## Performance

- Batch insertions for high throughput
- Connection pooling for concurrent access
- Prepared statements where supported
- Streaming for large result sets

## COOLJAPAN Compliance

- ✅ Pure Rust by default (C/asm dependencies are feature-gated, non-default)
  - Default features (`postgres`, `sqlite`, `clickhouse`) use 100% Pure Rust
  - SQLite uses the pure-Rust `oxisql-sqlite-compat` engine — no C dependency
  - `mysql` (libz-sys), `mongodb` (ring), `cassandra` (aws-lc-sys via Scylla) each pull a C/asm dependency and are opt-in only
- ✅ No unwrap() calls
- ✅ Files < 2000 lines
- ✅ Workspace dependencies

### Feature Configuration Examples

**Pure Rust only (default set):**
```toml
[dependencies]
oxigeo-db-connectors = { version = "0.2", default-features = false, features = ["postgres", "sqlite", "clickhouse"] }
```

**With MySQL (includes a C dependency, libz-sys):**
```toml
[dependencies]
oxigeo-db-connectors = { version = "0.2", features = ["mysql"] }
```

## License

Apache-2.0

## Copyright

COOLJAPAN OU (Team Kitasan)
