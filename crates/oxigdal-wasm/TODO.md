# TODO: oxigdal-wasm

## High Priority
- [ ] Implement actual HTTP range-request fetching for COG tiles via web_sys Fetch API
- [ ] Add SharedArrayBuffer support for true multi-threaded Web Worker processing
- [ ] Implement WebGPU compute pipeline for GPU-accelerated raster operations in browser
- [ ] Add IndexedDB-backed persistent tile cache replacing in-memory-only caching
- [ ] Implement wasm32-wasip2 Component Model compilation verification
- [ ] Add streaming decode for large GeoTIFF files exceeding browser memory limits

## Medium Priority
- [ ] Implement OffscreenCanvas rendering for Web Worker-based tile compositing
- [ ] Add WebCodecs integration for hardware-accelerated image decode
- [ ] Implement Service Worker tile prefetch strategy integration with oxigdal-pwa
- [ ] Add WebSocket support for real-time tile update notifications
- [ ] Implement SIMD-optimized pixel operations using wasm128 intrinsics
- [ ] Add memory growth strategy with configurable limits for mobile browsers
- [ ] Implement drag-and-drop file handling for local GeoTIFF/GeoJSON loading
- [ ] Add WebRTC data channel support for peer-to-peer tile sharing

## Low Priority / Future
- [ ] Implement WebXR integration for AR/VR geospatial visualization
- [ ] Add Comlink-style transparent proxy for ergonomic Worker communication
- [ ] Implement wasm-bindgen-futures integration for all async operations
- [ ] Add OPFS (Origin Private File System) support for large local datasets
- [ ] Implement progressive mesh loading for 3D terrain in browser
- [ ] Add Emscripten-free pure wasm-bindgen build path
- [ ] Implement WebTransport for HTTP/3-based tile streaming
