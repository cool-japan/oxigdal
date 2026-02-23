#!/usr/bin/env python3
"""Example: NDVI calculation with OxiGDAL.

This example demonstrates vegetation index calculation using the raster calculator.
"""

import numpy as np
import oxigdal


def create_synthetic_multispectral() -> tuple[np.ndarray, np.ndarray]:
    """Create synthetic RED and NIR bands.

    Returns:
        Tuple of (red, nir) NumPy arrays
    """
    width, height = 1024, 1024

    # Create coordinate grids
    x = np.linspace(0, 10, width)
    y = np.linspace(0, 10, height)
    X, Y = np.meshgrid(x, y)

    # Simulate vegetation patterns
    # Higher NIR, lower RED = healthy vegetation
    vegetation_mask = (
        (X > 2) & (X < 8) & (Y > 2) & (Y < 8) &
        (np.sin(X) * np.cos(Y) > 0)
    )

    # Base reflectance values
    red = np.ones((height, width)) * 0.3
    nir = np.ones((height, width)) * 0.3

    # Vegetation areas: low RED, high NIR
    red[vegetation_mask] = 0.05 + np.random.rand(np.sum(vegetation_mask)) * 0.1
    nir[vegetation_mask] = 0.4 + np.random.rand(np.sum(vegetation_mask)) * 0.3

    # Soil/bare areas: higher RED
    soil_mask = ~vegetation_mask & (X < 5)
    red[soil_mask] = 0.2 + np.random.rand(np.sum(soil_mask)) * 0.2
    nir[soil_mask] = 0.25 + np.random.rand(np.sum(soil_mask)) * 0.15

    # Add some noise
    red += np.random.randn(height, width) * 0.02
    nir += np.random.randn(height, width) * 0.02

    # Clip to valid range
    red = np.clip(red, 0, 1)
    nir = np.clip(nir, 0, 1)

    return red, nir


