# TODO: oxigdal-etl

## High Priority
- [ ] Implement actual file I/O in FileSource and FileSink (currently placeholder)
- [ ] Add S3 source/sink with streaming read/write for large datasets
- [ ] Implement Kafka source with consumer group offset tracking
- [ ] Wire PostGIS sink to actual PostgreSQL connection with spatial INSERT
- [ ] Add pipeline checkpoint persistence to disk for crash recovery
- [ ] Implement backpressure propagation from sink to source (slow consumer handling)
- [ ] Add STAC source with bbox/datetime filtering for catalog-driven ETL

## Medium Priority
- [ ] Implement windowed aggregation operator (tumbling, sliding, session windows)
- [ ] Add stream-to-stream join operator with configurable join condition
- [ ] Implement data quality validation transform (schema check, null check, range check)
- [ ] Add HTTP source with polling interval and webhook receiver mode
- [ ] Implement cron-based scheduler with persistent job state
- [ ] Add pipeline DAG visualization (DOT/Mermaid export)
- [ ] Implement parallel execution mode with configurable worker count per stage
- [ ] Add dead letter queue for records that fail transformation
- [ ] Implement incremental/CDC mode (process only new/changed records)

## Low Priority / Future
- [ ] Add GeoParquet source/sink for columnar geospatial ETL
- [ ] Implement schema inference from first N records in source
- [ ] Add pipeline versioning and migration (upgrade running pipelines)
- [ ] Implement data lineage tracking (source record to output record mapping)
- [ ] Add pipeline template library (common geospatial ETL patterns)
- [ ] Implement cost estimation for pipeline runs (I/O, compute, storage)
- [ ] Add REST API for pipeline management (create, start, stop, status)
