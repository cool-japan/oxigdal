# TODO: oxigdal-jpeg2000

## High Priority
- [ ] Complete EBCOT tier-1 decoder implementation (currently placeholder)
- [ ] Implement full decode_rgb() returning actual pixel data
- [ ] Add JPEG2000 encoding/writing support (JP2/J2K codestream generation)
- [ ] Implement SIMD-optimized wavelet transforms (5/3 and 9/7)
- [ ] Add parallel tile decoding using rayon
- [ ] Implement rate-distortion optimization for encoder quality control
- [ ] Add GeoJP2 metadata box reading/writing for georeferenced images

## Medium Priority
- [ ] Implement GMLJP2 (GML in JPEG2000) metadata support
- [ ] Add memory-efficient decoding for large images (tile-at-a-time streaming)
- [ ] Implement multi-resolution extraction without full decode
- [ ] Add JPX (JPEG2000 Part 2) extended features (compositing, animation)
- [ ] Implement MQD (arithmetic) decoder optimization for throughput
- [ ] Add support for non-standard color spaces (CIE Lab, palette)
- [ ] Implement code-block grouping for better cache utilization
- [ ] Add JPIP (JPEG2000 Interactive Protocol) client for remote access

## Low Priority / Future
- [ ] Implement HTJ2K (High-Throughput JPEG2000) decoder for faster decoding
- [ ] Add JPEG2000 Part 15 (HTJ2K) encoding support
- [ ] Implement GPU-accelerated wavelet transforms (compute shader)
- [ ] Add lossless-to-lossy transcoding without full decode/encode cycle
- [ ] Implement JPEG2000 file repair for corrupted codestreams
- [ ] Add benchmark suite against OpenJPEG and Kakadu reference decoders
- [ ] Implement code-block caching for interactive zoom applications
- [ ] Add ICC profile handling for color-managed workflows
