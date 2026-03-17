# TODO: oxigdal-mobile

## High Priority
- [ ] Add Swift Package Manager (SPM) distribution with XCFramework build script
- [ ] Implement Android AAR packaging with Gradle build integration
- [ ] Add streaming raster read via FFI for memory-constrained mobile devices
- [ ] Implement COG range-request reader accessible from FFI layer
- [ ] Add thread-safe error message queue replacing single last-error global state
- [ ] Implement zero-copy buffer sharing between Rust and platform (Metal/Vulkan)

## Medium Priority
- [ ] Add JNI bindings for Android (currently only C FFI, no Kotlin/Java layer)
- [ ] Implement offline tile cache accessible via FFI for map display
- [ ] Add vector tile (MVT) rendering support through FFI interface
- [ ] Implement progress callback for long-running FFI operations
- [ ] Add cancellation token support for async FFI operations
- [ ] Implement memory-mapped file access for large local raster datasets
- [ ] Add coordinate transformation FFI functions (proj integration)
- [ ] Implement GeoJSON read/write through FFI layer

## Low Priority / Future
- [ ] Add React Native bridge module
- [ ] Implement Flutter plugin via dart:ffi
- [ ] Add .NET MAUI / Xamarin bindings via P/Invoke
- [ ] Implement on-device ML inference integration (CoreML/NNAPI passthrough)
- [ ] Add ARKit/ARCore spatial anchor support for geo-registered AR
- [ ] Implement MapLibre/Mapbox integration layer for native map display
- [ ] Add automated cbindgen header generation in CI
- [ ] Implement fuzzing for all FFI entry points
