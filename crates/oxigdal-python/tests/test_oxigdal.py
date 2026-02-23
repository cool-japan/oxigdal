"""
Comprehensive pytest test suite for OxiGDAL Python bindings.

This module provides thorough testing of the OxiGDAL Python API including:
- Raster I/O operations
- Vector I/O operations
- Algorithm functions (statistics, filters, morphology, spectral indices)
- Projection and coordinate transformation
- NumPy integration
- Error handling
- Performance tests
- Round-trip tests

Author: COOLJAPAN OU (Team Kitasan)
"""

import json
import os
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import numpy as np
import pytest
from numpy.testing import assert_allclose, assert_array_equal

# Import will work after building the package
try:
    import oxigdal
except ImportError:
    pytest.skip("oxigdal not built yet", allow_module_level=True)


# ============================================================================
# Test Fixtures
# ============================================================================


@pytest.fixture
def temp_dir():
    """Create a temporary directory for test files."""
    with tempfile.TemporaryDirectory() as tmpdir:
        yield Path(tmpdir)


@pytest.fixture
def sample_raster_data() -> np.ndarray:
    """Generate sample 2D raster data for testing."""
    np.random.seed(42)
    return np.random.rand(100, 100).astype(np.float64)


@pytest.fixture
def sample_multiband_data() -> np.ndarray:
    """Generate sample 3D multi-band raster data."""
    np.random.seed(42)
    return np.random.rand(3, 100, 100).astype(np.float64)


@pytest.fixture
def sample_geojson() -> Dict[str, Any]:
    """Generate sample GeoJSON FeatureCollection."""
    return {
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": {"name": "Test Point", "id": 1},
                "geometry": {
                    "type": "Point",
                    "coordinates": [0.0, 0.0]
                }
            },
            {
                "type": "Feature",
                "properties": {"name": "Test Polygon", "id": 2},
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [
                        [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0]]
                    ]
                }
            }
        ]
    }


@pytest.fixture
def sample_point_geometry() -> Dict[str, Any]:
    """Generate sample Point geometry."""
    return {
        "type": "Point",
        "coordinates": [0.0, 0.0]
    }


@pytest.fixture
def sample_polygon_geometry() -> Dict[str, Any]:
    """Generate sample Polygon geometry."""
    return {
        "type": "Polygon",
        "coordinates": [
            [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0]]
        ]
    }


@pytest.fixture
def sample_linestring_geometry() -> Dict[str, Any]:
    """Generate sample LineString geometry."""
    return {
        "type": "LineString",
        "coordinates": [[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]]
    }


# ============================================================================
# Module-Level Tests
# ============================================================================


class TestModuleAttributes:
    """Tests for module-level attributes and functions."""

    def test_version_exists(self) -> None:
        """Test that __version__ attribute exists."""
        assert hasattr(oxigdal, "__version__")
        assert isinstance(oxigdal.__version__, str)

    def test_version_format(self) -> None:
        """Test version string format (semver-like)."""
        version = oxigdal.__version__
        parts = version.split(".")
        assert len(parts) >= 2, "Version should have at least major.minor"

    def test_author_attribute(self) -> None:
        """Test that __author__ attribute is set correctly."""
        assert hasattr(oxigdal, "__author__")
        assert oxigdal.__author__ == "COOLJAPAN OU (Team Kitasan)"

    def test_version_function(self) -> None:
        """Test version() function returns consistent version."""
        assert oxigdal.version() == oxigdal.__version__

    def test_module_exports(self) -> None:
        """Test that expected functions are exported."""
        expected_exports = [
            "open", "open_raster", "create_raster", "calc", "warp",
            "read_geojson", "write_geojson", "buffer_geometry",
            "Dataset", "RasterMetadata"
        ]
        for name in expected_exports:
            assert hasattr(oxigdal, name), f"Missing export: {name}"


# ============================================================================
# Raster I/O Tests
# ============================================================================


