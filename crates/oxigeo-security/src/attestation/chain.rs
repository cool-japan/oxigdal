//! Append-only session log with a domain-separated blake3 hash chain.

use super::{AttestationError, compute_entry_hash, genesis_hash, leaf_hash, params_digest};

/// One entry in a [`SessionLog`], with its chained hashes materialized.
///
/// `prev_hash` and `entry_hash` are the raw 32-byte blake3 outputs described at
/// the [module level](crate::attestation); their hex forms appear in
/// [`AttestedOperation`](super::AttestedOperation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Zero-based position in the log.
    pub seq: u64,
    /// Caller-supplied timestamp in milliseconds (informational).
    pub ts_ms: u64,
    /// Operation name (opaque UTF-8).
    pub op: String,
    /// Operation parameters as a JSON string (hashed as opaque UTF-8 bytes).
    pub params: String,
    /// Predecessor hash this entry chains onto.
    pub prev_hash: [u8; 32],
    /// This entry's chained hash.
    pub entry_hash: [u8; 32],
}

/// An append-only, tamper-evident log of session operations.
///
/// Each [`append`](SessionLog::append) links the new entry to the current
/// [`head_hash`](SessionLog::head_hash) via a domain-separated blake3 hash, so
/// any later edit to a recorded entry invalidates every hash after it.
#[derive(Debug, Clone)]
pub struct SessionLog {
    session_id: [u8; 16],
    entries: Vec<LogEntry>,
}

impl SessionLog {
    /// Create an empty log bound to the given 16-byte session identifier.
    ///
    /// The session id seeds the genesis head hash and the empty-log Merkle
    /// root, so two logs with different ids never share a chain or root.
    pub fn new(session_id: [u8; 16]) -> Self {
        Self {
            session_id,
            entries: Vec::new(),
        }
    }

    /// Append an operation and return its assigned sequence number.
    ///
    /// The entry chains onto the current head. `params_json` is hashed as
    /// opaque UTF-8 bytes; callers typically pass compact JSON.
    pub fn append(&mut self, ts_ms: u64, op: &str, params_json: &str) -> u64 {
        let seq = self.entries.len() as u64;
        let prev_hash = self.head_hash();
        let digest = params_digest(params_json);
        let entry_hash = compute_entry_hash(seq, ts_ms, op, &digest, &prev_hash);
        self.entries.push(LogEntry {
            seq,
            ts_ms,
            op: op.to_string(),
            params: params_json.to_string(),
            prev_hash,
            entry_hash,
        });
        seq
    }

    /// The current head of the hash chain.
    ///
    /// Returns the last entry's hash, or the genesis hash
    /// (`blake3(DOM_GENESIS ‖ session_id)`) when the log is empty.
    pub fn head_hash(&self) -> [u8; 32] {
        match self.entries.last() {
            Some(entry) => entry.entry_hash,
            None => genesis_hash(&self.session_id),
        }
    }

    /// Borrow the recorded entries in order.
    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    /// The session identifier this log is bound to.
    pub(crate) fn session_id_bytes(&self) -> [u8; 16] {
        self.session_id
    }

    /// Re-verify the entire hash chain from genesis.
    ///
    /// Returns [`AttestationError::ChainBroken`] carrying the first sequence
    /// number whose sequence value, predecessor link, or recomputed entry hash
    /// is inconsistent.
    pub fn verify_chain(&self) -> Result<(), AttestationError> {
        let mut prev = genesis_hash(&self.session_id);
        for (index, entry) in self.entries.iter().enumerate() {
            let expected_seq = index as u64;
            if entry.seq != expected_seq {
                return Err(AttestationError::ChainBroken { seq: expected_seq });
            }
            if entry.prev_hash != prev {
                return Err(AttestationError::ChainBroken { seq: entry.seq });
            }
            let digest = params_digest(&entry.params);
            let recomputed =
                compute_entry_hash(entry.seq, entry.ts_ms, &entry.op, &digest, &entry.prev_hash);
            if recomputed != entry.entry_hash {
                return Err(AttestationError::ChainBroken { seq: entry.seq });
            }
            prev = entry.entry_hash;
        }
        Ok(())
    }

    /// The domain-separated Merkle root committing to every entry.
    ///
    /// For an empty log this is `blake3(DOM_EMPTY ‖ session_id)`.
    pub fn merkle_root(&self) -> [u8; 32] {
        let leaves: Vec<[u8; 32]> = self
            .entries
            .iter()
            .map(|entry| leaf_hash(&entry.entry_hash))
            .collect();
        super::merkle::merkle_root(&leaves, &self.session_id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn populated() -> SessionLog {
        let mut log = SessionLog::new([1u8; 16]);
        for i in 0..5u64 {
            log.append(1_000 + i, "op", "{}");
        }
        log
    }

    #[test]
    fn well_formed_chain_verifies() {
        assert!(populated().verify_chain().is_ok());
    }

    #[test]
    fn tampered_params_reported_at_exact_seq() {
        for k in 0..5usize {
            let mut log = populated();
            // Corrupt entry k's params but leave its stored entry_hash: the
            // recomputed hash will no longer match, and the break is reported
            // at exactly sequence k.
            log.entries[k].params = "tampered".to_string();
            match log.verify_chain() {
                Err(AttestationError::ChainBroken { seq }) => assert_eq!(seq, k as u64),
                other => panic!("expected ChainBroken at {k}, got {other:?}"),
            }
        }
    }

    #[test]
    fn broken_prev_link_reported() {
        let mut log = populated();
        log.entries[2].prev_hash = [0u8; 32];
        match log.verify_chain() {
            Err(AttestationError::ChainBroken { seq }) => assert_eq!(seq, 2),
            other => panic!("expected ChainBroken at 2, got {other:?}"),
        }
    }

    #[test]
    fn genesis_head_differs_by_session() {
        assert_ne!(
            SessionLog::new([1u8; 16]).head_hash(),
            SessionLog::new([2u8; 16]).head_hash()
        );
    }
}
