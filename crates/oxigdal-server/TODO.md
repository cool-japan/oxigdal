# TODO: oxigdal-server

## High Priority
- [ ] Implement actual raster tile rendering from GeoTIFF/COG sources
- [ ] Add WMS GetMap handler that produces PNG/JPEG image responses
- [ ] Implement WMTS RESTful tile endpoint with on-the-fly reprojection
- [ ] Wire dataset_registry to load real raster datasets at startup
- [ ] Add TileJSON endpoint with minzoom/maxzoom/bounds from dataset metadata
- [ ] Implement disk cache tier (currently memory-only LRU cache)
- [ ] Add graceful shutdown with in-flight request draining

## Medium Priority
- [ ] Implement tile pre-seeding CLI command for cache warming
- [ ] Add vector tile (MVT) serving from GeoJSON/GeoPackage sources
- [ ] Implement multi-layer composite tiles (blend multiple rasters)
- [ ] Add style-based rendering (Mapbox GL JSON style application)
- [ ] Implement GetFeatureInfo for identifying pixel values at click location
- [ ] Add CORS configuration and security headers
- [ ] Implement request rate limiting per client IP
- [ ] Add /health and /ready endpoints for Kubernetes probes
- [ ] Implement hot-reload of layer configuration without server restart

## Low Priority / Future
- [ ] Add Prometheus metrics endpoint (/metrics) for monitoring
- [ ] Implement terrain tile serving (Quantized Mesh / Mapbox Terrain RGB)
- [ ] Add TLS/HTTPS support with certificate auto-renewal
- [ ] Implement cluster-aware tile cache with Redis/memcached backend
- [ ] Add tile request logging with analytics dashboard
- [ ] Implement S3/GCS tile cache backend for serverless deployments
- [ ] Add Docker and Kubernetes Helm chart for deployment
