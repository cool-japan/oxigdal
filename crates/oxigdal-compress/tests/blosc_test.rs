//! Integration tests for the Blosc-style meta-compressor.

use oxigdal_compress::codecs::{
    BloscBackend, BloscCodec, BloscFrame, ShuffleKind,
    blosc::{BLOSC_HEADER_LEN, BLOSC_VERSION, BLOSC_VERSIONLZ, Codec, compress, decompress},
    byte_shuffle, byte_unshuffle,
};

/// Build a deterministic `f32`-as-bytes payload of `count` elements.
fn f32_payload(count: usize) -> Vec<u8> {
    (0..count).flat_map(|i| (i as f32).to_le_bytes()).collect()
}

/// Build a deterministic `u16`-as-bytes payload of `count` elements.
fn u16_payload(count: usize) -> Vec<u8> {
    (0..count).flat_map(|i| (i as u16).to_le_bytes()).collect()
}

#[test]
fn test_byte_shuffle_f32_array_round_trip() {
    let original = f32_payload(1_024);
    let shuffled = byte_shuffle(&original, 4);
    assert_eq!(shuffled.len(), original.len());
    assert_ne!(shuffled, original);
    let restored = byte_unshuffle(&shuffled, 4);
    assert_eq!(restored, original);
}

#[test]
fn test_byte_shuffle_u16_array_round_trip() {
    let original = u16_payload(2_048);
    let shuffled = byte_shuffle(&original, 2);
    assert_eq!(shuffled.len(), original.len());
    let restored = byte_unshuffle(&shuffled, 2);
    assert_eq!(restored, original);
}

#[test]
fn test_byte_shuffle_typesize_1_is_identity() {
    let original: Vec<u8> = (0..256u32).map(|i| (i & 0xff) as u8).collect();
    let shuffled = byte_shuffle(&original, 1);
    assert_eq!(shuffled, original);
    let restored = byte_unshuffle(&original, 1);
    assert_eq!(restored, original);
}

#[test]
fn test_byte_shuffle_uneven_tail_preserved() {
    // 17 bytes with typesize 4 => 4 complete elements (16 bytes) + 1 trailing.
    let original: Vec<u8> = (0..17u8).collect();
    let shuffled = byte_shuffle(&original, 4);
    assert_eq!(shuffled.len(), original.len());
    // The final tail byte must be preserved at the end after shuffling.
    assert_eq!(*shuffled.last().expect("non-empty"), 16);

    let restored = byte_unshuffle(&shuffled, 4);
    assert_eq!(restored, original);
}

#[test]
fn test_blosc_frame_header_layout_16_bytes() {
    let options = BloscCodec {
        typesize: 4,
        blocksize: 1 << 14,
        backend: BloscBackend::Lz4,
        shuffle: ShuffleKind::ByteShuffle,
        clevel: 3,
    };
    let payload = f32_payload(4_096);
    let blob = compress(&payload, &options).expect("compress ok");
    assert!(blob.len() >= BLOSC_HEADER_LEN);

    let header = BloscFrame::parse(&blob[..BLOSC_HEADER_LEN]).expect("parse header");
    // The 16-byte boundary is sacred for the c-blosc2 wire format.
    assert_eq!(BLOSC_HEADER_LEN, 16);
    // nbytes / blocksize fields must round-trip.
    assert_eq!(header.nbytes as usize, payload.len());
    assert_eq!(header.blocksize, options.blocksize);
    assert_eq!(header.typesize, options.typesize);
}

#[test]
fn test_blosc_frame_header_version_2_versionlz_1() {
    let payload = f32_payload(128);
    let blob = compress(&payload, &BloscCodec::default()).expect("compress ok");
    assert_eq!(blob[0], BLOSC_VERSION);
    assert_eq!(blob[1], BLOSC_VERSIONLZ);
    assert_eq!(BLOSC_VERSION, 0x02);
    assert_eq!(BLOSC_VERSIONLZ, 0x01);
}

#[test]
fn test_blosc_compress_round_trip_f32_array_lz4() {
    let payload = f32_payload(8_192);
    let options = BloscCodec {
        typesize: 4,
        blocksize: 1 << 12,
        backend: BloscBackend::Lz4,
        shuffle: ShuffleKind::ByteShuffle,
        clevel: 4,
    };
    let blob = compress(&payload, &options).expect("compress ok");
    let restored = decompress(&blob).expect("decompress ok");
    assert_eq!(restored, payload);
}

#[test]
fn test_blosc_compress_round_trip_f32_array_zstd() {
    let payload = f32_payload(8_192);
    let options = BloscCodec {
        typesize: 4,
        blocksize: 1 << 12,
        backend: BloscBackend::Zstd,
        shuffle: ShuffleKind::ByteShuffle,
        clevel: 5,
    };
    let blob = compress(&payload, &options).expect("compress ok");
    let restored = decompress(&blob).expect("decompress ok");
    assert_eq!(restored, payload);
}

