# TODO: oxigdal-terrain

## High Priority
- [ ] Implement D8/D-infinity flow direction with flat-area resolution
- [ ] Add priority-flood sink filling algorithm (Wang & Liu 2006)
- [ ] Implement Strahler stream ordering for extracted networks
- [ ] Add catchment/sub-watershed delineation from multiple pour points
- [ ] Implement profile and plan curvature (Horn's method extension)
- [ ] Add parallel tile-based processing for large DEMs

## Medium Priority
- [ ] Implement topographic wetness index (TWI)
- [ ] Add solar radiation modeling (hillshade with sun position over time)
- [ ] Implement terrain ruggedness index (Riley et al.)
- [ ] Add multi-scale TPI (Topographic Position Index) for landform classification
- [ ] Implement Fresnel zone analysis for viewshed
- [ ] Add cumulative viewshed (observer frequency surface)
- [ ] Implement valley depth and ridge height extraction
- [ ] Add cost-distance/cost-path analysis on terrain surfaces
- [ ] Implement channel network extraction with threshold calibration

## Low Priority / Future
- [ ] Add geomorphon classification (Jasiewicz & Stepinski)
- [ ] Implement terrain texture metrics (entropy, homogeneity)
- [ ] Add 3D terrain mesh generation (TIN from DEM)
- [ ] Implement glacial landform detection (cirques, moraines)
- [ ] Add real-time terrain profile extraction along arbitrary polylines
- [ ] Implement flood simulation (simple 2D shallow water)
- [ ] Add integration with oxigdal-copc for LiDAR-derived DEMs
