#!/usr/bin/env python3
"""Example: Raster processing with OxiGDAL.

This example demonstrates basic raster I/O, metadata access, and array operations.
"""

import numpy as np
import oxigdal


def main() -> None:
    """Main example function."""
    print("OxiGDAL Python Bindings Example")
    print(f"Version: {oxigdal.version()}")
    print()

    # Example 1: Create a synthetic raster
    print("=" * 60)
    print("Example 1: Creating a synthetic raster")
    print("=" * 60)

    # Create test data - elevation model
    width, height = 512, 512
    x = np.linspace(-10, 10, width)
    y = np.linspace(-10, 10, height)
    X, Y = np.meshgrid(x, y)

    # Generate synthetic elevation (Gaussian hills)
    elevation = (
        100 * np.exp(-(X**2 + Y**2) / 10) +
        50 * np.exp(-((X - 5)**2 + (Y - 5)**2) / 5) +
        30 * np.exp(-((X + 5)**2 + (Y + 5)**2) / 8)
    )

    print(f"Created synthetic elevation: {elevation.shape}")
    print(f"  Min: {elevation.min():.2f}m")
    print(f"  Max: {elevation.max():.2f}m")
    print(f"  Mean: {elevation.mean():.2f}m")
    print()

    # Example 2: Create and write raster
    print("=" * 60)
    print("Example 2: Writing raster to file")
    print("=" * 60)

    # Create metadata
    metadata = oxigdal.RasterMetadata(
        width=width,
        height=height,
        band_count=1,
        data_type="float32",
        crs="EPSG:4326",
        nodata=-9999.0,
    )

    print(f"Metadata: {metadata}")
    print()

    # Create raster file
    output_path = "/tmp/synthetic_elevation.tif"
    print(f"Creating raster: {output_path}")

    with oxigdal.create_raster(
        output_path,
        width=width,
        height=height,
        bands=1,
        dtype="float32",
        crs="EPSG:4326",
        nodata=-9999.0,
    ) as ds:
        # Write elevation data
        ds.write_band(1, elevation.astype(np.float32))

        # Set additional metadata
        ds.set_metadata({
            "description": "Synthetic elevation model",
            "units": "meters",
        })

    print("Raster written successfully!")
    print()

    # Example 3: Read raster back
    print("=" * 60)
    print("Example 3: Reading raster from file")
    print("=" * 60)

    with oxigdal.open(output_path) as ds:
        print(f"Dataset: {ds}")
        print(f"  Size: {ds.width}x{ds.height}")
        print(f"  Bands: {ds.band_count}")

        # Get metadata
        meta = ds.get_metadata()
        print(f"  CRS: {meta.get('crs', 'None')}")
        print(f"  NoData: {meta.get('nodata', 'None')}")

        # Read band
        data = ds.read_band(1)
        print(f"  Data shape: {data.shape}")
        print(f"  Data dtype: {data.dtype}")
        print(f"  Data range: [{data.min():.2f}, {data.max():.2f}]")

    print()

    # Example 4: Raster calculator
    print("=" * 60)
    print("Example 4: Raster calculator operations")
    print("=" * 60)

    # Simple operations
    print("Calculating slope (simplified)")
    # In reality, you'd use proper terrain analysis
    slope = oxigdal.calc("A * 0.01", A=elevation)
    print(f"  Slope range: [{slope.min():.2f}, {slope.max():.2f}]")

    # Classification
    print("Classifying elevation zones:")
    print("  Low: < 30m")
    print("  Medium: 30-70m")
    print("  High: > 70m")

    low_areas = np.sum(elevation < 30)
    medium_areas = np.sum((elevation >= 30) & (elevation < 70))
    high_areas = np.sum(elevation >= 70)

    total_pixels = width * height
    print(f"  Low areas: {low_areas / total_pixels * 100:.1f}%")
    print(f"  Medium areas: {medium_areas / total_pixels * 100:.1f}%")
    print(f"  High areas: {high_areas / total_pixels * 100:.1f}%")

    print()

    # Example 5: Multi-band operations
    print("=" * 60)
    print("Example 5: Multi-band raster (RGB)")
    print("=" * 60)

    # Create synthetic RGB data
    red = (elevation / elevation.max() * 255).astype(np.uint8)
    green = (np.sin(elevation / 20) * 127 + 128).astype(np.uint8)
    blue = (np.cos(elevation / 15) * 127 + 128).astype(np.uint8)

    # Create multi-band raster
    rgb_path = "/tmp/synthetic_rgb.tif"
    print(f"Creating RGB raster: {rgb_path}")

    with oxigdal.create_raster(
        rgb_path,
        width=width,
        height=height,
        bands=3,
        dtype="uint8",
        crs="EPSG:4326",
    ) as ds:
        ds.write_band(1, red)
        ds.write_band(2, green)
        ds.write_band(3, blue)

    print("RGB raster created!")
    print()

    print("=" * 60)
    print("Examples completed successfully!")
    print("=" * 60)


if __name__ == "__main__":
    main()
