# oxigdal-db-connectors

Database connectors for OxiGDAL with support for multiple database systems.

## Features

The crate supports multiple database backends, each feature-gated for flexibility:

- **MySQL/MariaDB** (`mysql` feature) - Spatial extensions with WKT/WKB support
- **SQLite/SpatiaLite** (`sqlite` feature) - Embedded spatial database (⚠️ C dependency)
- **MongoDB** (`mongodb` feature) - Native GeoJSON support with geospatial queries
- **ClickHouse** (`clickhouse` feature) - Massive-scale spatial analytics
- **TimescaleDB** (`postgres` feature) - Time-series geospatial data
- **Cassandra/ScyllaDB** (`cassandra` feature) - Distributed spatial data storage

### Default Features

By default, the following features are enabled:
- `postgres` (TimescaleDB)
- `mysql`
- `mongodb`
- `clickhouse`
- `cassandra`

**Note:** SQLite is **NOT** included in default features due to its C dependency (libsqlite3-sys), in compliance with the COOLJAPAN Pure Rust Policy.

## Database Support

### MySQL/MariaDB

```rust
use oxigdal_db_connectors::mysql::{MySqlConfig, MySqlConnector};
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

### SQLite/SpatiaLite

⚠️ **Note:** SQLite support requires enabling the `sqlite` feature flag.

```toml
[dependencies]
oxigdal-db-connectors = { version = "0.1", features = ["sqlite"] }
```

```rust
use oxigdal_db_connectors::sqlite::{SqliteConfig, SqliteConnector};

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

**Why is SQLite feature-gated?**

SQLite has a C dependency (`libsqlite3-sys`) which violates the COOLJAPAN Pure Rust Policy. By feature-gating it, users who need 100% Pure Rust can use the crate without any C dependencies by disabling default features and only enabling the Pure Rust database backends they need.

### MongoDB

```rust
use oxigdal_db_connectors::mongodb::{MongoDbConfig, MongoDbConnector};

let config = MongoDbConfig::default();
let connector = MongoDbConnector::new(config).await?;

// Create 2dsphere index for geospatial queries
connector.create_geo_index("locations", "geometry").await?;
```

### ClickHouse

```rust
use oxigdal_db_connectors::clickhouse::{ClickHouseConfig, ClickHouseConnector};

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
use oxigdal_db_connectors::timescale::{TimescaleConfig, TimescaleConnector};

let config = TimescaleConfig::default();
let connector = TimescaleConnector::new(config)?;

// Create hypertable for time-series data
connector.create_hypertable("sensor_data", "time", Some("1 hour")).await?;
```

### Cassandra/ScyllaDB

```rust
use oxigdal_db_connectors::cassandra::{CassandraConfig, CassandraConnector};

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
use oxigdal_db_connectors::connection::ConnectionString;

let conn_str = "mysql://user:pass@localhost:3306/gis";
let parsed = ConnectionString::parse(conn_str)?;

println!("Database: {}", parsed.database_type());
println!("Host: {:?}", parsed.host());
println!("Port: {:?}", parsed.port());
```

### Connection Pooling

```rust
use oxigdal_db_connectors::connection::pool::PoolConfig;
use std::time::Duration;

let config = PoolConfig::new()
    .with_min_connections(5)
    .with_max_connections(20)
    .with_connection_timeout(Duration::from_secs(30));
```

### Health Checking

```rust
use oxigdal_db_connectors::connection::health::{HealthCheckConfig, HealthTracker};

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

- ✅ Pure Rust by default (C dependencies are feature-gated)
  - Default features use 100% Pure Rust
  - SQLite (C dependency) is behind the `sqlite` feature flag
- ✅ No unwrap() calls
- ✅ Files < 2000 lines
- ✅ Workspace dependencies

### Feature Configuration Examples

**Pure Rust only (no C dependencies):**
```toml
[dependencies]
oxigdal-db-connectors = { version = "0.1", default-features = false, features = ["postgres", "mysql"] }
```

**With SQLite (includes C dependency):**
```toml
[dependencies]
oxigdal-db-connectors = { version = "0.1", features = ["sqlite"] }
```

## License

Apache-2.0

## Copyright

COOLJAPAN OU (Team Kitasan)