class TestRasterMetadata:
    """Tests for RasterMetadata class."""

    def test_metadata_creation_basic(self) -> None:
        """Test basic metadata creation with required parameters."""
        meta = oxigdal.RasterMetadata(width=512, height=256)
        assert meta.width == 512
        assert meta.height == 256
        assert meta.band_count == 1  # default
        assert meta.data_type == "float32"  # default

    def test_metadata_creation_full(self) -> None:
        """Test metadata creation with all parameters."""
        gt = [0.0, 10.0, 0.0, 1000.0, 0.0, -10.0]
        meta = oxigdal.RasterMetadata(
            width=1024,
            height=768,
            band_count=4,
            data_type="uint16",
            crs="EPSG:4326",
            nodata=-9999.0,
            geotransform=gt
        )
        assert meta.width == 1024
        assert meta.height == 768
        assert meta.band_count == 4
        assert meta.data_type == "uint16"
        assert meta.crs == "EPSG:4326"
        assert meta.nodata == -9999.0
        assert meta.geotransform == gt

    def test_metadata_repr(self) -> None:
        """Test string representation of metadata."""
        meta = oxigdal.RasterMetadata(
            width=100, height=200, band_count=3, data_type="uint8"
        )
        repr_str = repr(meta)
        assert "width=100" in repr_str
        assert "height=200" in repr_str
        assert "bands=3" in repr_str
        assert "dtype=uint8" in repr_str

    def test_metadata_to_dict(self) -> None:
        """Test conversion of metadata to dictionary."""
        meta = oxigdal.RasterMetadata(
            width=256, height=128, band_count=2,
            data_type="int16", crs="EPSG:3857", nodata=0.0
        )
        d = meta.to_dict()
        assert d["width"] == 256
        assert d["height"] == 128
        assert d["band_count"] == 2
        assert d["data_type"] == "int16"
        assert d["crs"] == "EPSG:3857"
        assert d["nodata"] == 0.0

    def test_metadata_from_dict(self) -> None:
        """Test creating metadata from dictionary."""
        d = {"width": 64, "height": 64, "band_count": 1, "data_type": "float64"}
        meta = oxigdal.RasterMetadata.from_dict(d)
        assert meta.width == 64
        assert meta.height == 64

    def test_metadata_geotransform_validation(self) -> None:
        """Test geotransform validation (must have 6 elements)."""
        with pytest.raises(ValueError, match="6 elements"):
            oxigdal.RasterMetadata(
                width=100, height=100,
                geotransform=[0.0, 1.0, 0.0]  # only 3 elements
            )

    def test_metadata_get_bounds(self) -> None:
        """Test getting bounding box from metadata."""
        gt = [100.0, 10.0, 0.0, 200.0, 0.0, -10.0]
        meta = oxigdal.RasterMetadata(
            width=10, height=20, geotransform=gt
        )
        bounds = meta.get_bounds()
        assert bounds is not None
        # minx, miny, maxx, maxy
        assert bounds[0] == 100.0  # minx = origin_x
        assert bounds[2] == 200.0  # maxx = origin_x + width * pixel_width
        assert bounds[3] == 200.0  # maxy = origin_y

    def test_metadata_get_resolution(self) -> None:
        """Test getting pixel resolution from metadata."""
        gt = [0.0, 30.0, 0.0, 0.0, 0.0, -30.0]
        meta = oxigdal.RasterMetadata(
            width=100, height=100, geotransform=gt
        )
        res = meta.get_resolution()
        assert res is not None
        assert res[0] == 30.0
        assert res[1] == 30.0


class TestRasterCreate:
    """Tests for raster creation functions."""

    def test_create_raster_basic(self, temp_dir: Path) -> None:
        """Test basic raster creation."""
        path = str(temp_dir / "test_create.tif")
        ds = oxigdal.create_raster(path, width=100, height=100)
        assert ds is not None

    def test_create_raster_with_bands(self, temp_dir: Path) -> None:
        """Test creating multi-band raster."""
        path = str(temp_dir / "test_multiband.tif")
        ds = oxigdal.create_raster(path, width=50, height=50, bands=4)
        assert ds is not None

    def test_create_raster_with_dtype(self, temp_dir: Path) -> None:
        """Test creating raster with specific data type."""
        path = str(temp_dir / "test_dtype.tif")
        ds = oxigdal.create_raster(path, width=32, height=32, dtype="uint8")
        assert ds is not None

    def test_create_raster_zero_width(self, temp_dir: Path) -> None:
        """Test that zero width raises ValueError."""
        path = str(temp_dir / "test.tif")
        with pytest.raises(ValueError, match="positive"):
            oxigdal.create_raster(path, width=0, height=100)

    def test_create_raster_zero_height(self, temp_dir: Path) -> None:
        """Test that zero height raises ValueError."""
        path = str(temp_dir / "test.tif")
        with pytest.raises(ValueError, match="positive"):
            oxigdal.create_raster(path, width=100, height=0)

    def test_create_raster_zero_bands(self, temp_dir: Path) -> None:
        """Test that zero bands raises ValueError."""
        path = str(temp_dir / "test.tif")
        with pytest.raises(ValueError, match="positive"):
            oxigdal.create_raster(path, width=100, height=100, bands=0)

    def test_create_raster_invalid_dtype(self, temp_dir: Path) -> None:
        """Test that invalid dtype raises ValueError."""
        path = str(temp_dir / "test.tif")
        with pytest.raises(ValueError, match="Invalid dtype"):
            oxigdal.create_raster(path, width=100, height=100, dtype="invalid")


class TestRasterWarp:
    """Tests for raster warping/reprojection."""

    def test_warp_invalid_resampling(self) -> None:
        """Test that invalid resampling method raises ValueError."""
        with pytest.raises(ValueError, match="Invalid resampling"):
            oxigdal.warp("input.tif", "output.tif", resampling="invalid_method")

    def test_warp_valid_resampling_methods(self) -> None:
        """Test all valid resampling methods are accepted."""
        valid_methods = ["nearest", "bilinear", "cubic", "lanczos", "average", "mode"]
        for method in valid_methods:
            # Should not raise
            try:
                oxigdal.warp("input.tif", "output.tif", resampling=method)
            except FileNotFoundError:
                pass  # Expected since files don't exist
            except ValueError as e:
                pytest.fail(f"Valid resampling method '{method}' rejected: {e}")


# ============================================================================
# Vector I/O Tests
# ============================================================================


