# TODO: oxigdal-python

## High Priority
- [ ] Implement NumPy array zero-copy integration using PyO3 buffer protocol
- [ ] Add actual raster file I/O connecting to oxigdal-geotiff reader/writer
- [ ] Implement CRS/projection support in Python API (from_epsg, transform)
- [ ] Add type stub (.pyi) auto-generation for IDE completion support
- [ ] Implement windowed reading API for processing rasters larger than memory

## Medium Priority
- [ ] Add rasterio-compatible API surface for drop-in replacement usage
- [ ] Implement GeoDataFrame interop via geopandas __geo_interface__
- [ ] Add xarray integration for multi-band raster as labeled arrays
- [ ] Implement matplotlib/folium visualization helpers
- [ ] Add async support via Python asyncio integration
- [ ] Implement point cloud (LAS/LAZ) read/write bindings
- [ ] Add GeoPackage vector read/write bindings
- [ ] Implement raster calculator with NumPy expression evaluation
- [ ] Add manylinux/musllinux wheel builds for PyPI distribution

## Low Priority / Future
- [ ] Implement Jupyter magic commands (%oxigdal_load, %oxigdal_plot)
- [ ] Add Dask array backend for distributed raster processing
- [ ] Implement STAC client bindings with search and download
- [ ] Add scikit-learn integration for ML pipeline compatibility
- [ ] Implement Apache Arrow exchange for zero-copy DataFrame interop
- [ ] Add QGIS processing provider plugin
- [ ] Implement conda-forge package recipe
