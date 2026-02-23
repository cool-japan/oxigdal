"""
Type stubs for OxiGDAL Python bindings.

This module provides type hints for the OxiGDAL library, enabling
better IDE support, type checking with mypy/pyright, and improved
code completion.
"""

from typing import Any, Dict, List, Optional, Tuple, Union, Literal, overload
import numpy as np
import numpy.typing as npt

__version__: str
__author__: str

# Type aliases
NDArrayFloat = npt.NDArray[np.float64]
NDArrayInt = npt.NDArray[np.int64]
GeometryDict = Dict[str, Any]
FeatureCollection = Dict[str, Any]
BoundingBox = Tuple[float, float, float, float]
GeoTransform = Tuple[float, float, float, float, float, float]

class RasterMetadata:
    """Metadata for raster datasets."""

    width: int
    height: int
    band_count: int
    data_type: str
    crs: Optional[str]
    nodata: Optional[float]
    geotransform: Optional[List[float]]

    def __init__(
        self,
        width: int,
        height: int,
        band_count: int = 1,
        data_type: str = "float32",
        crs: Optional[str] = None,
        nodata: Optional[float] = None,
        geotransform: Optional[List[float]] = None,
    ) -> None: ...

    def __repr__(self) -> str: ...

    def to_dict(self) -> Dict[str, Any]: ...

    @staticmethod
    def from_dict(data: Dict[str, Any]) -> RasterMetadata: ...

    def get_bounds(self) -> Optional[List[float]]: ...

    def get_resolution(self) -> Optional[List[float]]: ...

class Window:
    """Window specification for reading sub-regions of rasters."""

    col_off: int
    row_off: int
    width: int
    height: int

    def __init__(self, col_off: int, row_off: int, width: int, height: int) -> None: ...

    def __repr__(self) -> str: ...

    @staticmethod
    def from_bounds(bounds: List[float], metadata: RasterMetadata) -> Window: ...

class Dataset:
    """Geospatial dataset for reading and writing raster data."""

    path: str
    width: int
    height: int
    band_count: int

    def read_band(self, band: int) -> NDArrayFloat: ...

    def write_band(self, band: int, array: NDArrayFloat) -> None: ...

    def get_metadata(self) -> Dict[str, Any]: ...

    def set_metadata(self, metadata: Dict[str, Any]) -> None: ...

    def close(self) -> None: ...

    def __repr__(self) -> str: ...

    def __enter__(self) -> Dataset: ...

    def __exit__(self, *args: Any) -> bool: ...

class OxiGdalPyError(Exception):
    """OxiGDAL Python binding error."""
    pass

# Core functions

def open(path: str, mode: Literal["r", "w"] = "r") -> Dataset:
    """
    Opens a geospatial dataset.

    Args:
        path: Path to the file to open (local or remote URL)
        mode: Open mode - "r" for read (default), "w" for write

    Returns:
        An opened dataset object

    Raises:
        IOError: If the file cannot be opened
        ValueError: If the format is not supported
    """
    ...

def version() -> str:
    """
    Returns the version of OxiGDAL.

    Returns:
        Version string
    """
    ...

# Raster I/O functions

def open_raster(
    path: str,
    mode: str = "r",
    driver: Optional[str] = None,
    options: Optional[Dict[str, str]] = None,
) -> Dataset:
    """Opens a raster file."""
    ...

def create_raster(
    path: str,
    width: int,
    height: int,
    bands: int = 1,
    dtype: str = "float32",
    crs: Optional[str] = None,
    nodata: Optional[float] = None,
    geotransform: Optional[List[float]] = None,
    driver: Optional[str] = None,
    options: Optional[Dict[str, str]] = None,
) -> Dataset:
    """Creates a new raster file."""
    ...

def read(
    path: str,
    band: int = 1,
    window: Optional[Window] = None,
    out_shape: Optional[Tuple[int, int]] = None,
    masked: bool = False,
) -> NDArrayFloat:
    """Reads a raster band as NumPy array."""
    ...

