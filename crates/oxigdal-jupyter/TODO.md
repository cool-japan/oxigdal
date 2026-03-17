# TODO: oxigdal-jupyter

## High Priority
- [ ] Implement Jupyter kernel protocol (ZMQ message handling, execution lifecycle)
- [ ] Add interactive map widget using Leaflet.js via comm messages
- [ ] Implement rich HTML/SVG display for raster band visualization
- [ ] Add %load_raster magic command with actual file loading integration
- [ ] Implement cell output caching for expensive geospatial computations

## Medium Priority
- [ ] Add inline histogram and statistics display for raster bands
- [ ] Implement %crs_info magic command for CRS inspection
- [ ] Add interactive polygon drawing widget for ROI selection
- [ ] Implement side-by-side comparison widget for before/after processing
- [ ] Add progress bar widget for long-running operations (indicatif integration)
- [ ] Implement %export magic command for saving results to various formats
- [ ] Add tab-completion for OxiGDAL functions and dataset properties
- [ ] Implement kernel interrupt handling for cancelling long operations

## Low Priority / Future
- [ ] Add JupyterLab extension for dedicated geospatial sidebar panel
- [ ] Implement Voila dashboard support for sharing interactive maps
- [ ] Add nbconvert exporter for geospatial report generation
- [ ] Implement collaborative editing support via Jupyter Real-Time Collaboration
- [ ] Add GPU memory monitoring widget for oxigdal-gpu operations
- [ ] Implement automatic code generation from widget interactions
- [ ] Add integration with Google Colab and AWS SageMaker environments
