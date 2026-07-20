//! Tamper-evident session attestation (blake3 hash chain → Merkle root →
//! Ed25519 seal).
//!
//! This module implements a small, self-contained, `wasm`-friendly attestation
//! format. It records an ordered log of operations, chains each entry to its
//! predecessor with a domain-separated blake3 hash, commits the whole log to a
//! domain-separated Merkle root, and seals a compact summary of the session
//! with an Ed25519 signature. Everything needed to independently re-verify an
//! [`Attestation`] is present in its JSON serialization — see
//! [`verify_attestation`].
//!
//! # Trust model (read this)
//!
//! An attestation is a *tamper-evident record*. It proves, cryptographically:
//!
//! 1. the operation log is **complete and unaltered** since it was sealed
//!    (hash chain + Merkle root), and
//! 2. it was sealed by the holder of the **session key** shown
//!    (Ed25519 signature over the session summary).
//!
//! It does **not** prove that no other software on the machine exfiltrated
//! data. The `bytes_egressed` / `bytes_ingressed` counters are values *observed
//! and reported by the application*; the attestation binds them so they cannot
//! be altered after sealing, but their truthfulness rests on the honesty of the
//! (inspectable) application code that produced them. Integrity of the record:
//! proven. No-egress: enforced and observed by the application, not
//! mathematically proven by this signature.
//!
//! # Canonical byte encoding (the format *is* the spec)
//!
//! Hashes and the signature bind **bytes**, never the JSON text. The JSON in
//! [`Attestation`] is a display / transport form; a verifier reconstructs the
//! canonical bytes below from the JSON and recomputes every hash.
//!
//! All multi-byte integers are **little-endian**. `‖` denotes concatenation.
//! `[n]` denotes a fixed `n`-byte field. Domain-separation tags are ASCII byte
//! strings ([`DOM_GENESIS`], [`DOM_ENTRY`], …):
//!
//! ```text
//! DOM_GENESIS = b"oxigeo.attest.v1.genesis"
//! DOM_ENTRY   = b"oxigeo.attest.v1.entry"
//! DOM_PARAMS  = b"oxigeo.attest.v1.params"
//! DOM_LEAF    = b"oxigeo.attest.v1.leaf"
//! DOM_NODE    = b"oxigeo.attest.v1.node"
//! DOM_SEAL    = b"oxigeo.attest.v1.seal"
//! DOM_EMPTY   = b"oxigeo.attest.v1.empty"
//!
//! params_digest = blake3(DOM_PARAMS ‖ params_json_utf8)
//!
//! entry_bytes   = seq:u64le ‖ ts_ms:u64le ‖ op_len:u32le ‖ op_utf8
//!                 ‖ params_digest[32] ‖ prev_hash[32]
//! entry_hash    = blake3(DOM_ENTRY ‖ entry_bytes)
//! prev_hash(0)  = blake3(DOM_GENESIS ‖ session_id[16])   // genesis head
//! prev_hash(i)  = entry_hash(i-1)   for i > 0
//!
//! leaf_i        = blake3(DOM_LEAF ‖ entry_hash_i)
//! node(l, r)    = blake3(DOM_NODE ‖ l[32] ‖ r[32])
//! // pairs combined left-to-right; an odd trailing node is carried up
//! // unchanged; the root of an empty log is:
//! empty_root    = blake3(DOM_EMPTY ‖ session_id[16])
//!
//! seal_bytes    = DOM_SEAL ‖ version:u32le ‖ session_id[16]
//!                 ‖ started_ms:u64le ‖ ended_ms:u64le ‖ entry_count:u64le
//!                 ‖ merkle_root[32] ‖ head_hash[32]
//!                 ‖ bytes_egressed:u64le ‖ bytes_ingressed:u64le
//!                 ‖ blake3(DOM_PARAMS ‖ policy_json)
//! signature     = Ed25519_sign(seal_bytes)
//! ```
//!
//! The Merkle root and head hash the signature covers are the *claimed* values
//! in the sealed summary, which lets a verifier check the three properties —
//! hash chain, Merkle commitment, and signature — **independently** (see
//! [`VerificationReport`]).
//!
//! # Example
//!
//! ```
//! use oxigeo_security::attestation::{SealMetadata, SessionLog, SessionSigner,
//!     verify_attestation};
//!
//! let mut log = SessionLog::new([7u8; 16]);
//! log.append(1_700_000_000_000, "file.open", r#"{"name":"dem.tif"}"#);
//! log.append(1_700_000_001_000, "terrain.hillshade", "{}");
//!
//! let signer = SessionSigner::from_seed([42u8; 32]);
//! let meta = SealMetadata {
//!     started_at_ms: 1_700_000_000_000,
//!     ended_at_ms: 1_700_000_002_000,
//!     bytes_egressed: 0,
//!     bytes_ingressed: 4096,
//!     policy_json: r#"{"csp":"default-src 'self'"}"#.to_string(),
//!     app_name: "geovault".to_string(),
//!     app_version: "0.1.7".to_string(),
//! };
//! let attestation = signer.seal(&log, &meta).expect("seal");
//! let json = serde_json::to_string(&attestation).expect("json");
//!
//! let report = verify_attestation(&json).expect("verify");
//! assert!(report.chain_ok && report.merkle_ok && report.signature_ok);
//! ```

