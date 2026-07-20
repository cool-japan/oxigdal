//! Integration tests for the real RustCrypto-backed SHA-256 and AES-256-GCM
//! storage transformers in OxiGeo Zarr.
//
// Tests legitimately call `.expect()` on known-good values; allow it here.
#![allow(clippy::expect_used)]

use oxigeo_zarr::transformers::{AesGcmTransformer, Sha256Transformer, Transformer};

// ── helpers ──────────────────────────────────────────────────────────────────

fn hex_to_bytes(s: &str) -> Vec<u8> {
    assert!(
        s.len().is_multiple_of(2),
        "hex string must have even length"
    );
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex digit"))
        .collect()
}

// ── SHA-256 FIPS 180-4 known-answer tests ────────────────────────────────────

/// FIPS 180-4 KAT: SHA-256("") =
///   e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
///
/// When `append = true`, encode() returns `data || hash`, so the first
/// (and in this case only) 32 bytes of the result are the hash itself.
#[test]
fn test_sha256_empty_input_matches_fips_kat() {
    let t = Sha256Transformer::new(true);
    let encoded = t.encode(&[]).expect("encode empty");
    // append=true, data is empty → entire output is the 32-byte hash
    assert_eq!(encoded.len(), 32);
    let expected = hex_to_bytes("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(
        encoded, expected,
        "SHA-256 of empty string must match FIPS 180-4 KAT"
    );
}

/// SHA-256("abc") known-answer test.
///
/// Reference value verified with both `openssl dgst -sha256` and `sha256sum`:
///   ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
#[test]
fn test_sha256_abc_matches_fips_kat() {
    // Use append=true so the hash sits at the end of the encoded buffer.
    let t = Sha256Transformer::new(true);
    let data = b"abc";
    let encoded = t.encode(data).expect("encode abc");
    assert_eq!(encoded.len(), data.len() + 32);
    let hash_slice = &encoded[data.len()..]; // last 32 bytes
    let expected = hex_to_bytes("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(
        hash_slice,
        expected.as_slice(),
        "SHA-256 of 'abc' must match known KAT"
    );
}

/// encode() must append exactly 32 bytes to any input.
#[test]
fn test_sha256_encode_appends_32_bytes() {
    for &len in &[0usize, 1, 16, 64, 1024] {
        let data: Vec<u8> = (0..len).map(|i| (i & 0xFF) as u8).collect();
        let t = Sha256Transformer::new(true);
        let encoded = t.encode(&data).expect("encode");
        assert_eq!(
            encoded.len(),
            data.len() + 32,
            "encoded length must be input + 32 bytes for input len {len}"
        );
    }
}

/// Corrupting any byte inside the appended hash region must cause decode() to
/// return `Err` containing "SHA256 hash mismatch".
#[test]
fn test_sha256_decode_detects_corruption() {
    let t = Sha256Transformer::new(true);
    let data = b"important payload";
    let mut encoded = t.encode(data).expect("encode");

    // Flip a bit in the hash region (last 32 bytes)
    let hash_start = encoded.len() - 32;
    encoded[hash_start] ^= 0x01;

    let err = t
        .decode(&encoded)
        .expect_err("should error on corrupted hash");
    let msg = format!("{err}");
    assert!(
        msg.contains("SHA256 hash mismatch"),
        "error message should mention SHA256 hash mismatch, got: {msg}"
    );
}

/// With `append = false` the hash is prepended; with `append = true` it is
/// appended.  Both modes must round-trip correctly.
#[test]
fn test_sha256_append_vs_prepend_modes() {
    let data = b"mode test payload";

    // append=true: encoded = data || hash
    let t_append = Sha256Transformer::new(true);
    let enc_append = t_append.encode(data).expect("encode append");
    assert_eq!(
        &enc_append[..data.len()],
        data.as_ref(),
        "data must be at front in append mode"
    );
    let dec_append = t_append.decode(&enc_append).expect("decode append");
    assert_eq!(dec_append, data.as_ref());

    // append=false: encoded = hash || data
    let t_prepend = Sha256Transformer::new(false);
    let enc_prepend = t_prepend.encode(data).expect("encode prepend");
    assert_eq!(
        &enc_prepend[32..],
        data.as_ref(),
        "data must be at end in prepend mode"
    );
    let dec_prepend = t_prepend.decode(&enc_prepend).expect("decode prepend");
    assert_eq!(dec_prepend, data.as_ref());

    // The embedded hash bytes must be identical in both modes (same input data).
    let hash_from_append = &enc_append[data.len()..];
    let hash_from_prepend = &enc_prepend[..32];
    assert_eq!(
        hash_from_append, hash_from_prepend,
        "hash must be the same regardless of position"
    );
}