def read_bands(
    path: str,
    window: Optional[Window] = None,
    out_shape: Optional[Tuple[int, int]] = None,
    bands: Optional[List[int]] = None,
) -> npt.NDArray[np.float64]:
    """Reads all bands as a 3D NumPy array."""
    ...

def write(
    path: str,
    array: Union[NDArrayFloat, npt.NDArray[np.float64]],
    metadata: Optional[Union[RasterMetadata, Dict[str, Any]]] = None,
    driver: Optional[str] = None,
    compress: Optional[str] = None,
    tiled: bool = False,
    blocksize: int = 256,
    overviews: Optional[List[int]] = None,
) -> None:
    """Writes a NumPy array to a raster file."""
    ...

def get_metadata(path: str) -> RasterMetadata:
    """Gets raster metadata."""
    ...

# Raster processing functions

def calc(expression: str, **arrays: NDArrayFloat) -> NDArrayFloat:
    """
    Raster calculator - evaluates expressions on raster data.

    Args:
        expression: Mathematical expression (e.g., "(A - B) / (A + B)")
        **arrays: Named NumPy arrays (A=array1, B=array2, etc.)

    Returns:
        Result array
    """
    ...

def warp(
    src_path: str,
    dst_path: str,
    dst_crs: Optional[str] = None,
    width: Optional[int] = None,
    height: Optional[int] = None,
    resampling: str = "bilinear",
    src_nodata: Optional[float] = None,
    dst_nodata: Optional[float] = None,
    cutline: Optional[str] = None,
    options: Optional[Dict[str, str]] = None,
) -> None:
    """Reprojects (warps) a raster to a different CRS or resolution."""
    ...

def resample(
    src_path: str,
    dst_path: str,
    target_resolution: Tuple[float, float],
    resampling: str = "bilinear",
    nodata: Optional[float] = None,
) -> None:
    """Resamples a raster to a different resolution."""
    ...

def clip(
    src_path: str,
    dst_path: str,
    geometry: Optional[GeometryDict] = None,
    bounds: Optional[List[float]] = None,
    nodata: Optional[float] = None,
) -> None:
    """Clips a raster to a geometry or bounds."""
    ...

def merge(
    src_paths: List[str],
    dst_path: str,
    nodata: Optional[float] = None,
    method: str = "first",
    target_aligned_pixels: bool = False,
) -> None:
    """Merges multiple rasters into a single raster."""
    ...

def translate(
    src_path: str,
    dst_path: str,
    driver: Optional[str] = None,
    options: Optional[Dict[str, str]] = None,
    strict: bool = False,
) -> None:
    """Translates (copies) a raster with format conversion."""
    ...

def build_overviews(
    path: str,
    levels: List[int],
    resampling: str = "average",
) -> None:
    """Builds overviews (pyramids) for a raster."""
    ...

# Vector I/O functions

def read_geojson(
    path: str,
    layer: Optional[str] = None,
    bbox: Optional[BoundingBox] = None,
    where_clause: Optional[str] = None,
) -> FeatureCollection:
    """Reads a GeoJSON file."""
    ...

def write_geojson(
    path: str,
    data: FeatureCollection,
    pretty: bool = True,
    precision: Optional[int] = None,
    driver: Optional[str] = None,
) -> None:
    """Writes a GeoJSON file."""
    ...

def read_shapefile(
    path: str,
    encoding: str = "utf-8",
    bbox: Optional[BoundingBox] = None,
    where_clause: Optional[str] = None,
) -> FeatureCollection:
    """Reads a Shapefile."""
    ...

def write_shapefile(
    path: str,
    data: FeatureCollection,
    encoding: str = "utf-8",
    driver: Optional[str] = None,
) -> None:
    """Writes a Shapefile."""
    ...

# Vector geometry operations