mod chain;
mod merkle;
mod sign;
mod verify;

pub use chain::{LogEntry, SessionLog};
pub use merkle::{ProofStep, merkle_proof, merkle_root, verify_merkle_proof};
pub use sign::{SealMetadata, SessionSigner};
pub use verify::{VerificationReport, verify_attestation};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Domain-separation tag for the genesis head hash.
pub const DOM_GENESIS: &[u8] = b"oxigeo.attest.v1.genesis";
/// Domain-separation tag for a chained log entry hash.
pub const DOM_ENTRY: &[u8] = b"oxigeo.attest.v1.entry";
/// Domain-separation tag for an operation-parameters digest.
pub const DOM_PARAMS: &[u8] = b"oxigeo.attest.v1.params";
/// Domain-separation tag for a Merkle leaf.
pub const DOM_LEAF: &[u8] = b"oxigeo.attest.v1.leaf";
/// Domain-separation tag for a Merkle internal node.
pub const DOM_NODE: &[u8] = b"oxigeo.attest.v1.node";
/// Domain-separation tag for the sealed session summary.
pub const DOM_SEAL: &[u8] = b"oxigeo.attest.v1.seal";
/// Domain-separation tag for the Merkle root of an empty log.
pub const DOM_EMPTY: &[u8] = b"oxigeo.attest.v1.empty";

/// On-disk / on-wire format version encoded in every [`Attestation`] and seal.
pub const ATTESTATION_VERSION: u32 = 1;
/// Format identifier stored in [`Attestation::format`].
pub const ATTESTATION_FORMAT: &str = "oxigeo-attestation";

/// Errors produced by the attestation module.
#[derive(Debug, Error)]
pub enum AttestationError {
    /// The operating-system RNG failed while generating a signing key.
    #[error("random number generation failed: {0}")]
    Rng(String),
    /// The hash chain is inconsistent at the given sequence number.
    #[error("hash chain broken at sequence {seq}")]
    ChainBroken {
        /// The sequence number at which the chain first fails to verify.
        seq: u64,
    },
    /// A recomputed Merkle root does not match the claimed root.
    #[error("merkle root mismatch")]
    RootMismatch,
    /// An Ed25519 signature failed to verify.
    #[error("signature verification failed")]
    SignatureInvalid,
    /// The attestation JSON (or a hex field within it) is malformed.
    #[error("malformed attestation: {0}")]
    Malformed(String),
    /// A leaf index was out of range for the given Merkle tree.
    #[error("index {0} out of range")]
    IndexOutOfRange(usize),
}

/// A single operation as serialized inside an [`Attestation`].
///
/// All hash fields are lowercase hex (`prev_hash` / `entry_hash`: 32 bytes →
/// 64 hex chars). This is the display form of a [`LogEntry`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestedOperation {
    /// Zero-based position in the log.
    pub seq: u64,
    /// Caller-supplied timestamp in milliseconds (informational; ordering
    /// authority is `seq` + the hash chain, not the clock).
    pub ts_ms: u64,
    /// Operation name (opaque UTF-8, e.g. `"terrain.hillshade"`).
    pub op: String,
    /// Operation parameters as a JSON string (hashed as opaque UTF-8 bytes).
    pub params: String,
    /// Predecessor hash this entry chains onto (lowercase hex, 64 chars).
    pub prev_hash: String,
    /// This entry's chained hash (lowercase hex, 64 chars).
    pub entry_hash: String,
}

/// A sealed, signed session attestation.
///
/// Every field needed to independently re-verify the record is present; see
/// [`verify_attestation`]. Byte-level semantics are documented at the
/// [module level](crate::attestation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attestation {
    /// Always [`ATTESTATION_FORMAT`] (`"oxigeo-attestation"`).
    pub format: String,
    /// Format version ([`ATTESTATION_VERSION`]).
    pub version: u32,
    /// Session identifier (lowercase hex, 16 bytes → 32 chars).
    pub session_id: String,
    /// Human-readable application name.
    pub app_name: String,
    /// Application version string.
    pub app_version: String,
    /// Session start time in milliseconds (informational).
    pub started_at_ms: u64,
    /// Session end / seal time in milliseconds (informational).
    pub ended_at_ms: u64,
    /// Policy JSON the session ran under (bound by the signature).
    pub policy: String,
    /// Bytes the application observed leaving to external origins.
    pub bytes_egressed: u64,
    /// Bytes the application observed entering from same-origin fetches.
    pub bytes_ingressed: u64,
    /// The ordered operation log.
    pub operations: Vec<AttestedOperation>,
    /// Domain-separated Merkle root over the log (lowercase hex, 64 chars).
    pub merkle_root: String,
    /// Head of the hash chain (lowercase hex, 64 chars).
    pub head_hash: String,
    /// Ed25519 public key of the session (lowercase hex, 32 bytes → 64 chars).
    pub public_key: String,
    /// Ed25519 signature over the canonical seal bytes (lowercase hex, 64 bytes
    /// → 128 chars).
    pub signature: String,
}