class TestGeoJSONIO:
    """Tests for GeoJSON I/O operations."""

    def test_write_read_geojson(
        self, temp_dir: Path, sample_geojson: Dict[str, Any]
    ) -> None:
        """Test writing and reading GeoJSON."""
        path = str(temp_dir / "test.geojson")

        # Write
        oxigdal.write_geojson(path, sample_geojson)
        assert os.path.exists(path)

        # Read
        result = oxigdal.read_geojson(path)
        assert result["type"] == "FeatureCollection"

    def test_write_geojson_pretty(
        self, temp_dir: Path, sample_geojson: Dict[str, Any]
    ) -> None:
        """Test writing pretty-printed GeoJSON."""
        path_pretty = str(temp_dir / "pretty.geojson")
        path_compact = str(temp_dir / "compact.geojson")

        oxigdal.write_geojson(path_pretty, sample_geojson, pretty=True)
        oxigdal.write_geojson(path_compact, sample_geojson, pretty=False)

        # Pretty should be larger
        size_pretty = os.path.getsize(path_pretty)
        size_compact = os.path.getsize(path_compact)
        assert size_pretty >= size_compact

    def test_read_geojson_bbox_validation(self) -> None:
        """Test that invalid bbox raises ValueError."""
        with pytest.raises(ValueError, match="4 elements"):
            oxigdal.read_geojson("test.geojson", bbox=[0.0, 0.0])  # only 2 elements

    def test_read_geojson_not_found(self) -> None:
        """Test reading non-existent file raises IOError."""
        with pytest.raises(IOError):
            oxigdal.read_geojson("/nonexistent/path/file.geojson")

    def test_write_geojson_precision_validation(
        self, temp_dir: Path, sample_geojson: Dict[str, Any]
    ) -> None:
        """Test precision validation for GeoJSON writing."""
        path = str(temp_dir / "test.geojson")

        # Invalid precision
        with pytest.raises(ValueError, match="Precision"):
            oxigdal.write_geojson(path, sample_geojson, precision=-1)

        with pytest.raises(ValueError, match="Precision"):
            oxigdal.write_geojson(path, sample_geojson, precision=20)


class TestShapefileIO:
    """Tests for Shapefile I/O operations."""

    def test_read_shapefile_bbox_validation(self) -> None:
        """Test that invalid bbox raises ValueError."""
        with pytest.raises(ValueError, match="4 elements"):
            oxigdal.read_shapefile("test.shp", bbox=[0.0])


# ============================================================================
# Vector Geometry Operations Tests
# ============================================================================


