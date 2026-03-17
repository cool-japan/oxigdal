# TODO: oxigdal-stac

## High Priority
- [ ] Wire StacClient to perform real HTTP requests against STAC API endpoints
- [ ] Implement STAC API item download (fetch asset bytes via href)
- [ ] Add CQL2-JSON filter support (currently only CQL2-text is parsed)
- [ ] Implement STAC Collection temporal/spatial summarization from items
- [ ] Add STAC 1.1.0 specification support (new fields, updated link relations)
- [ ] Implement async pagination iterator that streams across pages lazily
- [ ] Add conformance class detection from landing page and auto-adapt client behavior

## Medium Priority
- [ ] Implement STAC Transaction Extension (POST/PUT/PATCH/DELETE items)
- [ ] Add bulk item ingest with batch validation and error reporting
- [ ] Implement STAC Filter Extension (full CQL2 with spatial/temporal operators)
- [ ] Add cross-collection search with result merging and deduplication
- [ ] Implement STAC Aggregation Extension (date histogram, geohash grid)
- [ ] Add pointcloud, raster, and label STAC extensions
- [ ] Implement collection-level asset management (collection assets)
- [ ] Add authentication support for private STAC APIs (Bearer token, API key)
- [ ] Implement STAC Sorting Extension with multi-field sort

## Low Priority / Future
- [ ] Add STAC to/from GeoParquet conversion
- [ ] Implement local file-based STAC catalog (static catalog generation)
- [ ] Add STAC catalog crawler for harvesting remote catalogs
- [ ] Implement STAC Item change detection (diff between versions)
- [ ] Add STAC API query cost estimation based on spatial/temporal extent
- [ ] Implement STAC item thumbnail generation from COG assets
- [ ] Add STAC catalog validation against JSON Schema
