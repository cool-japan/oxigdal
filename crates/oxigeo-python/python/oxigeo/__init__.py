"""OxiGeo - Pure Rust geospatial data abstraction library.

Python bindings for OxiGeo, providing high-performance geospatial operations
with NumPy integration.

Examples:
    Read a raster file:
        >>> import oxigeo
        >>> ds = oxigeo.open("input.tif")
        >>> data = ds.read_band(1)
        >>> print(data.shape)

    Calculate NDVI:
        >>> ndvi = oxigeo.calc("(NIR - RED) / (NIR + RED)", NIR=band4, RED=band3)

    Reproject a raster:
        >>> oxigeo.warp("input.tif", "output.tif", dst_crs="EPSG:3857")

    Read GeoJSON:
        >>> features = oxigeo.read_geojson("input.geojson")
"""

from oxigeo._oxigeo import (
    # Version info
    __version__,
    __author__,
    version,
    # Core functions
    open,
    # Raster functions
    open_raster,
    create_raster,
    calc,
    warp,
    # Vector functions
    read_geojson,
    write_geojson,
    buffer_geometry,
    # Classes
    Dataset,
    RasterMetadata,
    # Exceptions
    OxiGeoError,
)

__all__ = [
    # Module metadata
    "__version__",
    "__author__",
    "version",
    # Core
    "open",
    # Raster
    "open_raster",
    "create_raster",
    "calc",
    "warp",
    # Vector
    "read_geojson",
    "write_geojson",
    "buffer_geometry",
    # Classes
    "Dataset",
    "RasterMetadata",
    # Exceptions
    "OxiGeoError",
]
