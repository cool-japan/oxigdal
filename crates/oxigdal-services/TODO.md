# TODO: oxigdal-services

## High Priority
- [ ] Implement WMS GetMap actual raster rendering (currently returns XML metadata only)
- [ ] Add WFS GetFeature GML 3.2 output format with proper namespace handling
- [ ] Implement WCS GetCoverage with real raster subsetting and format conversion
- [ ] Wire OGC Features API to real data sources (currently in-memory only)
- [ ] Add OGC API - Tiles Part 2 (vector tiles) with dynamic MVT generation from features
- [ ] Implement WPS Execute with async job tracking (WPS 2.0 dismiss/status polling)
- [ ] Add CSW GetRecords with Dublin Core and ISO 19115 metadata output

## Medium Priority
- [ ] Implement Mapbox GL Style Spec renderer (currently parses but does not render)
- [ ] Add WMS GetFeatureInfo with configurable info formats (HTML, JSON, GML)
- [ ] Implement WFS Transaction operations (Insert, Update, Delete)
- [ ] Add OGC API - Processes Part 1 (replaces WPS with RESTful interface)
- [ ] Implement tile cache invalidation on source data update
- [ ] Add HTTP/2 Server Push for predicted tile requests
- [ ] Implement ETag generation from actual tile content hash (currently FNV-1a placeholder)
- [ ] Add CORS and security headers middleware for OGC service endpoints
- [ ] Implement OGC API - Styles for serving and managing map styles

## Low Priority / Future
- [ ] Add OGC API - Records (replacement for CSW)
- [ ] Implement OGC API - Maps for dynamic map image generation
- [ ] Add 3D Tiles (OGC Community Standard) support
- [ ] Implement SensorThings API for IoT geospatial data
- [ ] Add OGC API - EDR (Environmental Data Retrieval) for point/trajectory queries
- [ ] Implement WMS time dimension support for temporal raster data
- [ ] Add OGC API - Coverages for modern raster data access
