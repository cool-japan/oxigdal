# TODO: oxigdal-db-connectors

## High Priority
- [ ] Implement MySQL spatial query execution (ST_Within, ST_Intersects, ST_Buffer)
- [ ] Add SQLite/SpatiaLite connection with spatial index creation and querying
- [ ] Implement MongoDB geospatial queries ($geoWithin, $geoIntersects, $near)
- [ ] Wire DatabaseConnector trait to actual database client libraries
- [ ] Add connection pooling with configurable min/max connections and idle timeout
- [ ] Implement WKB/WKT geometry serialization for all connectors
- [ ] Add ClickHouse geo functions (pointInPolygon, geoDistance) support

## Medium Priority
- [ ] Implement TimescaleDB hypertable creation and time-series spatial queries
- [ ] Add Cassandra/ScyllaDB spatial partitioning with geohash-based partition keys
- [ ] Implement bulk insert with batched prepared statements for all connectors
- [ ] Add schema migration support (create spatial tables, add spatial indexes)
- [ ] Implement query builder with spatial filter DSL (bbox, radius, polygon)
- [ ] Add connection health monitoring with automatic reconnection
- [ ] Implement read replica routing (write to primary, read from replica)
- [ ] Add GeoJSON to/from database geometry type conversion for all backends
- [ ] Implement cursor-based pagination for large spatial query results

## Low Priority / Future
- [ ] Add DuckDB connector for embedded analytical spatial queries
- [ ] Implement database-to-database spatial data transfer (ETL bridge)
- [ ] Add CockroachDB spatial support (distributed PostGIS-compatible)
- [ ] Implement query plan analysis for spatial query optimization hints
- [ ] Add database backup/restore utilities with spatial data integrity checks
- [ ] Implement multi-database transaction coordinator (distributed commit)
- [ ] Add database connection metrics export (query latency, pool usage, errors)
