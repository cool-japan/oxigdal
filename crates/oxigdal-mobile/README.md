# OxiGDAL Mobile SDK

Mobile FFI bindings for OxiGDAL - bringing Pure Rust geospatial capabilities to iOS and Android.

## Overview

OxiGDAL Mobile provides C-compatible FFI bindings that enable OxiGDAL to be used from native iOS (Swift/Objective-C) and Android (Kotlin/Java) applications. This allows mobile developers to leverage OxiGDAL's powerful geospatial capabilities with:

- **Zero-copy operations** for maximum performance
- **Offline-first** COG (Cloud Optimized GeoTIFF) support
- **Pure Rust implementation** - no C/C++ dependencies
- **Memory efficient** design for mobile devices
- **Battery conscious** algorithms
- **Platform-native APIs** (Swift for iOS, Kotlin for Android)

## Features

- ✅ Raster dataset reading (GeoTIFF, COG, PNG, JPEG)
- ✅ Vector dataset reading (GeoJSON, Shapefile, GeoPackage)
- ✅ Map tile generation (XYZ scheme)
- ✅ Image enhancement (brightness, contrast, saturation, gamma)
- ✅ Coordinate transformations
- ✅ Spatial filtering
- ✅ Statistics computation
- ✅ iOS UIImage integration
- ✅ Android Bitmap integration
- ✅ Memory-safe FFI layer
- ✅ Comprehensive error handling

## Architecture

```
┌─────────────────────────────────────────┐
│         Mobile Applications             │
│  ┌──────────────┐  ┌──────────────┐    │
│  │ Swift (iOS)  │  │Kotlin(Android)│    │
│  └──────────────┘  └──────────────┘    │
└─────────────────────────────────────────┘
           │                  │
           ▼                  ▼
┌─────────────────────────────────────────┐
│        Language Bindings                │
│  ┌──────────────┐  ┌──────────────┐    │
│  │OxiGDAL.swift │  │ OxiGDAL.kt   │    │
│  └──────────────┘  └──────────────┘    │
└─────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────┐
│         C FFI Layer (Rust)              │
│  ┌───────────────────────────────────┐  │
│  │ • Error handling                  │  │
│  │ • Type conversions                │  │
│  │ • Memory management               │  │
│  │ • Platform-specific utilities     │  │
│  └───────────────────────────────────┘  │
└─────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────┐
│          OxiGDAL Core                   │
│         (Pure Rust)                     │
└─────────────────────────────────────────┘
```

## Platform Support

### iOS
- **Minimum**: iOS 13.0+
- **Architectures**: arm64 (device), x86_64 (simulator)
- **Integration**: Swift Package Manager, CocoaPods
- **Language**: Swift 5.9+

### Android
- **Minimum**: Android 7.0 (API 24)+
- **Architectures**: arm64-v8a, armeabi-v7a, x86_64
- **Integration**: Gradle, AAR library
- **Language**: Kotlin 1.9+

## Quick Start

### iOS (Swift)

```swift
import OxiGDAL

// Initialize
OxiGDAL.initialize()

// Open dataset
let dataset = try OxiGDAL.open("map.tif")

// Read as UIImage
let image = try dataset.toImage()
imageView.image = image

// Clean up
dataset.close()
```

### Android (Kotlin)

```kotlin
import com.cooljapan.oxigdal.OxiGDAL

// Initialize
OxiGDAL.initialize()

// Open dataset
val dataset = OxiGDAL.open("map.tif")

// Read as Bitmap
val bitmap = dataset.toBitmap()
imageView.setImageBitmap(bitmap)

// Clean up
dataset.close()
```

## Installation

See platform-specific guides:
- [iOS Integration Guide](examples/ios/README.md)
- [Android Integration Guide](examples/android/README.md)

## Building

### Prerequisites

- Rust 1.85+
- For iOS: Xcode 15.0+
- For Android: Android NDK 26+

### iOS Targets

```bash
# Add targets
rustup target add aarch64-apple-ios x86_64-apple-ios

# Build
cargo build --release --target aarch64-apple-ios --features ios
cargo build --release --target x86_64-apple-ios --features ios
```

### Android Targets

```bash
# Add targets
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

# Build
cargo build --release --target aarch64-linux-android --features android
cargo build --release --target armv7-linux-androideabi --features android
cargo build --release --target x86_64-linux-android --features android
```

## API Overview

### Core Types