// --- shared canonical-encoding primitives (crate-internal) ---

/// Hash a sequence of byte slices with blake3 (streaming; no allocation of the
/// concatenation).
pub(crate) fn hash_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(part);
    }
    *hasher.finalize().as_bytes()
}

/// Genesis head hash: `blake3(DOM_GENESIS ‖ session_id)`.
pub(crate) fn genesis_hash(session_id: &[u8; 16]) -> [u8; 32] {
    hash_parts(&[DOM_GENESIS, session_id])
}

/// Parameters digest: `blake3(DOM_PARAMS ‖ params_json_utf8)`.
pub(crate) fn params_digest(params_json: &str) -> [u8; 32] {
    hash_parts(&[DOM_PARAMS, params_json.as_bytes()])
}

/// Chained entry hash over the canonical `entry_bytes` layout.
pub(crate) fn compute_entry_hash(
    seq: u64,
    ts_ms: u64,
    op: &str,
    params_digest: &[u8; 32],
    prev_hash: &[u8; 32],
) -> [u8; 32] {
    let op_bytes = op.as_bytes();
    let mut buf = Vec::with_capacity(8 + 8 + 4 + op_bytes.len() + 32 + 32);
    buf.extend_from_slice(&seq.to_le_bytes());
    buf.extend_from_slice(&ts_ms.to_le_bytes());
    buf.extend_from_slice(&(op_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(op_bytes);
    buf.extend_from_slice(params_digest);
    buf.extend_from_slice(prev_hash);
    hash_parts(&[DOM_ENTRY, &buf])
}

/// Merkle leaf hash: `blake3(DOM_LEAF ‖ entry_hash)`.
pub(crate) fn leaf_hash(entry_hash: &[u8; 32]) -> [u8; 32] {
    hash_parts(&[DOM_LEAF, entry_hash])
}

/// Merkle node hash: `blake3(DOM_NODE ‖ left ‖ right)`.
pub(crate) fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    hash_parts(&[DOM_NODE, left, right])
}

/// Build the canonical seal bytes covered by the Ed25519 signature.
#[allow(clippy::too_many_arguments)]
pub(crate) fn seal_bytes(
    version: u32,
    session_id: &[u8; 16],
    started_ms: u64,
    ended_ms: u64,
    entry_count: u64,
    merkle_root: &[u8; 32],
    head_hash: &[u8; 32],
    bytes_egressed: u64,
    bytes_ingressed: u64,
    policy_digest: &[u8; 32],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(DOM_SEAL.len() + 4 + 16 + 8 * 5 + 32 * 3);
    buf.extend_from_slice(DOM_SEAL);
    buf.extend_from_slice(&version.to_le_bytes());
    buf.extend_from_slice(session_id);
    buf.extend_from_slice(&started_ms.to_le_bytes());
    buf.extend_from_slice(&ended_ms.to_le_bytes());
    buf.extend_from_slice(&entry_count.to_le_bytes());
    buf.extend_from_slice(merkle_root);
    buf.extend_from_slice(head_hash);
    buf.extend_from_slice(&bytes_egressed.to_le_bytes());
    buf.extend_from_slice(&bytes_ingressed.to_le_bytes());
    buf.extend_from_slice(policy_digest);
    buf
}

// --- lowercase hex helpers (no external dependency) ---

const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

/// Encode bytes as a lowercase hexadecimal string.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX_CHARS[(b >> 4) as usize] as char);
        out.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_nibble(c: u8) -> Result<u8, AttestationError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        other => Err(AttestationError::Malformed(format!(
            "invalid hex character: {:?}",
            other as char
        ))),
    }
}

/// Decode a lowercase (or uppercase) hex string into a fixed-size byte array.
pub(crate) fn from_hex<const N: usize>(s: &str) -> Result<[u8; N], AttestationError> {
    if s.len() != N * 2 {
        return Err(AttestationError::Malformed(format!(
            "expected {} hex characters, got {}",
            N * 2,
            s.len()
        )));
    }
    let mut out = [0u8; N];
    for (slot, chunk) in out.iter_mut().zip(s.as_bytes().chunks_exact(2)) {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        *slot = (hi << 4) | lo;
    }
    Ok(out)
}
