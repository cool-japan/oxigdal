# TODO: oxigdal-proj

## Completed
- [x] Expand built-in EPSG registry to 500+ codes (added extended.rs with 300+ definitions covering NAD83 State Planes, European, Asian-Pacific, South American, African, Polar grids, geographic CRS, and vertical CRS)

## High Priority
- [ ] Implement NTv2 grid shift file parser for high-accuracy datum transforms
- [x] Add WKT2:2019 (ISO 19162) parsing and generation (WKT2 keywords GEOGCRS/PROJCRS/GEODCRS/VERTCRS/ENGCRS/COMPOUNDCRS/BOUNDCRS, axis extraction, ellipsoid alias support, 26 tests)
- [ ] Implement compound CRS support (horizontal + vertical)
- [ ] Add batch coordinate transformation with SIMD acceleration
- [ ] Implement coordinate epoch support for time-dependent transformations (ITRF)
- [x] Add Oblique Mercator and Hotine Oblique Mercator projections (Hotine variant A, spherical, 3D rotation-matrix approach, 8 tests)
- [ ] Implement area-of-use validation to warn when coordinates are outside CRS bounds
- [ ] Add EPSG registry auto-update mechanism from EPSG database exports

## Medium Priority
- [x] Implement Albers Equal-Area Conic projection (forward + inverse, Snyder p.98-103, 7 tests)
- [x] Add Polyconic and Equirectangular projections (American Polyconic with Newton-Raphson inverse + Plate Carrée, 12 tests)
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