// ── AES-256-GCM tests ─────────────────────────────────────────────────────────

fn make_aes_transformer(key_byte: u8) -> AesGcmTransformer {
    let key = vec![key_byte; 32];
    AesGcmTransformer::new(key, format!("key-{key_byte:02x}")).expect("create transformer")
}

/// Basic round-trip with a short payload.
#[test]
fn test_aesgcm_round_trip_short_payload() {
    let t = make_aes_transformer(0x42);
    let data = b"hello world";
    let enc = t.encode(data).expect("encrypt");
    let dec = t.decode(&enc).expect("decrypt");
    assert_eq!(dec.as_slice(), data.as_ref());
}

/// Round-trip with a 1 MiB payload to exercise streaming-like behaviour.
#[test]
fn test_aesgcm_round_trip_1mb_payload() {
    let t = make_aes_transformer(0xAB);
    let data: Vec<u8> = (0..1_048_576).map(|i: usize| (i & 0xFF) as u8).collect();
    let enc = t.encode(&data).expect("encrypt 1 MiB");
    let dec = t.decode(&enc).expect("decrypt 1 MiB");
    assert_eq!(dec, data);
}

/// Output length must equal input length + 12 (nonce) + 16 (GHASH tag).
#[test]
fn test_aesgcm_nonce_prepended_length_is_input_plus_28() {
    let t = make_aes_transformer(0x01);
    for &input_len in &[0usize, 1, 15, 16, 17, 255, 1024] {
        let data: Vec<u8> = (0..input_len).map(|i| (i & 0xFF) as u8).collect();
        let enc = t.encode(&data).expect("encrypt");
        assert_eq!(
            enc.len(),
            input_len + 28,
            "ciphertext length must be input ({input_len}) + 12 (nonce) + 16 (tag)"
        );
    }
}

/// Two encryptions of the same plaintext must produce different ciphertexts
/// because AES-256-GCM generates a fresh random nonce per call.
#[test]
fn test_aesgcm_two_encrypts_differ_due_to_nonce() {
    let t = make_aes_transformer(0x77);
    let data = b"determinism check";
    let enc1 = t.encode(data).expect("encrypt 1");
    let enc2 = t.encode(data).expect("encrypt 2");
    assert_ne!(
        enc1, enc2,
        "two encryptions of the same data must differ (fresh nonce per call)"
    );
}

/// Decrypting with the wrong key must fail authentication.
#[test]
fn test_aesgcm_wrong_key_rejected() {
    let t_a = make_aes_transformer(0xAA);
    let t_b = make_aes_transformer(0xBB);
    let data = b"sensitive payload";
    let enc = t_a.encode(data).expect("encrypt with key A");
    let result = t_b.decode(&enc);
    assert!(result.is_err(), "decrypting with a different key must fail");
}

/// Flipping any byte in the ciphertext+tag region must trigger authentication
/// failure (AEAD integrity check).
#[test]
fn test_aesgcm_tampered_ciphertext_rejected() {
    let t = make_aes_transformer(0x55);
    let data = b"tamper-evident data";
    let mut enc = t.encode(data).expect("encrypt");

    // The first 12 bytes are the nonce; tamper with the first byte of the
    // ciphertext (byte 12).
    enc[12] ^= 0xFF;

    let result = t.decode(&enc);
    assert!(
        result.is_err(),
        "tampered ciphertext must not decrypt successfully"
    );
}

/// Passing a buffer shorter than 12 bytes (no room for a nonce) must return
/// an error immediately without attempting decryption.
#[test]
fn test_aesgcm_truncated_input_below_nonce_errors() {
    let t = make_aes_transformer(0x33);
    for short_len in 0usize..12 {
        let short_buf: Vec<u8> = vec![0u8; short_len];
        let result = t.decode(&short_buf);
        assert!(
            result.is_err(),
            "decode of {short_len}-byte input (< 12) must fail"
        );
    }
}