def buffer_geometry(
    geometry: GeometryDict,
    distance: float,
    segments: int = 8,
    cap_style: str = "round",
    join_style: str = "round",
    mitre_limit: float = 5.0,
) -> GeometryDict:
    """Buffers a geometry by a specified distance."""
    ...

def union(geom1: GeometryDict, geom2: GeometryDict) -> GeometryDict:
    """Computes the union of two geometries."""
    ...

def intersection(geom1: GeometryDict, geom2: GeometryDict) -> GeometryDict:
    """Computes the intersection of two geometries."""
    ...

def difference(geom1: GeometryDict, geom2: GeometryDict) -> GeometryDict:
    """Computes the difference of two geometries (geom1 - geom2)."""
    ...

def symmetric_difference(geom1: GeometryDict, geom2: GeometryDict) -> GeometryDict:
    """Computes the symmetric difference of two geometries."""
    ...

def simplify(
    geometry: GeometryDict,
    tolerance: float,
    preserve_topology: bool = True,
) -> GeometryDict:
    """Simplifies a geometry using the Douglas-Peucker algorithm."""
    ...

def centroid(geometry: GeometryDict) -> GeometryDict:
    """Computes the centroid of a geometry."""
    ...

def convex_hull(geometry: GeometryDict) -> GeometryDict:
    """Computes the convex hull of a geometry."""
    ...

def envelope(geometry: GeometryDict) -> BoundingBox:
    """Computes the envelope (bounding box) of a geometry."""
    ...

# Vector spatial predicates

def intersects(geom1: GeometryDict, geom2: GeometryDict) -> bool:
    """Tests if two geometries intersect."""
    ...

def contains(geom1: GeometryDict, geom2: GeometryDict) -> bool:
    """Tests if geom1 contains geom2."""
    ...

def within(geom1: GeometryDict, geom2: GeometryDict) -> bool:
    """Tests if geom1 is within geom2."""
    ...

def touches(geom1: GeometryDict, geom2: GeometryDict) -> bool:
    """Tests if two geometries touch."""
    ...

def overlaps(geom1: GeometryDict, geom2: GeometryDict) -> bool:
    """Tests if two geometries overlap."""
    ...

def crosses(geom1: GeometryDict, geom2: GeometryDict) -> bool:
    """Tests if two geometries cross."""
    ...

def disjoint(geom1: GeometryDict, geom2: GeometryDict) -> bool:
    """Tests if two geometries are disjoint."""
    ...

# Vector measurements

def area(geometry: GeometryDict, geodesic: bool = False) -> float:
    """Calculates the area of a geometry."""
    ...

def length(geometry: GeometryDict, geodesic: bool = False) -> float:
    """Calculates the length of a geometry."""
    ...

def distance(geom1: GeometryDict, geom2: GeometryDict, geodesic: bool = False) -> float:
    """Computes the distance between two geometries."""
    ...

# Vector utilities

def is_valid(geometry: GeometryDict) -> Tuple[bool, Optional[str]]:
    """Validates a geometry."""
    ...

def make_valid(geometry: GeometryDict) -> GeometryDict:
    """Makes a geometry valid."""
    ...

def transform(geometry: GeometryDict, src_crs: str, dst_crs: str) -> GeometryDict:
    """Transforms geometry coordinates to a different CRS."""
    ...

def clip_by_bbox(geometries: Any, bbox: BoundingBox) -> List[GeometryDict]:
    """Clips geometries by a bounding box."""
    ...

def merge_polygons(polygons: List[GeometryDict]) -> List[GeometryDict]:
    """Merges overlapping polygons."""
    ...

def dissolve(features: FeatureCollection, attribute: str) -> FeatureCollection:
    """Dissolves polygons based on an attribute."""
    ...

# Algorithm functions - Statistics

def statistics(
    array: NDArrayFloat,
    nodata: Optional[float] = None,
    compute_percentiles: bool = False,
    percentiles: Optional[List[float]] = None,
) -> Dict[str, float]:
    """Calculates statistics for a raster array."""
    ...

