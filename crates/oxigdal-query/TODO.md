# TODO: oxigdal-query

## High Priority
- [ ] Implement spatial SQL functions (ST_Contains, ST_Intersects, ST_Within, ST_Distance)
- [ ] Add JOIN support (INNER, LEFT, spatial joins)
- [ ] Implement GROUP BY with spatial aggregation (ST_Union, ST_Collect)
- [ ] Add actual data source connectors (GeoTIFF, GeoJSON, GeoParquet, PostGIS)
- [ ] Implement predicate pushdown for spatial indexes (R-tree from oxigdal-index)
- [ ] Add ORDER BY with spatial ordering (ST_Distance-based)

## Medium Priority
- [ ] Implement CQL2 (OGC Common Query Language) parser alongside SQL
- [ ] Add window functions (ROW_NUMBER, LAG/LEAD for temporal queries)
- [ ] Implement INSERT/UPDATE/DELETE for mutable data sources
- [ ] Add query plan visualization (DOT graph export)
- [ ] Implement cost model calibration from actual execution statistics
- [ ] Add prepared statement support with parameter binding
- [ ] Implement sub-query and CTE (WITH clause) support
- [ ] Add LIMIT/OFFSET with streaming cursor for large result sets
- [ ] Implement EXPLAIN ANALYZE with actual timing statistics

## Low Priority / Future
- [ ] Add distributed query execution across multiple nodes
- [ ] Implement query federation (query across PostGIS + GeoParquet + STAC)
- [ ] Add materialized view support for cached query results
- [ ] Implement adaptive query optimization (learn from execution history)
- [ ] Add GeoJSON/GeoParquet output format for query results
- [ ] Implement user-defined functions (UDF) registration
