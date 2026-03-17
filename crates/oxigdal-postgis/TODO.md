# TODO: oxigdal-postgis

## High Priority
- [ ] Implement actual PostgreSQL connection via tokio-postgres (currently structure only)
- [ ] Add connection pool health monitoring with automatic reconnection
- [ ] Implement spatial query execution with PostGIS function mapping
- [ ] Add WKB geometry encoding/decoding for all OGC Simple Features types
- [ ] Implement batch INSERT with COPY protocol for high-throughput writes
- [ ] Add parameterized query builder to prevent SQL injection

## Medium Priority
- [ ] Implement raster support (PostGIS Raster, ST_AsTIFF, ST_FromGDALRaster)
- [ ] Add spatial index management (CREATE/DROP INDEX, ANALYZE, CLUSTER)
- [ ] Implement streaming cursor for large result sets (portal-based pagination)
- [ ] Add PostGIS topology extension support
- [ ] Implement schema migration helpers (CREATE TABLE with geometry columns)
- [ ] Add connection string parsing (postgresql:// URI format)
- [ ] Implement prepared statement caching for repeated queries
- [ ] Add SSL/TLS connection support with certificate validation
- [ ] Implement LISTEN/NOTIFY for real-time change notifications

## Low Priority / Future
- [ ] Add PostGIS 3D geometry support (ST_3DDistance, etc.)
- [ ] Implement foreign data wrapper integration (postgres_fdw for remote tables)
- [ ] Add pg_tileserv/pg_featureserv compatible query generation
- [ ] Implement database migration version tracking
- [ ] Add connection pool metrics export (active/idle/waiting counts)
- [ ] Implement automatic geometry simplification for display queries
