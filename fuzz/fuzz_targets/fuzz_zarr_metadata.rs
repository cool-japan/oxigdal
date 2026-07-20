//! Fuzz target: Zarr v2/v3 array-metadata JSON parsing and the built-in
//! chunk-codec decode pipeline.
//!
//! Exercises `.zarray`/`.zattrs` (v2) and `zarr.json` (v3) style metadata
//! deserialization plus the Gzip/Zstd chunk codecs, which is where a
//! malformed/malicious chunk (e.g. from an untrusted cloud object store)
//! could trigger a panic or out-of-bounds read in the decompressor.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigeo_zarr::codecs::Codec;
use oxigeo_zarr::codecs::gzip::GzipCodec;
use oxigeo_zarr::codecs::zstd_codec::ZstdCodec;
use oxigeo_zarr::metadata::v2::ArrayMetadataV2;
use oxigeo_zarr::metadata::v3::ArrayMetadataV3;

fuzz_target!(|data: &[u8]| {
    // Zarr v2 `.zarray` metadata JSON.
    let _ = serde_json::from_slice::<ArrayMetadataV2>(data);

    // Zarr v3 `zarr.json` metadata JSON.
    let _ = serde_json::from_slice::<ArrayMetadataV3>(data);

    // Generic JSON value, exercised for `.zattrs` free-form attribute maps.
    let _ = serde_json::from_slice::<serde_json::Value>(data);

    // Chunk codec decode pipeline: treat the fuzz input as a compressed
    // chunk payload straight off (untrusted) storage.
    if let Ok(codec) = GzipCodec::new(6) {
        let _ = codec.decode(data);
    }
    if let Ok(codec) = ZstdCodec::new(3) {
        let _ = codec.decode(data);
    }
});