def histogram(
    array: NDArrayFloat,
    bins: int = 256,
    range: Optional[Tuple[float, float]] = None,
    nodata: Optional[float] = None,
) -> Tuple[List[int], List[float]]:
    """Computes histogram for a raster array."""
    ...

# Algorithm functions - Filters

def convolve(
    array: NDArrayFloat,
    kernel: NDArrayFloat,
    normalize: bool = False,
    boundary: str = "reflect",
    fill_value: float = 0.0,
) -> NDArrayFloat:
    """Applies convolution filter to a raster array."""
    ...

def gaussian_blur(
    array: NDArrayFloat,
    sigma: float,
    kernel_size: Optional[int] = None,
    truncate: float = 4.0,
) -> NDArrayFloat:
    """Applies Gaussian blur filter."""
    ...

def median_filter(
    array: NDArrayFloat,
    size: int,
    nodata: Optional[float] = None,
) -> NDArrayFloat:
    """Applies median filter."""
    ...

# Algorithm functions - Morphology

def erosion(
    array: NDArrayFloat,
    kernel: Optional[NDArrayFloat] = None,
    iterations: int = 1,
) -> NDArrayFloat:
    """Applies morphological erosion."""
    ...

def dilation(
    array: NDArrayFloat,
    kernel: Optional[NDArrayFloat] = None,
    iterations: int = 1,
) -> NDArrayFloat:
    """Applies morphological dilation."""
    ...

def opening(
    array: NDArrayFloat,
    kernel: Optional[NDArrayFloat] = None,
    iterations: int = 1,
) -> NDArrayFloat:
    """Applies morphological opening (erosion followed by dilation)."""
    ...

def closing(
    array: NDArrayFloat,
    kernel: Optional[NDArrayFloat] = None,
    iterations: int = 1,
) -> NDArrayFloat:
    """Applies morphological closing (dilation followed by erosion)."""
    ...

# Algorithm functions - Spectral Indices

def ndvi(
    nir: NDArrayFloat,
    red: NDArrayFloat,
    nodata: Optional[float] = None,
) -> NDArrayFloat:
    """Calculates NDVI (Normalized Difference Vegetation Index)."""
    ...

def evi(
    nir: NDArrayFloat,
    red: NDArrayFloat,
    blue: NDArrayFloat,
    G: float = 2.5,
    C1: float = 6.0,
    C2: float = 7.5,
    L: float = 1.0,
) -> NDArrayFloat:
    """Calculates EVI (Enhanced Vegetation Index)."""
    ...

def ndwi(
    green: NDArrayFloat,
    nir: NDArrayFloat,
    nodata: Optional[float] = None,
) -> NDArrayFloat:
    """Calculates NDWI (Normalized Difference Water Index)."""
    ...

# Algorithm functions - Classification

def kmeans_classify(
    bands: List[NDArrayFloat],
    n_clusters: int,
    max_iter: int = 100,
    tolerance: float = 0.001,
    nodata: Optional[float] = None,
) -> NDArrayInt:
    """Performs unsupervised k-means classification."""
    ...

def supervised_classify(
    bands: List[NDArrayFloat],
    training_data: Dict[int, List[Tuple[int, int]]],
    method: str = "maximum_likelihood",
) -> NDArrayInt:
    """Performs supervised classification using training samples."""
    ...

# Algorithm functions - Edge Detection

def sobel_edges(
    array: NDArrayFloat,
    direction: str = "both",
    threshold: Optional[float] = None,
) -> NDArrayFloat:
    """Detects edges using Sobel operator."""
    ...

def canny_edges(
    array: NDArrayFloat,
    low_threshold: float,
    high_threshold: float,
    sigma: float = 1.0,
) -> NDArrayFloat:
    """Applies Canny edge detection."""
    ...