class TestGeometryOperations:
    """Tests for vector geometry operations."""

    def test_buffer_geometry_positive_distance(
        self, sample_point_geometry: Dict[str, Any]
    ) -> None:
        """Test buffering a geometry with positive distance."""
        result = oxigdal.buffer_geometry(sample_point_geometry, distance=100.0)
        assert "type" in result

    def test_buffer_geometry_negative_distance(
        self, sample_point_geometry: Dict[str, Any]
    ) -> None:
        """Test that negative buffer distance raises ValueError."""
        with pytest.raises(ValueError, match="non-negative"):
            oxigdal.buffer_geometry(sample_point_geometry, distance=-1.0)

    def test_buffer_geometry_invalid_segments(
        self, sample_point_geometry: Dict[str, Any]
    ) -> None:
        """Test that invalid segments parameter raises ValueError."""
        with pytest.raises(ValueError, match="positive"):
            oxigdal.buffer_geometry(sample_point_geometry, distance=10.0, segments=0)

    def test_buffer_geometry_invalid_cap_style(
        self, sample_point_geometry: Dict[str, Any]
    ) -> None:
        """Test that invalid cap_style raises ValueError."""
        with pytest.raises(ValueError, match="cap_style"):
            oxigdal.buffer_geometry(
                sample_point_geometry, distance=10.0, cap_style="invalid"
            )

    def test_buffer_geometry_invalid_join_style(
        self, sample_point_geometry: Dict[str, Any]
    ) -> None:
        """Test that invalid join_style raises ValueError."""
        with pytest.raises(ValueError, match="join_style"):
            oxigdal.buffer_geometry(
                sample_point_geometry, distance=10.0, join_style="invalid"
            )

    def test_buffer_geometry_invalid_mitre_limit(
        self, sample_point_geometry: Dict[str, Any]
    ) -> None:
        """Test that invalid mitre_limit raises ValueError."""
        with pytest.raises(ValueError, match="Mitre limit"):
            oxigdal.buffer_geometry(
                sample_point_geometry, distance=10.0, mitre_limit=0.5
            )

    def test_simplify_positive_tolerance(
        self, sample_linestring_geometry: Dict[str, Any]
    ) -> None:
        """Test simplifying a geometry with positive tolerance."""
        result = oxigdal.simplify(sample_linestring_geometry, tolerance=0.1)
        assert "type" in result

    def test_simplify_negative_tolerance(
        self, sample_linestring_geometry: Dict[str, Any]
    ) -> None:
        """Test that negative tolerance raises ValueError."""
        with pytest.raises(ValueError, match="non-negative"):
            oxigdal.simplify(sample_linestring_geometry, tolerance=-1.0)

    def test_union_geometries(
        self, sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test union of two geometries."""
        result = oxigdal.union(sample_polygon_geometry, sample_polygon_geometry)
        assert "type" in result

    def test_intersection_geometries(
        self, sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test intersection of two geometries."""
        result = oxigdal.intersection(sample_polygon_geometry, sample_polygon_geometry)
        assert "type" in result

    def test_difference_geometries(
        self, sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test difference of two geometries."""
        result = oxigdal.difference(sample_polygon_geometry, sample_polygon_geometry)
        assert "type" in result

    def test_symmetric_difference_geometries(
        self, sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test symmetric difference of two geometries."""
        result = oxigdal.symmetric_difference(
            sample_polygon_geometry, sample_polygon_geometry
        )
        assert "type" in result

    def test_centroid(self, sample_polygon_geometry: Dict[str, Any]) -> None:
        """Test computing centroid of a polygon."""
        result = oxigdal.centroid(sample_polygon_geometry)
        assert result["type"] == "Point"
        assert "coordinates" in result

    def test_convex_hull(self, sample_polygon_geometry: Dict[str, Any]) -> None:
        """Test computing convex hull."""
        result = oxigdal.convex_hull(sample_polygon_geometry)
        assert result["type"] == "Polygon"

    def test_envelope(self, sample_polygon_geometry: Dict[str, Any]) -> None:
        """Test computing envelope/bounding box."""
        result = oxigdal.envelope(sample_polygon_geometry)
        assert len(result) == 4  # [minx, miny, maxx, maxy]


class TestSpatialPredicates:
    """Tests for spatial relationship predicates."""

    def test_intersects(
        self,
        sample_point_geometry: Dict[str, Any],
        sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test intersects predicate."""
        result = oxigdal.intersects(sample_point_geometry, sample_polygon_geometry)
        assert isinstance(result, bool)

    def test_contains(
        self,
        sample_point_geometry: Dict[str, Any],
        sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test contains predicate."""
        result = oxigdal.contains(sample_polygon_geometry, sample_point_geometry)
        assert isinstance(result, bool)

    def test_within(
        self,
        sample_point_geometry: Dict[str, Any],
        sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test within predicate."""
        result = oxigdal.within(sample_point_geometry, sample_polygon_geometry)
        assert isinstance(result, bool)

    def test_touches(
        self, sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test touches predicate."""
        result = oxigdal.touches(sample_polygon_geometry, sample_polygon_geometry)
        assert isinstance(result, bool)

    def test_overlaps(
        self, sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test overlaps predicate."""
        result = oxigdal.overlaps(sample_polygon_geometry, sample_polygon_geometry)
        assert isinstance(result, bool)

    def test_crosses(
        self, sample_linestring_geometry: Dict[str, Any]
    ) -> None:
        """Test crosses predicate."""
        result = oxigdal.crosses(sample_linestring_geometry, sample_linestring_geometry)
        assert isinstance(result, bool)

    def test_disjoint(
        self,
        sample_point_geometry: Dict[str, Any],
        sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test disjoint predicate."""
        result = oxigdal.disjoint(sample_point_geometry, sample_polygon_geometry)
        assert isinstance(result, bool)


class TestMeasurements:
    """Tests for geometry measurement functions."""

    def test_area_polygon(self, sample_polygon_geometry: Dict[str, Any]) -> None:
        """Test calculating area of a polygon."""
        result = oxigdal.area(sample_polygon_geometry)
        assert isinstance(result, float)
        assert result >= 0.0

    def test_area_point_error(self, sample_point_geometry: Dict[str, Any]) -> None:
        """Test that area on a point raises ValueError."""
        with pytest.raises(ValueError, match="does not support area"):
            oxigdal.area(sample_point_geometry)

    def test_length_linestring(
        self, sample_linestring_geometry: Dict[str, Any]
    ) -> None:
        """Test calculating length of a linestring."""
        result = oxigdal.length(sample_linestring_geometry)
        assert isinstance(result, float)
        assert result >= 0.0

    def test_length_point_error(self, sample_point_geometry: Dict[str, Any]) -> None:
        """Test that length on a point raises ValueError."""
        with pytest.raises(ValueError, match="does not support length"):
            oxigdal.length(sample_point_geometry)

    def test_distance(
        self,
        sample_point_geometry: Dict[str, Any],
        sample_polygon_geometry: Dict[str, Any]
    ) -> None:
        """Test calculating distance between geometries."""
        result = oxigdal.distance(sample_point_geometry, sample_polygon_geometry)
        assert isinstance(result, float)
        assert result >= 0.0


class TestGeometryValidation:
    """Tests for geometry validation functions."""

    def test_is_valid(self, sample_polygon_geometry: Dict[str, Any]) -> None:
        """Test geometry validation."""
        is_valid, error = oxigdal.is_valid(sample_polygon_geometry)
        assert isinstance(is_valid, bool)
        assert error is None or isinstance(error, str)

    def test_make_valid(self, sample_polygon_geometry: Dict[str, Any]) -> None:
        """Test making a geometry valid."""
        result = oxigdal.make_valid(sample_polygon_geometry)
        assert "type" in result


# ============================================================================
# Algorithm Tests
# ============================================================================


class TestStatistics:
    """Tests for statistical functions."""

    def test_statistics_basic(self, sample_raster_data: np.ndarray) -> None:
        """Test basic statistics calculation."""
        stats = oxigdal.statistics(sample_raster_data)

        assert "min" in stats
        assert "max" in stats
        assert "mean" in stats
        assert "std" in stats
        assert "count" in stats
        assert "sum" in stats
        assert "variance" in stats

        # Validate ranges
        assert stats["min"] <= stats["max"]
        assert stats["count"] == sample_raster_data.size

    def test_statistics_with_nodata(self, sample_raster_data: np.ndarray) -> None:
        """Test statistics with nodata value."""
        data_with_nodata = sample_raster_data.copy()
        data_with_nodata[0:10, 0:10] = -9999.0

        stats = oxigdal.statistics(data_with_nodata, nodata=-9999.0)
        # Count should be reduced
        assert stats["count"] < data_with_nodata.size

    def test_statistics_with_percentiles(self, sample_raster_data: np.ndarray) -> None:
        """Test statistics with percentile computation."""
        stats = oxigdal.statistics(
            sample_raster_data,
            compute_percentiles=True,
            percentiles=[10.0, 50.0, 90.0]
        )

        assert "percentiles" in stats
        assert "median" in stats

    def test_statistics_empty_array(self) -> None:
        """Test statistics with empty array."""
        empty = np.array([[]], dtype=np.float64)
        with pytest.raises(ValueError, match="No valid values"):
            oxigdal.statistics(empty)

    def test_statistics_all_nodata(self) -> None:
        """Test statistics when all values are nodata."""
        data = np.full((10, 10), -9999.0, dtype=np.float64)
        with pytest.raises(ValueError, match="No valid values"):
            oxigdal.statistics(data, nodata=-9999.0)


class TestHistogram:
    """Tests for histogram computation."""

    def test_histogram_basic(self, sample_raster_data: np.ndarray) -> None:
        """Test basic histogram computation."""
        hist, bin_edges = oxigdal.histogram(sample_raster_data, bins=256)

        assert len(hist) == 256
        assert len(bin_edges) == 257  # n+1 edges for n bins
        assert sum(hist) == sample_raster_data.size

    def test_histogram_custom_bins(self, sample_raster_data: np.ndarray) -> None:
        """Test histogram with custom number of bins."""
        hist, bin_edges = oxigdal.histogram(sample_raster_data, bins=50)
        assert len(hist) == 50
        assert len(bin_edges) == 51

    def test_histogram_custom_range(self, sample_raster_data: np.ndarray) -> None:
        """Test histogram with custom value range."""
        hist, bin_edges = oxigdal.histogram(
            sample_raster_data, bins=100, range=(0.0, 0.5)
        )
        assert len(hist) == 100
        assert bin_edges[0] == 0.0
        assert bin_edges[-1] == 0.5

    def test_histogram_invalid_bins(self, sample_raster_data: np.ndarray) -> None:
        """Test histogram with invalid number of bins."""
        with pytest.raises(ValueError, match="at least 2"):
            oxigdal.histogram(sample_raster_data, bins=1)


class TestFilters:
    """Tests for filtering operations."""

    def test_gaussian_blur(self, sample_raster_data: np.ndarray) -> None:
        """Test Gaussian blur filter."""
        result = oxigdal.gaussian_blur(sample_raster_data, sigma=2.0)
        assert result.shape == sample_raster_data.shape
        assert result.dtype == np.float64

    def test_gaussian_blur_invalid_sigma(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test Gaussian blur with invalid sigma."""
        with pytest.raises(ValueError, match="positive"):
            oxigdal.gaussian_blur(sample_raster_data, sigma=0.0)

        with pytest.raises(ValueError, match="positive"):
            oxigdal.gaussian_blur(sample_raster_data, sigma=-1.0)

    def test_gaussian_blur_invalid_kernel_size(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test Gaussian blur with invalid (even) kernel size."""
        with pytest.raises(ValueError, match="odd"):
            oxigdal.gaussian_blur(sample_raster_data, sigma=1.0, kernel_size=4)

    def test_median_filter(self, sample_raster_data: np.ndarray) -> None:
        """Test median filter."""
        result = oxigdal.median_filter(sample_raster_data, size=3)
        assert result.shape == sample_raster_data.shape

    def test_median_filter_invalid_size(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test median filter with invalid size."""
        with pytest.raises(ValueError, match="odd"):
            oxigdal.median_filter(sample_raster_data, size=4)

        with pytest.raises(ValueError, match="at least 3"):
            oxigdal.median_filter(sample_raster_data, size=1)

    def test_convolve_basic(self, sample_raster_data: np.ndarray) -> None:
        """Test basic convolution."""
        kernel = np.ones((3, 3), dtype=np.float64) / 9
        result = oxigdal.convolve(sample_raster_data, kernel)
        assert result.shape == sample_raster_data.shape

    def test_convolve_invalid_kernel_size(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test convolution with even kernel dimensions."""
        kernel = np.ones((4, 4), dtype=np.float64)
        with pytest.raises(ValueError, match="odd"):
            oxigdal.convolve(sample_raster_data, kernel)

    def test_convolve_invalid_boundary(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test convolution with invalid boundary mode."""
        kernel = np.ones((3, 3), dtype=np.float64)
        with pytest.raises(ValueError, match="Invalid boundary"):
            oxigdal.convolve(sample_raster_data, kernel, boundary="invalid")


class TestMorphology:
    """Tests for morphological operations."""

    def test_erosion(self, sample_raster_data: np.ndarray) -> None:
        """Test morphological erosion."""
        binary_data = (sample_raster_data > 0.5).astype(np.float64)
        result = oxigdal.erosion(binary_data)
        assert result.shape == binary_data.shape

    def test_dilation(self, sample_raster_data: np.ndarray) -> None:
        """Test morphological dilation."""
        binary_data = (sample_raster_data > 0.5).astype(np.float64)
        result = oxigdal.dilation(binary_data)
        assert result.shape == binary_data.shape

    def test_opening(self, sample_raster_data: np.ndarray) -> None:
        """Test morphological opening."""
        binary_data = (sample_raster_data > 0.5).astype(np.float64)
        result = oxigdal.opening(binary_data)
        assert result.shape == binary_data.shape

    def test_closing(self, sample_raster_data: np.ndarray) -> None:
        """Test morphological closing."""
        binary_data = (sample_raster_data > 0.5).astype(np.float64)
        result = oxigdal.closing(binary_data)
        assert result.shape == binary_data.shape

    def test_erosion_invalid_iterations(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test erosion with invalid iterations."""
        binary_data = (sample_raster_data > 0.5).astype(np.float64)
        with pytest.raises(ValueError, match="at least 1"):
            oxigdal.erosion(binary_data, iterations=0)


class TestSpectralIndices:
    """Tests for spectral index calculations."""

    def test_ndvi(self) -> None:
        """Test NDVI calculation."""
        np.random.seed(42)
        nir = np.random.rand(50, 50).astype(np.float64) * 0.8 + 0.2
        red = np.random.rand(50, 50).astype(np.float64) * 0.3 + 0.1

        ndvi = oxigdal.ndvi(nir, red)

        assert ndvi.shape == nir.shape
        assert ndvi.min() >= -1.0
        assert ndvi.max() <= 1.0

    def test_ndvi_shape_mismatch(self) -> None:
        """Test NDVI with mismatched band shapes."""
        nir = np.random.rand(50, 50).astype(np.float64)
        red = np.random.rand(60, 60).astype(np.float64)

        with pytest.raises(ValueError, match="same shape"):
            oxigdal.ndvi(nir, red)

    def test_evi(self) -> None:
        """Test EVI calculation."""
        np.random.seed(42)
        nir = np.random.rand(50, 50).astype(np.float64) * 0.8 + 0.2
        red = np.random.rand(50, 50).astype(np.float64) * 0.3 + 0.1
        blue = np.random.rand(50, 50).astype(np.float64) * 0.2 + 0.05

        evi = oxigdal.evi(nir, red, blue)

        assert evi.shape == nir.shape

    def test_evi_shape_mismatch(self) -> None:
        """Test EVI with mismatched band shapes."""
        nir = np.random.rand(50, 50).astype(np.float64)
        red = np.random.rand(50, 50).astype(np.float64)
        blue = np.random.rand(60, 60).astype(np.float64)  # different shape

        with pytest.raises(ValueError, match="same shape"):
            oxigdal.evi(nir, red, blue)

    def test_ndwi(self) -> None:
        """Test NDWI calculation."""
        np.random.seed(42)
        green = np.random.rand(50, 50).astype(np.float64) * 0.5 + 0.2
        nir = np.random.rand(50, 50).astype(np.float64) * 0.7 + 0.3

        ndwi = oxigdal.ndwi(green, nir)

        assert ndwi.shape == green.shape


class TestEdgeDetection:
    """Tests for edge detection algorithms."""

    def test_sobel_edges(self, sample_raster_data: np.ndarray) -> None:
        """Test Sobel edge detection."""
        result = oxigdal.sobel_edges(sample_raster_data)
        assert result.shape == sample_raster_data.shape

    def test_sobel_edges_directions(self, sample_raster_data: np.ndarray) -> None:
        """Test Sobel edge detection with different directions."""
        for direction in ["both", "horizontal", "vertical"]:
            result = oxigdal.sobel_edges(sample_raster_data, direction=direction)
            assert result.shape == sample_raster_data.shape

    def test_sobel_edges_invalid_direction(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test Sobel edge detection with invalid direction."""
        with pytest.raises(ValueError, match="Invalid direction"):
            oxigdal.sobel_edges(sample_raster_data, direction="invalid")

    def test_canny_edges(self, sample_raster_data: np.ndarray) -> None:
        """Test Canny edge detection."""
        result = oxigdal.canny_edges(
            sample_raster_data, low_threshold=0.1, high_threshold=0.3
        )
        assert result.shape == sample_raster_data.shape

    def test_canny_edges_invalid_thresholds(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test Canny edge detection with invalid thresholds."""
        with pytest.raises(ValueError, match="less than"):
            oxigdal.canny_edges(
                sample_raster_data, low_threshold=0.5, high_threshold=0.3
            )

    def test_canny_edges_invalid_sigma(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test Canny edge detection with invalid sigma."""
        with pytest.raises(ValueError, match="positive"):
            oxigdal.canny_edges(
                sample_raster_data, low_threshold=0.1, high_threshold=0.3, sigma=0.0
            )


class TestClassification:
    """Tests for classification algorithms."""

    def test_kmeans_classify(self) -> None:
        """Test k-means classification."""
        np.random.seed(42)
        bands = [
            np.random.rand(50, 50).astype(np.float64),
            np.random.rand(50, 50).astype(np.float64),
            np.random.rand(50, 50).astype(np.float64),
        ]

        result = oxigdal.kmeans_classify(bands, n_clusters=5)

        assert result.shape == (50, 50)
        assert result.dtype in [np.int32, np.int64]

    def test_kmeans_classify_empty_bands(self) -> None:
        """Test k-means with no bands."""
        with pytest.raises(ValueError, match="At least one band"):
            oxigdal.kmeans_classify([], n_clusters=5)

    def test_kmeans_classify_invalid_clusters(self) -> None:
        """Test k-means with invalid number of clusters."""
        bands = [np.random.rand(10, 10).astype(np.float64)]
        with pytest.raises(ValueError, match="at least 2"):
            oxigdal.kmeans_classify(bands, n_clusters=1)

    def test_supervised_classify_invalid_method(self) -> None:
        """Test supervised classification with invalid method."""
        bands = [np.random.rand(10, 10).astype(np.float64)]
        training = {1: [(0, 0), (1, 1)]}

        with pytest.raises(ValueError, match="Invalid method"):
            oxigdal.supervised_classify(bands, training, method="invalid_method")


# ============================================================================
# Raster Calculator Tests
# ============================================================================


class TestRasterCalculator:
    """Tests for raster calculator operations."""

    def test_calc_identity(self) -> None:
        """Test identity operation."""
        data = np.random.rand(10, 10).astype(np.float64)
        result = oxigdal.calc("A", A=data)

        assert result.shape == data.shape
        assert_allclose(result, data, rtol=1e-10)

    def test_calc_addition(self) -> None:
        """Test addition operation."""
        a = np.ones((10, 10), dtype=np.float64) * 2.0
        b = np.ones((10, 10), dtype=np.float64) * 3.0

        result = oxigdal.calc("A + B", A=a, B=b)

        assert result.shape == a.shape
        expected = np.ones((10, 10), dtype=np.float64) * 5.0
        assert_allclose(result, expected, rtol=1e-10)

    def test_calc_subtraction(self) -> None:
        """Test subtraction operation."""
        a = np.ones((10, 10), dtype=np.float64) * 5.0
        b = np.ones((10, 10), dtype=np.float64) * 2.0

        result = oxigdal.calc("A - B", A=a, B=b)

        expected = np.ones((10, 10), dtype=np.float64) * 3.0
        assert_allclose(result, expected, rtol=1e-10)

    def test_calc_scalar_multiply(self) -> None:
        """Test scalar multiplication."""
        a = np.ones((10, 10), dtype=np.float64) * 3.0

        result = oxigdal.calc("A * 2", A=a)

        expected = np.ones((10, 10), dtype=np.float64) * 6.0
        assert_allclose(result, expected, rtol=1e-10)

    def test_calc_ndvi_expression(self) -> None:
        """Test NDVI-style expression."""
        nir = np.ones((10, 10), dtype=np.float64) * 0.8
        red = np.ones((10, 10), dtype=np.float64) * 0.2

        result = oxigdal.calc("(A - B) / (A + B)", A=nir, B=red)

        expected_ndvi = (0.8 - 0.2) / (0.8 + 0.2)  # 0.6
        assert_allclose(result, np.full((10, 10), expected_ndvi), rtol=1e-10)

    def test_calc_no_arrays(self) -> None:
        """Test calculator with no input arrays."""
        with pytest.raises(ValueError, match="No input arrays"):
            oxigdal.calc("A + B")

    def test_calc_shape_mismatch(self) -> None:
        """Test calculator with mismatched array shapes."""
        a = np.random.rand(10, 10).astype(np.float64)
        b = np.random.rand(20, 20).astype(np.float64)

        with pytest.raises(ValueError, match="shape"):
            oxigdal.calc("A + B", A=a, B=b)

    def test_calc_non_array_input(self) -> None:
        """Test calculator with non-array input."""
        with pytest.raises(ValueError, match="NumPy array"):
            oxigdal.calc("A + B", A="not_an_array", B=[1, 2, 3])


# ============================================================================
# Projection Tests
# ============================================================================


class TestProjection:
    """Tests for coordinate transformation functions."""

    def test_transform_geometry(
        self, sample_point_geometry: Dict[str, Any]
    ) -> None:
        """Test transforming geometry coordinates."""
        result = oxigdal.transform(
            sample_point_geometry,
            src_crs="EPSG:4326",
            dst_crs="EPSG:3857"
        )
        assert "type" in result
        assert "coordinates" in result


# ============================================================================
# NumPy Integration Tests
# ============================================================================


class TestNumpyIntegration:
    """Tests for NumPy integration."""

    def test_array_dtypes(self) -> None:
        """Test various NumPy dtypes are handled correctly."""
        dtypes = [np.float32, np.float64]

        for dtype in dtypes:
            data = np.random.rand(20, 20).astype(dtype)
            data_f64 = data.astype(np.float64)  # Convert for API
            stats = oxigdal.statistics(data_f64)
            assert "mean" in stats

    def test_array_contiguity(self) -> None:
        """Test handling of non-contiguous arrays."""
        data = np.random.rand(50, 50).astype(np.float64)
        # Create non-contiguous view
        non_contiguous = data[::2, ::2]

        # Make contiguous copy
        contiguous = np.ascontiguousarray(non_contiguous)
        stats = oxigdal.statistics(contiguous)
        assert "mean" in stats

    def test_array_memory_order(self) -> None:
        """Test C-order vs Fortran-order arrays."""
        data_c = np.ascontiguousarray(np.random.rand(30, 30).astype(np.float64))
        data_f = np.asfortranarray(np.random.rand(30, 30).astype(np.float64))

        # Convert to contiguous for API
        data_f_contig = np.ascontiguousarray(data_f)

        stats_c = oxigdal.statistics(data_c)
        stats_f = oxigdal.statistics(data_f_contig)

        assert "mean" in stats_c
        assert "mean" in stats_f

    def test_large_arrays(self) -> None:
        """Test handling of larger arrays."""
        large_data = np.random.rand(500, 500).astype(np.float64)
        stats = oxigdal.statistics(large_data)

        assert stats["count"] == 250000
        assert "mean" in stats


# ============================================================================
# Error Handling Tests
# ============================================================================


class TestErrorHandling:
    """Tests for error handling."""

    def test_oxigdal_error_class(self) -> None:
        """Test OxiGdalError exception class."""
        err = oxigdal.OxiGdalError("test error message")
        assert str(err) == "test error message"
        assert "OxiGdalError" in repr(err)

    def test_file_not_found_error(self) -> None:
        """Test FileNotFoundError for missing files."""
        with pytest.raises((IOError, FileNotFoundError)):
            oxigdal.open("/nonexistent/path/to/file.tif")

    def test_value_error_propagation(self) -> None:
        """Test that ValueError is properly propagated."""
        with pytest.raises(ValueError):
            oxigdal.RasterMetadata(width=100, height=100, geotransform=[1, 2, 3])


# ============================================================================
# Performance Tests
# ============================================================================


class TestPerformance:
    """Performance-related tests."""

    def test_statistics_performance(self) -> None:
        """Test that statistics computation is reasonably fast."""
        data = np.random.rand(1000, 1000).astype(np.float64)

        start = time.time()
        stats = oxigdal.statistics(data)
        elapsed = time.time() - start

        # Should complete in reasonable time (< 5 seconds)
        assert elapsed < 5.0
        assert "mean" in stats

    def test_ndvi_performance(self) -> None:
        """Test that NDVI computation is reasonably fast."""
        nir = np.random.rand(1000, 1000).astype(np.float64)
        red = np.random.rand(1000, 1000).astype(np.float64)

        start = time.time()
        ndvi = oxigdal.ndvi(nir, red)
        elapsed = time.time() - start

        # Should complete in reasonable time (< 5 seconds)
        assert elapsed < 5.0
        assert ndvi.shape == nir.shape


# ============================================================================
# Round-Trip Tests
# ============================================================================


class TestRoundTrip:
    """Round-trip consistency tests."""

    def test_geojson_roundtrip(
        self, temp_dir: Path, sample_geojson: Dict[str, Any]
    ) -> None:
        """Test GeoJSON write-read round-trip preserves data."""
        path = str(temp_dir / "roundtrip.geojson")

        # Write
        oxigdal.write_geojson(path, sample_geojson)

        # Read back
        result = oxigdal.read_geojson(path)

        # Verify structure preserved
        assert result["type"] == sample_geojson["type"]

    def test_metadata_dict_roundtrip(self) -> None:
        """Test metadata to_dict/from_dict round-trip."""
        original = oxigdal.RasterMetadata(
            width=256,
            height=128,
            band_count=3,
            data_type="float32",
            crs="EPSG:4326",
            nodata=-9999.0
        )

        # Convert to dict and back
        d = original.to_dict()
        restored = oxigdal.RasterMetadata.from_dict(d)

        assert restored.width == original.width
        assert restored.height == original.height
        assert restored.band_count == original.band_count
        assert restored.data_type == original.data_type

    def test_ndvi_value_range(self) -> None:
        """Test that NDVI values are in valid range after computation."""
        np.random.seed(12345)
        nir = np.random.rand(100, 100).astype(np.float64)
        red = np.random.rand(100, 100).astype(np.float64)

        ndvi = oxigdal.ndvi(nir, red)

        # NDVI should always be in [-1, 1]
        assert ndvi.min() >= -1.0 - 1e-10
        assert ndvi.max() <= 1.0 + 1e-10


# ============================================================================
# Integration Tests
# ============================================================================


class TestIntegration:
    """Integration tests combining multiple operations."""

    def test_raster_processing_workflow(
        self, sample_raster_data: np.ndarray
    ) -> None:
        """Test a typical raster processing workflow."""
        # Step 1: Get statistics
        stats = oxigdal.statistics(sample_raster_data)
        assert "mean" in stats

        # Step 2: Apply filter
        blurred = oxigdal.gaussian_blur(sample_raster_data, sigma=1.0)
        assert blurred.shape == sample_raster_data.shape

        # Step 3: Compute histogram
        hist, bins = oxigdal.histogram(blurred, bins=100)
        assert len(hist) == 100

    def test_vector_processing_workflow(
        self, temp_dir: Path, sample_geojson: Dict[str, Any]
    ) -> None:
        """Test a typical vector processing workflow."""
        # Step 1: Write GeoJSON
        path = str(temp_dir / "workflow.geojson")
        oxigdal.write_geojson(path, sample_geojson)

        # Step 2: Read back
        features = oxigdal.read_geojson(path)
        assert features["type"] == "FeatureCollection"


# ============================================================================
# Main Entry Point
# ============================================================================


if __name__ == "__main__":
    pytest.main([__file__, "-v", "--tb=short"])
