//! Ed25519 session signer and seal construction.

use ed25519_dalek::{Signer, SigningKey};

use super::{
    ATTESTATION_FORMAT, ATTESTATION_VERSION, Attestation, AttestationError, AttestedOperation,
    SessionLog, params_digest, seal_bytes, to_hex,
};

/// Metadata bound into a session seal alongside the log commitment.
///
/// The `bytes_*` counters and `policy_json` are values the application observed
/// and reports; the seal binds them so they cannot be altered after signing.
/// See the [trust model](crate::attestation) for what that does and does not
/// prove.
#[derive(Debug, Clone)]
pub struct SealMetadata {
    /// Session start time in milliseconds (informational).
    pub started_at_ms: u64,
    /// Session end / seal time in milliseconds (informational).
    pub ended_at_ms: u64,
    /// Bytes the application observed leaving to external origins.
    pub bytes_egressed: u64,
    /// Bytes the application observed entering from same-origin fetches.
    pub bytes_ingressed: u64,
    /// Policy JSON the session ran under (hashed into the seal).
    pub policy_json: String,
    /// Human-readable application name recorded in the attestation.
    pub app_name: String,
    /// Application version string recorded in the attestation.
    pub app_version: String,
}

/// An Ed25519 signing key that seals [`SessionLog`]s into [`Attestation`]s.
pub struct SessionSigner(SigningKey);

impl SessionSigner {
    /// Generate a fresh signing key from operating-system entropy.
    ///
    /// Entropy comes from [`getrandom::fill`]; any RNG failure is surfaced as
    /// [`AttestationError::Rng`] rather than panicking.
    pub fn generate() -> Result<Self, AttestationError> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|err| AttestationError::Rng(err.to_string()))?;
        Ok(Self(SigningKey::from_bytes(&seed)))
    }

    /// Construct a signer deterministically from a 32-byte seed.
    ///
    /// Same seed ⇒ same key ⇒ reproducible signatures over identical seal
    /// bytes. Intended for tests and golden fixtures, not production keys.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(SigningKey::from_bytes(&seed))
    }

    /// The 32-byte Ed25519 public key.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.0.verifying_key().to_bytes()
    }

    /// Seal a log into a signed [`Attestation`].
    ///
    /// Commits the Merkle root and head hash, assembles the canonical seal
    /// bytes (see the [module docs](crate::attestation)), signs them, and
    /// returns the fully populated attestation ready for JSON serialization.
    pub fn seal(
        &self,
        log: &SessionLog,
        meta: &SealMetadata,
    ) -> Result<Attestation, AttestationError> {
        let session_id = log.session_id_bytes();
        let merkle_root = log.merkle_root();
        let head_hash = log.head_hash();
        let entry_count = log.entries().len() as u64;
        let policy_digest = params_digest(&meta.policy_json);

        let message = seal_bytes(
            ATTESTATION_VERSION,
            &session_id,
            meta.started_at_ms,
            meta.ended_at_ms,
            entry_count,
            &merkle_root,
            &head_hash,
            meta.bytes_egressed,
            meta.bytes_ingressed,
            &policy_digest,
        );
        let signature = self.0.sign(&message);

        let operations = log
            .entries()
            .iter()
            .map(|entry| AttestedOperation {
                seq: entry.seq,
                ts_ms: entry.ts_ms,
                op: entry.op.clone(),
                params: entry.params.clone(),
                prev_hash: to_hex(&entry.prev_hash),
                entry_hash: to_hex(&entry.entry_hash),
            })
            .collect();

        Ok(Attestation {
            format: ATTESTATION_FORMAT.to_string(),
            version: ATTESTATION_VERSION,
            session_id: to_hex(&session_id),
            app_name: meta.app_name.clone(),
            app_version: meta.app_version.clone(),
            started_at_ms: meta.started_at_ms,
            ended_at_ms: meta.ended_at_ms,
            policy: meta.policy_json.clone(),
            bytes_egressed: meta.bytes_egressed,
            bytes_ingressed: meta.bytes_ingressed,
            operations,
            merkle_root: to_hex(&merkle_root),
            head_hash: to_hex(&head_hash),
            public_key: to_hex(&self.public_key_bytes()),
            signature: to_hex(&signature.to_bytes()),
        })
    }
}
