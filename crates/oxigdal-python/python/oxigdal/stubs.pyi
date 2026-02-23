"""Type stubs for OxiGDAL Python bindings.

This file provides type hints for IDE support and static type checking.
"""

from typing import Any, Optional, Union
import numpy as np
import numpy.typing as npt

__version__: str
__author__: str

class OxiGdalError(Exception):
    """Base exception for OxiGDAL errors."""
    def __init__(self, message: str) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class RasterMetadata:
    """Metadata for raster datasets.

    Attributes:
        width: Width in pixels
        height: Height in pixels
        band_count: Number of bands
        data_type: Data type as string (e.g., 'float32')
        crs: Coordinate reference system
        nodata: NoData value
    """
    width: int
    height: int
    band_count: int
    data_type: str
    crs: Optional[str]
    nodata: Optional[float]

    def __init__(
        self,
        width: int,
        height: int,
        band_count: int = 1,
        data_type: str = "float32",
        crs: Optional[str] = None,
        nodata: Optional[float] = None,
    ) -> None: ...

    def __repr__(self) -> str: ...

    def to_dict(self) -> dict[str, Any]: ...

class Dataset:
    """A geospatial dataset.

    Represents an opened raster or vector dataset with methods to access
    metadata and read/write data.

    Attributes:
        path: Path to the dataset
        width: Width in pixels (raster only)
        height: Height in pixels (raster only)
        band_count: Number of bands (raster only)
    """

    @property
    def path(self) -> str: ...

    @property
    def width(self) -> int: ...

    @property
    def height(self) -> int: ...

    @property
    def band_count(self) -> int: ...

    def read_band(self, band: int) -> npt.NDArray[np.float64]:
        """Read a raster band as NumPy array.

        Args:
            band: Band number (1-indexed)

        Returns:
            2D NumPy array with shape (height, width)

        Raises:
            ValueError: If band number is invalid
            IOError: If reading fails
        """
        ...

    def write_band(self, band: int, array: npt.NDArray[np.float64]) -> None:
        """Write a NumPy array to a raster band.

        Args:
            band: Band number (1-indexed)
            array: 2D NumPy array to write

        Raises:
            ValueError: If band or array is invalid
            IOError: If writing fails
        """
        ...

    def get_metadata(self) -> dict[str, Any]:
        """Get dataset metadata.

        Returns:
            Dictionary with metadata fields
        """
        ...

    def set_metadata(self, metadata: dict[str, Any]) -> None:
        """Set dataset metadata.

        Args:
            metadata: Metadata dictionary

        Raises:
            IOError: If dataset not opened for writing
        """
        ...

    def close(self) -> None:
        """Close the dataset and flush pending writes."""
        ...

    def __repr__(self) -> str: ...

    def __enter__(self) -> "Dataset": ...

    def __exit__(self, exc_type: Any, exc_value: Any, traceback: Any) -> bool: ...

def version() -> str:
    """Get OxiGDAL version.

    Returns:
        Version string
    """
    ...

def open(path: str, mode: str = "r") -> Dataset:
    """Open a geospatial dataset.

    Args:
        path: Path to file (local or remote URL)
        mode: Open mode - "r" for read (default), "w" for write

    Returns:
        Opened dataset

    Raises:
        IOError: If file cannot be opened
        ValueError: If format is not supported
    """
    ...

def open_raster(path: str) -> Dataset:
    """Open a raster file.

    Args:
        path: Path to raster file

    Returns:
        Opened dataset

    Raises:
        IOError: If file cannot be opened
    """
    ...

def create_raster(
    path: str,
    width: int,
    height: int,
    bands: int = 1,
    dtype: str = "float32",
    crs: Optional[str] = None,
    nodata: Optional[float] = None,
) -> Dataset:
    """Create a new raster file.

    Args:
        path: Output path
        width: Width in pixels
        height: Height in pixels
        bands: Number of bands (default: 1)
        dtype: Data type (default: "float32")
        crs: CRS as WKT or EPSG code
        nodata: NoData value

    Returns:
        Created dataset opened for writing

    Raises:
        IOError: If file cannot be created
        ValueError: If parameters are invalid
    """
    ...

def calc(expression: str, **arrays: npt.NDArray[np.float64]) -> npt.NDArray[np.float64]:
    """Raster calculator - evaluate expressions on raster data.

    Performs pixel-wise calculations using algebraic expressions.
    Variables A-Z can reference input arrays.

    Args:
        expression: Mathematical expression (e.g., "(A - B) / (A + B)")
        **arrays: Named NumPy arrays (A=array1, B=array2, etc.)

    Returns:
        Result array

    Raises:
        ValueError: If expression is invalid or arrays have different shapes

    Examples:
        >>> # Calculate NDVI
        >>> ndvi = calc("(NIR - RED) / (NIR + RED)", NIR=band4, RED=band3)
        >>>
        >>> # Simple arithmetic
        >>> scaled = calc("A * 2 + 10", A=data)
    """
    ...

def warp(
    src_path: str,
    dst_path: str,
    dst_crs: Optional[str] = None,
    width: Optional[int] = None,
    height: Optional[int] = None,
    resampling: str = "bilinear",
) -> None:
    """Reproject (warp) a raster to different CRS or resolution.

    Args:
        src_path: Source raster path
        dst_path: Destination raster path
        dst_crs: Target CRS (EPSG code or WKT)
        width: Target width in pixels
        height: Target height in pixels
        resampling: Resampling method ("nearest", "bilinear", "cubic")

    Raises:
        IOError: If reading or writing fails
        ValueError: If parameters are invalid

    Examples:
        >>> # Reproject to Web Mercator
        >>> warp("input.tif", "output.tif", dst_crs="EPSG:3857")
        >>>
        >>> # Resize raster
        >>> warp("input.tif", "output.tif", width=1024, height=1024)
    """
    ...

def read_geojson(path: str) -> dict[str, Any]:
    """Read a GeoJSON file.

    Args:
        path: Path to GeoJSON file

    Returns:
        Parsed GeoJSON as dictionary

    Raises:
        IOError: If file cannot be read
        ValueError: If JSON is invalid
    """
    ...

def write_geojson(path: str, data: dict[str, Any], pretty: bool = True) -> None:
    """Write a GeoJSON file.

    Args:
        path: Output path
        data: GeoJSON data as dictionary
        pretty: Pretty-print JSON (default: True)

    Raises:
        IOError: If file cannot be written
        ValueError: If data is invalid
    """
    ...

def buffer_geometry(
    geometry: dict[str, Any],
    distance: float,
    segments: int = 8,
) -> dict[str, Any]:
    """Buffer a geometry by specified distance.

    Args:
        geometry: GeoJSON geometry
        distance: Buffer distance in geometry units
        segments: Number of segments per quadrant (default: 8)

    Returns:
        Buffered geometry as GeoJSON

    Raises:
        ValueError: If geometry is invalid
    """
    ...