#[test]
fn test_blosc_compress_round_trip_u16_array_snappy() {
    let payload = u16_payload(8_192);
    let options = BloscCodec {
        typesize: 2,
        blocksize: 1 << 12,
        backend: BloscBackend::Snappy,
        shuffle: ShuffleKind::ByteShuffle,
        clevel: 5,
    };
    let blob = compress(&payload, &options).expect("compress ok");
    let restored = decompress(&blob).expect("decompress ok");
    assert_eq!(restored, payload);
}

#[test]
fn test_blosc_compress_no_shuffle_round_trip() {
    let payload = f32_payload(4_096);
    let options = BloscCodec {
        typesize: 4,
        blocksize: 1 << 12,
        backend: BloscBackend::Lz4,
        shuffle: ShuffleKind::None,
        clevel: 4,
    };
    let blob = compress(&payload, &options).expect("compress ok");
    let restored = decompress(&blob).expect("decompress ok");
    assert_eq!(restored, payload);
}

#[test]
fn test_blosc_compress_byte_shuffle_round_trip() {
    let payload = f32_payload(4_096);
    let options = BloscCodec {
        typesize: 4,
        blocksize: 1 << 12,
        backend: BloscBackend::Lz4,
        shuffle: ShuffleKind::ByteShuffle,
        clevel: 4,
    };
    let blob = compress(&payload, &options).expect("compress ok");
    let restored = decompress(&blob).expect("decompress ok");
    assert_eq!(restored, payload);
}

#[test]
fn test_blosc_decompress_truncated_header_returns_error() {
    // Less than 16 bytes -> parse must fail.
    let truncated = [0u8; 10];
    let err = decompress(&truncated).expect_err("must error on truncated header");
    let msg = format!("{}", err);
    assert!(
        msg.to_lowercase().contains("truncated") || msg.to_lowercase().contains("header"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn test_blosc_decompress_invalid_version_returns_error() {
    let payload = f32_payload(128);
    let mut blob = compress(&payload, &BloscCodec::default()).expect("compress ok");
    blob[0] = 0xff; // Corrupt the version byte.
    let err = decompress(&blob).expect_err("must error on bad version");
    let msg = format!("{}", err);
    assert!(
        msg.to_lowercase().contains("version") || msg.to_lowercase().contains("integrity"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn test_blosc_decompress_block_count_mismatch_returns_error() {
    let payload = f32_payload(8_192);
    let options = BloscCodec {
        typesize: 4,
        blocksize: 1 << 12,
        backend: BloscBackend::Lz4,
        shuffle: ShuffleKind::ByteShuffle,
        clevel: 3,
    };
    let mut blob = compress(&payload, &options).expect("compress ok");

    // Locate and corrupt the block-count u32 (right after the 16-byte header
    // and the 1-byte filter pipeline id).
    let block_count_offset = BLOSC_HEADER_LEN + 1;
    assert!(blob.len() >= block_count_offset + 4);
    let bogus = 0xdead_beefu32.to_le_bytes();
    blob[block_count_offset..block_count_offset + 4].copy_from_slice(&bogus);

    let err = decompress(&blob).expect_err("must error on block-count mismatch");
    let msg = format!("{}", err).to_lowercase();
    assert!(
        msg.contains("block") || msg.contains("mismatch") || msg.contains("integrity"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn test_blosc_codec_trait_impl_matches_oneshot() {
    let payload = f32_payload(2_048);
    let codec = BloscCodec {
        typesize: 4,
        blocksize: 1 << 12,
        backend: BloscBackend::Zstd,
        shuffle: ShuffleKind::ByteShuffle,
        clevel: 5,
    };

    let one_shot = compress(&payload, &codec).expect("oneshot compress");
    let via_trait = <BloscCodec as Codec>::compress(&codec, &payload).expect("trait compress");
    assert_eq!(one_shot.len(), via_trait.len());

    let restored = <BloscCodec as Codec>::decompress(&codec, &via_trait).expect("trait decompress");
    assert_eq!(restored, payload);
}

#[test]
fn test_blosc_compress_smaller_than_raw_for_repetitive_input() {
    // 64 KiB of zeros must compress massively.
    let payload = vec![0u8; 64 * 1024];
    let options = BloscCodec {
        typesize: 4,
        blocksize: 1 << 14,
        backend: BloscBackend::Zstd,
        shuffle: ShuffleKind::ByteShuffle,
        clevel: 5,
    };
    let blob = compress(&payload, &options).expect("compress ok");
    assert!(
        blob.len() < payload.len(),
        "expected compressed ({}) < raw ({})",
        blob.len(),
        payload.len()
    );
    let restored = decompress(&blob).expect("decompress ok");
    assert_eq!(restored, payload);
}
