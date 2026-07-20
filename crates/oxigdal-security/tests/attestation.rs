//! Integration tests for the tamper-evident session attestation module.
//!
//! Run with: `cargo nextest run -p oxigdal-security --features attestation`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigdal_security::attestation::{
    Attestation, SealMetadata, SessionLog, SessionSigner, merkle_proof, merkle_root,
    verify_attestation, verify_merkle_proof,
};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/attestation_v1.json"
);

fn sample_meta() -> SealMetadata {
    SealMetadata {
        started_at_ms: 1_700_000_000_000,
        ended_at_ms: 1_700_000_002_000,
        bytes_egressed: 0,
        bytes_ingressed: 1_052_672,
        policy_json: r#"{"csp":"default-src 'self'","enforcement":["csp-meta","fetch-hook"]}"#
            .to_string(),
        app_name: "geovault".to_string(),
        app_version: "0.1.7".to_string(),
    }
}

fn sample_log() -> SessionLog {
    let mut log = SessionLog::new([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    log.append(
        1_700_000_000_000,
        "session.start",
        r#"{"policy":"default-src 'self'"}"#,
    );
    log.append(
        1_700_000_000_500,
        "file.open",
        r#"{"name":"site-k7.tif","size":1048576}"#,
    );
    log.append(
        1_700_000_001_000,
        "terrain.hillshade",
        r#"{"azimuth":315,"altitude":45}"#,
    );
    log.append(
        1_700_000_001_500,
        "anomaly.detect",
        r#"{"method":"modified_zscore","threshold":3.5,"count":642}"#,
    );
    log
}

fn golden_attestation() -> Attestation {
    let signer = SessionSigner::from_seed([42u8; 32]);
    signer
        .seal(&sample_log(), &sample_meta())
        .expect("seal golden attestation")
}

fn flip_first_hex(s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    if let Some(c) = chars.get_mut(0) {
        *c = if *c == '0' { '1' } else { '0' };
    }
    chars.into_iter().collect()
}

fn reverify(att: &Attestation) -> oxigdal_security::attestation::VerificationReport {
    let json = serde_json::to_string(att).expect("serialize");
    verify_attestation(&json).expect("verify")
}

#[test]
fn sign_then_verify_round_trip() {
    let att = golden_attestation();
    let report = reverify(&att);
    assert!(report.chain_ok, "chain must verify");
    assert!(report.merkle_ok, "merkle must verify");
    assert!(report.signature_ok, "signature must verify");
    assert_eq!(report.entry_count, 4);
    assert_eq!(report.bytes_egressed, 0);
}

#[test]
fn session_log_verify_chain_ok() {
    let log = sample_log();
    assert!(log.verify_chain().is_ok());
    assert_eq!(log.entries().len(), 4);
    // Head hash equals the last entry's chained hash.
    assert_eq!(log.head_hash(), log.entries()[3].entry_hash);
}

#[test]
fn empty_log_seal_verifies() {
    let log = SessionLog::new([9u8; 16]);
    // Empty-log head is the genesis hash and differs from a populated head.
    assert!(log.verify_chain().is_ok());
    let signer = SessionSigner::from_seed([7u8; 32]);
    let att = signer.seal(&log, &sample_meta()).expect("seal empty");
    assert_eq!(att.operations.len(), 0);
    let report = reverify(&att);
    assert!(report.chain_ok && report.merkle_ok && report.signature_ok);
    assert_eq!(report.entry_count, 0);
}

#[test]
fn json_serde_round_trip_reverifies() {
    let att = golden_attestation();
    let json = serde_json::to_string_pretty(&att).expect("serialize");
    let decoded: Attestation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(att, decoded);
    let report = verify_attestation(&json).expect("verify");
    assert!(report.chain_ok && report.merkle_ok && report.signature_ok);
}

#[test]
fn tamper_params_breaks_only_chain() {
    let mut att = golden_attestation();
    // Alter an operation's params without touching its stored entry_hash.
    att.operations[2].params.push('X');
    let report = reverify(&att);
    assert!(!report.chain_ok, "params tamper must break the chain");
    assert!(report.merkle_ok, "merkle uses stored hashes, stays intact");
    assert!(
        report.signature_ok,
        "signature covers claimed root/head, unaffected"
    );
}

#[test]
fn tamper_at_middle_operation_detected() {
    // Tampering operation k invalidates the record; earlier operations still
    // rehash cleanly, so the break is localized to k onward.
    let mut att = golden_attestation();
    att.operations[1].op = "file.open.evil".to_string();
    let report = reverify(&att);
    assert!(!report.chain_ok);
}

#[test]
fn tamper_signature_breaks_only_signature() {
    let mut att = golden_attestation();
    att.signature = flip_first_hex(&att.signature);
    let report = reverify(&att);
    assert!(report.chain_ok, "chain unaffected");
    assert!(report.merkle_ok, "merkle unaffected");
    assert!(!report.signature_ok, "signature tamper must be detected");
}

#[test]
fn tamper_public_key_breaks_signature() {
    let mut att = golden_attestation();
    att.public_key = flip_first_hex(&att.public_key);
    let report = reverify(&att);
    assert!(!report.signature_ok);
}

#[test]
fn tamper_merkle_root_breaks_merkle_and_signature() {
    let mut att = golden_attestation();
    att.merkle_root = flip_first_hex(&att.merkle_root);
    let report = reverify(&att);
    assert!(report.chain_ok, "chain independent of the claimed root");
    assert!(!report.merkle_ok, "recomputed root no longer matches");
    assert!(
        !report.signature_ok,
        "seal binds the root, so signature also fails"
    );
}

#[test]
fn tamper_bytes_egressed_breaks_signature() {
    let mut att = golden_attestation();
    att.bytes_egressed = 999_999;
    let report = reverify(&att);
    assert!(!report.signature_ok, "egress counter is sealed");
    assert_eq!(report.bytes_egressed, 999_999);
}

#[test]
fn malformed_json_is_error_not_panic() {
    assert!(verify_attestation("{ not valid").is_err());
    assert!(verify_attestation("{}").is_err());
    // Well-formed JSON but a hex field of the wrong length.
    let mut att = golden_attestation();
    att.session_id = "abcd".to_string();
    let json = serde_json::to_string(&att).expect("serialize");
    assert!(verify_attestation(&json).is_err());
}

#[test]
fn from_seed_is_deterministic() {
    let a = SessionSigner::from_seed([42u8; 32]);
    let b = SessionSigner::from_seed([42u8; 32]);
    assert_eq!(a.public_key_bytes(), b.public_key_bytes());
    let att_a = a.seal(&sample_log(), &sample_meta()).expect("seal a");
    let att_b = b.seal(&sample_log(), &sample_meta()).expect("seal b");
    assert_eq!(att_a, att_b, "same seed + same log ⇒ identical attestation");

    let c = SessionSigner::from_seed([43u8; 32]);
    assert_ne!(a.public_key_bytes(), c.public_key_bytes());
}

#[test]
fn generate_produces_verifiable_attestation() {
    let signer = SessionSigner::generate().expect("generate signing key");
    let att = signer.seal(&sample_log(), &sample_meta()).expect("seal");
    let report = reverify(&att);
    assert!(report.chain_ok && report.merkle_ok && report.signature_ok);
}

// --- Merkle tree / inclusion proofs (synthetic leaves) ---

fn leaf(n: u8) -> [u8; 32] {
    [n; 32]
}

#[test]
fn merkle_proof_verifies_per_leaf() {
    let session_id = [3u8; 16];
    for count in 1..=9usize {
        let leaves: Vec<[u8; 32]> = (0..count as u8).map(leaf).collect();
        let root = merkle_root(&leaves, &session_id);
        for (i, leaf_hash) in leaves.iter().enumerate() {
            let proof = merkle_proof(&leaves, i).expect("proof");
            assert!(
                verify_merkle_proof(leaf_hash, &proof, &root),
                "leaf {i} of {count} must verify"
            );
        }
    }
}

#[test]
fn merkle_proof_cross_index_fails() {
    let session_id = [3u8; 16];
    let leaves: Vec<[u8; 32]> = (0..6u8).map(leaf).collect();
    let root = merkle_root(&leaves, &session_id);
    let proof0 = merkle_proof(&leaves, 0).expect("proof 0");
    // A proof for index 0 must not validate a different leaf.
    assert!(!verify_merkle_proof(&leaves[1], &proof0, &root));
    // Nor should it validate against a bogus root.
    assert!(!verify_merkle_proof(&leaves[0], &proof0, &[0xFFu8; 32]));
}

#[test]
fn merkle_single_leaf_root_is_leaf() {
    let session_id = [3u8; 16];
    let leaves = vec![leaf(7)];
    let root = merkle_root(&leaves, &session_id);
    assert_eq!(root, leaves[0], "single-leaf root equals the leaf");
    let proof = merkle_proof(&leaves, 0).expect("proof");
    assert!(proof.is_empty(), "single leaf has no siblings");
    assert!(verify_merkle_proof(&leaves[0], &proof, &root));
}

#[test]
fn merkle_empty_root_is_session_bound() {
    let root_a = merkle_root(&[], &[1u8; 16]);
    let root_b = merkle_root(&[], &[2u8; 16]);
    assert_ne!(root_a, root_b, "empty root binds the session id");
    // Deterministic.
    assert_eq!(root_a, merkle_root(&[], &[1u8; 16]));
}

#[test]
fn merkle_proof_index_out_of_range() {
    let leaves: Vec<[u8; 32]> = (0..3u8).map(leaf).collect();
    assert!(merkle_proof(&leaves, 3).is_err());
    assert!(merkle_proof(&[], 0).is_err());
}

// --- Golden fixture (determinism regression) ---

#[test]
fn golden_fixture_matches_and_verifies() {
    let att = golden_attestation();
    let json = serde_json::to_string_pretty(&att).expect("serialize");

    if std::env::var("REGENERATE_FIXTURES").is_ok() {
        std::fs::write(FIXTURE_PATH, format!("{json}\n")).expect("write fixture");
    }

    let expected = std::fs::read_to_string(FIXTURE_PATH).expect("read fixture");
    assert_eq!(
        json.trim(),
        expected.trim(),
        "golden attestation drifted; regenerate with REGENERATE_FIXTURES=1 if intentional"
    );

    let report = verify_attestation(&expected).expect("verify fixture");
    assert!(report.chain_ok && report.merkle_ok && report.signature_ok);
}