def main() -> None:
    """Main example function."""
    print("OxiGDAL NDVI Calculation Example")
    print(f"Version: {oxigdal.version()}")
    print()

    # Create synthetic multispectral data
    print("=" * 60)
    print("Creating synthetic multispectral imagery")
    print("=" * 60)

    red, nir = create_synthetic_multispectral()
    print(f"RED band shape: {red.shape}")
    print(f"  Range: [{red.min():.3f}, {red.max():.3f}]")
    print(f"NIR band shape: {nir.shape}")
    print(f"  Range: [{nir.min():.3f}, {nir.max():.3f}]")
    print()

    # Save bands to files
    print("=" * 60)
    print("Saving multispectral bands")
    print("=" * 60)

    red_path = "/tmp/red_band.tif"
    nir_path = "/tmp/nir_band.tif"

    # Save RED band
    with oxigdal.create_raster(
        red_path,
        width=red.shape[1],
        height=red.shape[0],
        bands=1,
        dtype="float32",
        crs="EPSG:4326",
    ) as ds:
        ds.write_band(1, red.astype(np.float32))

    print(f"Saved RED band: {red_path}")

    # Save NIR band
    with oxigdal.create_raster(
        nir_path,
        width=nir.shape[1],
        height=nir.shape[0],
        bands=1,
        dtype="float32",
        crs="EPSG:4326",
    ) as ds:
        ds.write_band(1, nir.astype(np.float32))

    print(f"Saved NIR band: {nir_path}")
    print()

    # Calculate NDVI
    print("=" * 60)
    print("Calculating NDVI")
    print("=" * 60)

    # NDVI = (NIR - RED) / (NIR + RED)
    print("Formula: NDVI = (NIR - RED) / (NIR + RED)")

    ndvi = oxigdal.calc(
        "(NIR - RED) / (NIR + RED)",
        NIR=nir,
        RED=red,
    )

    print(f"NDVI range: [{ndvi.min():.3f}, {ndvi.max():.3f}]")

    # Analyze results
    print()
    print("NDVI Interpretation:")
    print("  < 0.0  : Water, clouds, snow")
    print("  0.0-0.2: Bare soil, rock")
    print("  0.2-0.4: Sparse vegetation")
    print("  0.4-0.6: Moderate vegetation")
    print("  > 0.6  : Dense vegetation")
    print()

    # Calculate statistics
    water_mask = ndvi < 0.0
    bare_soil = (ndvi >= 0.0) & (ndvi < 0.2)
    sparse_veg = (ndvi >= 0.2) & (ndvi < 0.4)
    moderate_veg = (ndvi >= 0.4) & (ndvi < 0.6)
    dense_veg = ndvi >= 0.6

    total_pixels = ndvi.size
    print("Land cover distribution:")
    print(f"  Water/clouds: {np.sum(water_mask) / total_pixels * 100:.1f}%")
    print(f"  Bare soil: {np.sum(bare_soil) / total_pixels * 100:.1f}%")
    print(f"  Sparse vegetation: {np.sum(sparse_veg) / total_pixels * 100:.1f}%")
    print(f"  Moderate vegetation: {np.sum(moderate_veg) / total_pixels * 100:.1f}%")
    print(f"  Dense vegetation: {np.sum(dense_veg) / total_pixels * 100:.1f}%")
    print()

    # Save NDVI
    print("=" * 60)
    print("Saving NDVI result")
    print("=" * 60)

    ndvi_path = "/tmp/ndvi.tif"
    with oxigdal.create_raster(
        ndvi_path,
        width=ndvi.shape[1],
        height=ndvi.shape[0],
        bands=1,
        dtype="float32",
        crs="EPSG:4326",
        nodata=-9999.0,
    ) as ds:
        ds.write_band(1, ndvi.astype(np.float32))
        ds.set_metadata({
            "description": "Normalized Difference Vegetation Index",
            "formula": "(NIR - RED) / (NIR + RED)",
        })

    print(f"Saved NDVI: {ndvi_path}")
    print()

    # Calculate other vegetation indices
    print("=" * 60)
    print("Calculating other vegetation indices")
    print("=" * 60)

    # SAVI (Soil Adjusted Vegetation Index)
    # SAVI = ((NIR - RED) / (NIR + RED + L)) * (1 + L)
    # where L = 0.5 for moderate vegetation
    print("1. SAVI (Soil Adjusted Vegetation Index)")
    savi = oxigdal.calc(
        "((NIR - RED) / (NIR + RED + 0.5)) * 1.5",
        NIR=nir,
        RED=red,
    )
    print(f"   Range: [{savi.min():.3f}, {savi.max():.3f}]")

    # GNDVI (Green NDVI) - would need green band
    # For demonstration, use a simplified version
    print("2. ARVI (Atmospherically Resistant Vegetation Index)")
    # ARVI = (NIR - RB) / (NIR + RB) where RB = RED - (RED - BLUE)
    # Simplified version without blue band
    arvi = oxigdal.calc(
        "(NIR - RED) / (NIR + RED)",
        NIR=nir,
        RED=red,
    )
    print(f"   Range: [{arvi.min():.3f}, {arvi.max():.3f}]")

    # Vegetation fraction
    print("3. Vegetation Fraction")
    # Scale NDVI to 0-1 range
    veg_fraction = (ndvi - ndvi.min()) / (ndvi.max() - ndvi.min())
    print(f"   Mean vegetation fraction: {veg_fraction.mean():.3f}")

    print()

    # Save all indices
    print("=" * 60)
    print("Saving all vegetation indices")
    print("=" * 60)

    indices_path = "/tmp/vegetation_indices.tif"
    with oxigdal.create_raster(
        indices_path,
        width=ndvi.shape[1],
        height=ndvi.shape[0],
        bands=3,
        dtype="float32",
        crs="EPSG:4326",
    ) as ds:
        ds.write_band(1, ndvi.astype(np.float32))
        ds.write_band(2, savi.astype(np.float32))
        ds.write_band(3, veg_fraction.astype(np.float32))
        ds.set_metadata({
            "band_1": "NDVI",
            "band_2": "SAVI",
            "band_3": "Vegetation Fraction",
        })

    print(f"Saved all indices: {indices_path}")
    print()

    print("=" * 60)
    print("NDVI calculation completed successfully!")
    print("=" * 60)


if __name__ == "__main__":
    main()