- `OxiGdalDataset` - Opaque handle to a dataset
- `OxiGdalBand` - Opaque handle to a raster band
- `OxiGdalLayer` - Opaque handle to a vector layer
- `OxiGdalFeature` - Opaque handle to a feature
- `OxiGdalBuffer` - Image buffer with pixel data
- `OxiGdalMetadata` - Dataset metadata (size, bands, CRS, etc.)

### Error Handling

All FFI functions return `OxiGdalErrorCode`:

```c
typedef enum {
    Success = 0,
    NullPointer = 1,
    InvalidArgument = 2,
    FileNotFound = 3,
    IoError = 4,
    UnsupportedFormat = 5,
    OutOfBounds = 6,
    AllocationFailed = 7,
    InvalidUtf8 = 8,
    DriverError = 9,
    ProjectionError = 10,
    Unknown = 99
} OxiGdalErrorCode;
```

Detailed error messages are available via `oxigdal_get_last_error()`.

### Memory Management

- **Handles**: Created by `*_open`/`*_create`, freed by `*_close`/`*_free`
- **Strings**: Returned strings must be freed with `oxigdal_string_free()`
- **Buffers**: Caller-allocated, OxiGDAL writes to them
- **Opaque Types**: Never dereference on FFI side

### Thread Safety

- Error messages are thread-local
- Handles can be used from different threads (with external synchronization)
- No global mutable state

## Features

- `std` (default) - Standard library support
- `ios` - iOS-specific bindings and utilities
- `android` - Android JNI bindings and utilities
- `offline` - Offline COG reading support
- `filters` - Image enhancement filters
- `tiles` - Map tile generation

## Examples

### Reading a Region

```c
// C API
OxiGdalDataset* dataset;
oxigdal_dataset_open("/path/to/map.tif", &dataset);

OxiGdalBuffer* buffer = oxigdal_buffer_alloc(512, 512, 3);
oxigdal_dataset_read_region(dataset, 0, 0, 512, 512, 1, buffer);

// Use buffer...

oxigdal_buffer_free(buffer);
oxigdal_dataset_close(dataset);
```

### Map Tiles

```c
OxiGdalTileCoord coord = { .z = 10, .x = 512, .y = 341 };
OxiGdalTile* tile;
oxigdal_dataset_read_tile(dataset, &coord, 256, &tile);
```

### Metadata

```c
OxiGdalMetadata metadata;
oxigdal_dataset_get_metadata(dataset, &metadata);

printf("Size: %dx%d\n", metadata.width, metadata.height);
printf("Bands: %d\n", metadata.band_count);
printf("EPSG: %d\n", metadata.epsg_code);
```

## Testing

```bash
# Run tests
cargo test --features std

# Run iOS tests
cargo test --target aarch64-apple-ios --features ios

# Run Android tests
cargo test --target aarch64-linux-android --features android
```

## Performance

The mobile SDK is optimized for:

- **Low memory footprint** - Streaming operations, no large allocations
- **Battery efficiency** - Minimal CPU usage, efficient algorithms
- **Fast startup** - Lazy initialization, minimal setup
- **Offline performance** - Local file operations, no network required

## Limitations

- Maximum dataset size depends on available device memory
- Some operations may be slower on older devices
- Mobile-native exposure of all 11 format drivers still expanding (v0.2.0)

## Roadmap

| Release | Feature |
|---------|--------|
| **v0.1.0** (released) | GeoTIFF/COG, GeoJSON, Shapefile, PROJ, raster algorithms, offline sync |
| **v0.2.0** (Q2 2026) | HDF5, NetCDF, Zarr mobile bindings; write support expansion; 3D terrain |
| **v0.3.0** (Q3 2026) | Real-time GPS integration, streaming from cloud storage, background tile generation |

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](../../CONTRIBUTING.md).

## License

Licensed under Apache License 2.0. See [LICENSE](../../LICENSE) for details.

## Authors

Copyright (c) 2026 COOLJAPAN OU (Team Kitasan)

## Related Projects

- [OxiGDAL Core](../oxigdal-core) - Core Rust library
- [OxiGDAL WASM](../oxigdal-wasm) - WebAssembly bindings
- [OxiGDAL Python](../oxigdal-python) - Python bindings
- [OxiBLAS](https://github.com/cool-japan/oxiblas) - Pure Rust BLAS
- [NumRS2](https://github.com/cool-japan/numrs2) - NumPy for Rust

## Support

- Documentation: https://docs.rs/oxigdal-mobile
- Issues: https://github.com/cool-japan/oxigdal/issues
- Discussions: https://github.com/cool-japan/oxigdal/discussions
