# TODO: oxigdal-proj

## High Priority
- [ ] Implement NTv2 grid shift file parser for high-accuracy datum transforms
- [ ] Add WKT2:2019 (ISO 19162) parsing and generation (currently WKT1 only)
- [ ] Implement compound CRS support (horizontal + vertical)
- [ ] Add batch coordinate transformation with SIMD acceleration
- [ ] Implement coordinate epoch support for time-dependent transformations (ITRF)
- [ ] Add Oblique Mercator and Hotine Oblique Mercator projections
- [ ] Implement area-of-use validation to warn when coordinates are outside CRS bounds
- [ ] Add EPSG registry auto-update mechanism from EPSG database exports

## Medium Priority
- [ ] Implement Albers Equal-Area Conic projection
- [ ] Add Polyconic and Equirectangular projections
- [ ] Implement vertical CRS and geoid height transformations
- [ ] Add coordinate operation pipeline (chain multiple transforms)
- [ ] Implement CRS detection from WKT/PROJ string (auto-identify EPSG code)
- [ ] Add geodetic distance and azimuth calculations (Vincenty inverse)
- [ ] Implement dynamic datum transformation using Helmert parameters with rates
- [ ] Add support for engineering/local CRS definitions

## Low Priority / Future
- [ ] Implement full PROJ pipeline string parsing (proj=pipeline step=...)
- [ ] Add thread-safe CRS cache with LRU eviction for transformer reuse
- [ ] Implement geoid model loading (EGM96, EGM2008) for orthometric heights
- [ ] Add South Pole / North Pole specific projection handling (UPS)
- [ ] Implement GeocentricCRS for 3D ECEF transformations with full accuracy
- [ ] Add PROJ.db SQLite database reader for complete EPSG lookup
- [ ] Implement coordinate operation selection by accuracy (pick best transform)
- [ ] Add benchmarks against proj4rs and PROJ C library for accuracy regression testing
