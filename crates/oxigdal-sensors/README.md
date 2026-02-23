# OxiGDAL Sensors

Remote sensing and satellite sensor data processing for OxiGDAL.

## Features

### Sensor Support

Comprehensive sensor definitions for major satellite platforms:

- **Landsat**: 5 TM, 7 ETM+, 8/9 OLI/TIRS
- **Sentinel**: 2 MSI (optical), 1 SAR
- **MODIS**: Terra/Aqua (36 bands)
- **ASTER**: VNIR, SWIR, and TIR subsystems

### Radiometric Corrections

- DN → Radiance → TOA Reflectance conversion
- Thermal calibration (brightness temperature)
- Earth-Sun distance calculation
- Band-specific calibration parameters

### Atmospheric Correction

- Dark Object Subtraction (DOS)
- Cosine correction for topography
- Haze removal
- BRDF normalization (Ross-Thick Li-Sparse)

### Spectral Indices (20+)

#### Vegetation Indices
- NDVI, EVI, EVI2
- SAVI, MSAVI, OSAVI
- GNDVI, GRVI, CI
- NDWI, NDMI

#### Burn Indices
- NBR, dNBR, NBR2

#### Urban Indices
- NDBI, UI, IBI

#### Water Indices
- MNDWI, AWEI, WRI

### Pan-Sharpening

- Brovey Transform
- IHS (Intensity-Hue-Saturation)
- PCA (Principal Component Analysis)

### Image Classification

- Unsupervised: K-Means, ISODATA
- Supervised: Maximum Likelihood

## Usage

```rust
use oxigdal_sensors::sensors::landsat;
use oxigdal_sensors::indices::vegetation::NDVI;
use oxigdal_sensors::radiometry::calibration::RadiometricCalibration;
use scirs2_core::ndarray::array;

// Get sensor definition
let sensor = landsat::landsat8_oli_tirs();
let nir_band = sensor.get_band_by_common_name("NIR")
    .ok_or(SensorError::BandNotFound("NIR".into()))?;
let red_band = sensor.get_band_by_common_name("Red")
    .ok_or(SensorError::BandNotFound("Red".into()))?;

// Radiometric calibration
let cal = RadiometricCalibration::new(0.00002, 0.0)
    .with_solar_irradiance(1554.0);

let dn = array![[10000.0, 12000.0], [15000.0, 18000.0]];
let radiance = cal.dn_to_radiance(&dn.view());
let toa = cal.radiance_to_reflectance(&radiance.view(), 30.0, 1.0)?;

// Calculate NDVI
let nir = array![[0.5, 0.6], [0.7, 0.8]];
let red = array![[0.1, 0.1], [0.1, 0.1]];
let ndvi = NDVI(&nir.view(), &red.view())?;
```

## COOLJAPAN Policy Compliance

- ✅ **Pure Rust**: 100% Pure Rust, no C/Fortran dependencies
- ✅ **No unwrap()**: Proper error handling with `Result<T, E>`
- ✅ **SciRS2 Integration**: Uses `scirs2-core` for arrays and math
- ✅ **Workspace Policy**: Follows workspace dependency management
- ✅ **No Warnings**: Zero clippy warnings in production code

## License

Apache-2.0

## Author

COOLJAPAN OU (Team Kitasan)
