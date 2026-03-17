# TODO: oxigdal-compress

## High Priority
- [ ] Implement ZFP lossy compression with configurable error bounds (rate/precision/accuracy)
- [ ] Add SZ-style compression with point-wise relative error bounds
- [ ] Implement Blosc meta-compressor (shuffle + codec selection)
- [ ] Add streaming compression/decompression API (process chunks without full buffer)
- [ ] Implement codec chaining (e.g., shuffle -> delta -> zstd pipeline)
- [ ] Add compression ratio and throughput metrics collection per operation
- [ ] Implement byte-shuffle and bit-shuffle filters as pre-compression transforms

## Medium Priority
- [ ] Add adaptive codec selection using sample-based profiling (compress small sample first)
- [ ] Implement LZMA/XZ codec for maximum compression ratio
- [ ] Add LZ4HC (high-compression) mode alongside standard LZ4
- [ ] Implement Brotli quality auto-tuning based on data characteristics
- [ ] Add compression metadata embedding (codec ID, parameters, original size)
- [ ] Implement frame format for all codecs (header with size, checksum, codec info)
- [ ] Add dictionary training for domain-specific dictionary compression
- [ ] Implement parallel decompression for multi-chunk data

## Low Priority / Future
- [ ] Add FPZIP compression for structured floating-point grids
- [ ] Implement quantization-based compression (reduce precision before lossless)
- [ ] Add AEC/CCSDS compression (used in meteorological data)
- [ ] Implement compression benchmark harness with configurable test data generators
- [ ] Add WASM-compatible codec subset (no_std + alloc)
- [ ] Implement bit-packing codec for integer data with limited range
- [ ] Add checksumming integration (CRC32, XXHash) for integrity verification
- [ ] Implement compression-aware memory allocator (pre-allocate output buffers)
