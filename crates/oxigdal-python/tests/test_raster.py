"""Tests for raster operations in OxiGDAL Python bindings."""

import tempfile
from pathlib import Path
import pytest
import numpy as np

# Import will work after building the package
try:
    import oxigdal
except ImportError:
    pytest.skip("oxigdal not built yet", allow_module_level=True)


class TestRasterMetadata:
    """Tests for RasterMetadata class."""

    def test_metadata_creation(self) -> None:
        """Test creating raster metadata."""
        meta = oxigdal.RasterMetadata(
            width=512,
            height=512,
            band_count=3,
            data_type="float32",
            crs="EPSG:4326",
            nodata=-9999.0,
        )

        assert meta.width == 512
        assert meta.height == 512
        assert meta.band_count == 3
        assert meta.data_type == "float32"
        assert meta.crs == "EPSG:4326"
        assert meta.nodata == -9999.0

    def test_metadata_defaults(self) -> None:
        """Test metadata with default values."""
        meta = oxigdal.RasterMetadata(
            width=100,
            height=200,
        )

        assert meta.width == 100
        assert meta.height == 200
        assert meta.band_count == 1
        assert meta.data_type == "float32"
        assert meta.crs is None
        assert meta.nodata is None

    def test_metadata_repr(self) -> None:
        """Test string representation."""
        meta = oxigdal.RasterMetadata(
            width=100,
            height=200,
            band_count=1,
            data_type="uint8",
        )

        repr_str = repr(meta)
        assert "width=100" in repr_str
        assert "height=200" in repr_str
        assert "bands=1" in repr_str
        assert "dtype=uint8" in repr_str

    def test_metadata_to_dict(self) -> None:
        """Test conversion to dictionary."""
        meta = oxigdal.RasterMetadata(
            width=256,
            height=128,
            band_count=2,
            data_type="int16",
            crs="EPSG:3857",
            nodata=0.0,
        )

        meta_dict = meta.to_dict()
        assert meta_dict["width"] == 256
        assert meta_dict["height"] == 128
        assert meta_dict["band_count"] == 2
        assert meta_dict["data_type"] == "int16"
        assert meta_dict["crs"] == "EPSG:3857"
        assert meta_dict["nodata"] == 0.0


class TestRasterIO:
    """Tests for raster I/O operations."""

    def test_create_raster_validation(self) -> None:
        """Test parameter validation for create_raster."""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "test.tif"

            # Zero width should fail
            with pytest.raises(ValueError, match="positive"):
                oxigdal.create_raster(str(path), width=0, height=100)

            # Zero height should fail
            with pytest.raises(ValueError, match="positive"):
                oxigdal.create_raster(str(path), width=100, height=0)

            # Zero bands should fail
            with pytest.raises(ValueError, match="positive"):
                oxigdal.create_raster(str(path), width=100, height=100, bands=0)

    def test_version(self) -> None:
        """Test version function."""
        version = oxigdal.version()
        assert isinstance(version, str)
        assert len(version) > 0


class TestRasterCalculator:
    """Tests for raster calculator."""

    def test_calc_basic(self) -> None:
        """Test basic calculator operation."""
        # Create test data
        data = np.random.rand(10, 10)

        # Simple identity operation
        result = oxigdal.calc("A", A=data)

        assert result.shape == data.shape
        assert isinstance(result, np.ndarray)

    def test_calc_no_arrays(self) -> None:
        """Test calculator with no input arrays."""
        with pytest.raises(ValueError, match="No input arrays"):
            oxigdal.calc("A + B")


class TestWarp:
    """Tests for warp operation."""

    def test_warp_invalid_resampling(self) -> None:
        """Test warp with invalid resampling method."""
        with pytest.raises(ValueError, match="Invalid resampling"):
            oxigdal.warp(
                "input.tif",
                "output.tif",
                resampling="invalid_method",
            )


class TestDataset:
    """Tests for Dataset class."""

    def test_dataset_repr(self) -> None:
        """Test dataset string representation."""
        # Note: This test assumes a file exists or will handle the error
        # In practice, you'd use a fixture with a real test file
        pass

    def test_dataset_context_manager(self) -> None:
        """Test dataset as context manager."""
        # Note: Would need a real file for full test
        pass


def test_module_attributes() -> None:
    """Test module-level attributes."""
    assert hasattr(oxigdal, "__version__")
    assert hasattr(oxigdal, "__author__")
    assert oxigdal.__author__ == "COOLJAPAN OU (Team Kitasan)"


def test_exception_hierarchy() -> None:
    """Test exception types."""
    assert issubclass(oxigdal.OxiGdalError, Exception)


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
