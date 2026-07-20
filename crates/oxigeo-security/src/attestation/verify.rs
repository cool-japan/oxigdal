//! Independent re-verification of a serialized [`Attestation`].

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use super::{
    Attestation, AttestationError, compute_entry_hash, from_hex, genesis_hash, leaf_hash,
    merkle_root, params_digest, seal_bytes,
};

/// Outcome of [`verify_attestation`], reporting each property independently.
///
/// The three `*_ok` flags are evaluated separately so a caller can tell *what*
/// failed:
///
/// - `chain_ok` — every entry's sequence, predecessor link, and recomputed hash
///   are consistent (from genesis to the claimed head).
/// - `merkle_ok` — the Merkle root recomputed from the entries' stored hashes
///   equals the claimed `merkle_root`.
/// - `signature_ok` — the Ed25519 signature verifies over the *claimed* sealed
///   summary under the stated public key.
///
/// Because the signature covers the claimed Merkle root and head hash, tampering
/// with those trips both `merkle_ok` and `signature_ok`; tampering with an
/// operation's `params` (leaving its stored `entry_hash`) trips only `chain_ok`;
/// tampering with the signature bytes trips only `signature_ok`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    /// Whether the hash chain re-verifies end to end.
    pub chain_ok: bool,
    /// Whether the recomputed Merkle root matches the claimed root.
    pub merkle_ok: bool,
    /// Whether the Ed25519 signature verifies over the sealed summary.
    pub signature_ok: bool,
    /// Number of operations in the log.
    pub entry_count: u64,
    /// Session identifier as carried in the attestation (lowercase hex).
    pub session_id: String,
    /// Bytes the application reported egressing (bound by the signature).
    pub bytes_egressed: u64,
    /// Session public key as carried in the attestation (lowercase hex).
    pub public_key: String,
}

/// Re-verify an attestation from its JSON alone.
///
/// Parses the JSON, rebuilds the hash chain, Merkle root, and canonical seal
/// bytes from scratch, and checks the signature — reporting each property
/// independently in a [`VerificationReport`]. Returns
/// [`AttestationError::Malformed`] only when the JSON or a hex field cannot be
/// parsed; cryptographic failures are reported as `false` flags, never errors.
pub fn verify_attestation(json: &str) -> Result<VerificationReport, AttestationError> {
    let attestation: Attestation =
        serde_json::from_str(json).map_err(|err| AttestationError::Malformed(err.to_string()))?;

    let session_id = from_hex::<16>(&attestation.session_id)?;

    // --- rebuild and check the hash chain ---
    let mut chain_ok = true;
    let mut prev = genesis_hash(&session_id);
    let mut leaves: Vec<[u8; 32]> = Vec::with_capacity(attestation.operations.len());
    for (index, op) in attestation.operations.iter().enumerate() {
        let stored_prev = from_hex::<32>(&op.prev_hash)?;
        let stored_entry = from_hex::<32>(&op.entry_hash)?;

        if op.seq != index as u64 {
            chain_ok = false;
        }
        if stored_prev != prev {
            chain_ok = false;
        }
        let digest = params_digest(&op.params);
        let recomputed = compute_entry_hash(op.seq, op.ts_ms, &op.op, &digest, &stored_prev);
        if recomputed != stored_entry {
            chain_ok = false;
        }

        // The Merkle commitment is over the *stored* entry hashes, so tampering
        // with a params byte (without also editing entry_hash) shows up as a
        // chain failure while leaving the Merkle root intact.
        leaves.push(leaf_hash(&stored_entry));
        prev = stored_entry;
    }

    let claimed_head = from_hex::<32>(&attestation.head_hash)?;
    let computed_head = if attestation.operations.is_empty() {
        genesis_hash(&session_id)
    } else {
        prev
    };
    if computed_head != claimed_head {
        chain_ok = false;
    }

    // --- recompute and check the Merkle root ---
    let claimed_root = from_hex::<32>(&attestation.merkle_root)?;
    let computed_root = merkle_root(&leaves, &session_id);
    let merkle_ok = computed_root == claimed_root;

    // --- rebuild the seal bytes and check the signature ---
    let public_key = from_hex::<32>(&attestation.public_key)?;
    let signature_bytes = from_hex::<64>(&attestation.signature)?;
    let policy_digest = params_digest(&attestation.policy);
    let entry_count = attestation.operations.len() as u64;
    let message = seal_bytes(
        attestation.version,
        &session_id,
        attestation.started_at_ms,
        attestation.ended_at_ms,
        entry_count,
        &claimed_root,
        &claimed_head,
        attestation.bytes_egressed,
        attestation.bytes_ingressed,
        &policy_digest,
    );
    let signature_ok = match VerifyingKey::from_bytes(&public_key) {
        Ok(verifying_key) => {
            let signature = Signature::from_bytes(&signature_bytes);
            verifying_key.verify(&message, &signature).is_ok()
        }
        Err(_) => false,
    };

    Ok(VerificationReport {
        chain_ok,
        merkle_ok,
        signature_ok,
        entry_count,
        session_id: attestation.session_id,
        bytes_egressed: attestation.bytes_egressed,
        public_key: attestation.public_key,
    })
}
