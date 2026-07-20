//! GeoVault wasm surface: tamper-evident session ledger bindings.
//!
//! Wraps the [`oxigeo_security::attestation`] primitives (blake3 hash chain
//! → Merkle root → Ed25519 session seal) as the JS-facing `WasmVaultSession`
//! class, plus the standalone `verifyAttestation` / `fileDigestHex` helpers
//! used by the GeoVault clean-room workstation demo.
//!
//! # Layering
//!
//! * [`VaultSessionCore`] is a pure, dependency-injected core — the caller
//!   supplies the session id, the [`SessionSigner`], and every millisecond
//!   timestamp — so it compiles and is unit-tested on every target.
//! * The `#[wasm_bindgen]` surface (module `js`) is compiled only on
//!   `wasm32`, where it draws entropy from the browser RNG (`getrandom` with
//!   the `wasm_js` backend) and timestamps from `js_sys::Date::now()`.
//!
//! # Timestamps and trust model
//!
//! Timestamps are **informational**: the ordering authority is the sequence
//! number plus the hash chain, never the clock. See the
//! [`oxigeo_security::attestation`] module docs for the full trust model —
//! the attestation cryptographically proves integrity and completeness of the
//! recorded log; the traffic counters are application-observed values bound
//! by the seal, not a mathematical proof of no egress.
//!
//! # Sealed-session immutability
//!
//! [`VaultSessionCore::seal`] is a one-way transition enforced at this layer:
//! afterwards `log_operation` (and a second `seal`) fail with
//! [`VaultError::Sealed`] and `note_traffic` becomes a no-op, so a sealed
//! attestation can never drift from the session state it signed.

// On non-wasm targets the JS bindings below are compiled out and this module
// is private in lib.rs, so the pure core is reachable only from the inline
// unit tests; allow that in native library builds. On wasm32 (where every
// item is wired to the JS surface) the lint stays fully active.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use oxigeo_security::attestation::{
    AttestationError, SealMetadata, SessionLog, SessionSigner, verify_attestation,
};
use thiserror::Error;

/// Application name recorded in every attestation sealed by this surface.
const APP_NAME: &str = "geovault";
/// Application version recorded in attestations (this crate's version).
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Errors surfaced by the GeoVault session ledger bindings.
///
/// At the JS boundary these are stringified via [`std::fmt::Display`]; the
/// `Sealed` message is stable so UI code can match on it.
#[derive(Debug, Error)]
pub enum VaultError {
    /// The session has been sealed; the ledger is immutable afterwards.
    #[error("session sealed: the ledger is immutable after seal()")]
    Sealed,
    /// The RNG failed while creating the session identity.
    #[error("random number generation failed: {0}")]
    Rng(String),
    /// Attestation-layer failure (malformed JSON, bad hex, ...).
    #[error(transparent)]
    Attestation(#[from] AttestationError),
    /// JSON serialization of an attestation or report failed.
    #[error("serialization failed: {0}")]
    Serialization(String),
}

// --- lowercase hex helper (local; no extra dependency) ---

const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

/// Encode bytes as a lowercase hexadecimal string.
fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX_CHARS[(b >> 4) as usize] as char);
        out.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    out
}

/// Convert a JS-supplied cumulative byte total (`f64`) to `u64`.
///
/// Uses Rust saturating float-to-int cast semantics: `NaN` and negative
/// values map to `0`, values beyond `u64::MAX` saturate.
fn traffic_total_to_u64(total: f64) -> u64 {
    total as u64
}

/// Pure, target-independent core of a GeoVault session ledger.
///
/// Owns the append-only [`SessionLog`], the Ed25519 [`SessionSigner`], the
/// application-observed traffic totals, and the sealed flag. All timestamps
/// are injected by the caller (the wasm surface passes `js_sys::Date::now()`;
/// native tests pass fixed values), which keeps every code path — including
/// the exact one `verifyAttestation` wraps — testable off-browser.
pub struct VaultSessionCore {
    log: SessionLog,
    signer: SessionSigner,
    session_id: [u8; 16],
    policy_json: String,
    started_at_ms: u64,
    bytes_ingressed: u64,
    bytes_egressed: u64,
    sealed: bool,
}

impl VaultSessionCore {
    /// Create an unsealed session bound to `session_id`, sealed-by `signer`,
    /// running under `policy_json`, started at `started_at_ms`.
    pub fn new(
        session_id: [u8; 16],
        signer: SessionSigner,
        policy_json: &str,
        started_at_ms: u64,
    ) -> Self {
        Self {
            log: SessionLog::new(session_id),
            signer,
            session_id,
            policy_json: policy_json.to_string(),
            started_at_ms,
            bytes_ingressed: 0,
            bytes_egressed: 0,
            sealed: false,
        }
    }

    /// Session identifier as lowercase hex (16 bytes → 32 chars).
    pub fn session_id_hex(&self) -> String {
        to_hex(&self.session_id)
    }

    /// Ed25519 session public key as lowercase hex (32 bytes → 64 chars).
    pub fn public_key_hex(&self) -> String {
        to_hex(&self.signer.public_key_bytes())
    }

    /// Current head of the hash chain as lowercase hex (genesis hash while
    /// the log is empty).
    pub fn head_hash_hex(&self) -> String {
        to_hex(&self.log.head_hash())
    }

    /// Number of recorded operations (saturating at `u32::MAX`).
    pub fn operation_count(&self) -> u32 {
        u32::try_from(self.log.entries().len()).unwrap_or(u32::MAX)
    }

    /// Append an operation to the ledger and return its sequence number.
    ///
    /// `ts_ms` is informational (ordering authority is the chain itself).
    /// Fails with [`VaultError::Sealed`] once the session is sealed — the
    /// sealed-immutability guarantee is enforced here, at this layer.
    pub fn log_operation(
        &mut self,
        ts_ms: u64,
        op: &str,
        params_json: &str,
    ) -> Result<u64, VaultError> {
        if self.sealed {
            return Err(VaultError::Sealed);
        }
        Ok(self.log.append(ts_ms, op, params_json))
    }

    /// Record the application-observed cumulative traffic totals (bytes).
    ///
    /// Totals are absolute (not deltas); the latest call wins. Ignored after
    /// sealing so the signed counters can never drift from the attestation.
    pub fn note_traffic(&mut self, ingress_total: u64, egress_total: u64) {
        if self.sealed {
            return;
        }
        self.bytes_ingressed = ingress_total;
        self.bytes_egressed = egress_total;
    }

    /// Seal the session and return the signed attestation as pretty-printed
    /// JSON (the exact bytes offered for download as `attestation.json`).
    ///
    /// Marks the session sealed on success; a second call — like any
    /// subsequent [`log_operation`](Self::log_operation) — fails with
    /// [`VaultError::Sealed`].
    pub fn seal(&mut self, ended_at_ms: u64) -> Result<String, VaultError> {
        if self.sealed {
            return Err(VaultError::Sealed);
        }
        let meta = SealMetadata {
            started_at_ms: self.started_at_ms,
            ended_at_ms,
            bytes_egressed: self.bytes_egressed,
            bytes_ingressed: self.bytes_ingressed,
            policy_json: self.policy_json.clone(),
            app_name: APP_NAME.to_string(),
            app_version: APP_VERSION.to_string(),
        };
        let attestation = self.signer.seal(&self.log, &meta)?;
        let json = serde_json::to_string_pretty(&attestation)
            .map_err(|err| VaultError::Serialization(err.to_string()))?;
        self.sealed = true;
        Ok(json)
    }

    /// Whether the session has been sealed (test introspection only).
    #[cfg(test)]
    fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// Current `(ingress, egress)` totals (test introspection only).
    #[cfg(test)]
    fn traffic(&self) -> (u64, u64) {
        (self.bytes_ingressed, self.bytes_egressed)
    }
}

/// Re-verify an attestation JSON and return the
/// [`oxigeo_security::attestation::VerificationReport`] as compact JSON.
///
/// This is the pure code path the `verifyAttestation` JS export wraps; native
/// unit tests exercise it directly. Chain / Merkle / signature results are
/// reported as independent boolean flags inside the JSON; only unparseable
/// input is an error.
pub fn verify_report_json(attestation_json: &str) -> Result<String, VaultError> {
    let report = verify_attestation(attestation_json)?;
    serde_json::to_string(&report).map_err(|err| VaultError::Serialization(err.to_string()))
}

/// blake3 digest of `bytes` as lowercase hex (64 chars; matches `b3sum`).
///
/// Pure code path behind the `fileDigestHex` JS export: GeoVault logs this
/// digest in `file.open` ledger entries so a dropped file's identity is bound
/// into the attestation.
pub fn file_digest_hex_string(bytes: &[u8]) -> String {
    to_hex(blake3::hash(bytes).as_bytes())
}

// --- JS-facing surface (wasm32 only: browser RNG + Date.now()) ---

#[cfg(target_arch = "wasm32")]
mod js {
    use wasm_bindgen::prelude::*;

    use super::{VaultError, VaultSessionCore, traffic_total_to_u64};
    use oxigeo_security::attestation::SessionSigner;

    /// Current wall-clock time in milliseconds (informational only).
    fn now_ms() -> u64 {
        js_sys::Date::now() as u64
    }

    /// Stringify a [`VaultError`] for the JS boundary.
    fn js_err(err: VaultError) -> JsValue {
        JsValue::from_str(&err.to_string())
    }

    /// Tamper-evident GeoVault session ledger (blake3 hash chain → Merkle
    /// root → Ed25519 seal), exported to JavaScript.
    ///
    /// Timestamps come from `Date.now()` and are informational; ordering
    /// authority is the sequence number plus the hash chain. After `seal()`
    /// the ledger is immutable: `logOperation` throws.
    #[wasm_bindgen]
    pub struct WasmVaultSession {
        core: VaultSessionCore,
    }

    #[wasm_bindgen]
    impl WasmVaultSession {
        /// Start a new session under `policy_json`: draws a random 16-byte
        /// session id and a fresh Ed25519 keypair from the browser RNG.
        #[wasm_bindgen(constructor)]
        pub fn new(policy_json: &str) -> Result<WasmVaultSession, JsValue> {
            let mut session_id = [0u8; 16];
            getrandom::fill(&mut session_id)
                .map_err(|err| js_err(VaultError::Rng(err.to_string())))?;
            let signer = SessionSigner::generate().map_err(|err| js_err(VaultError::from(err)))?;
            Ok(WasmVaultSession {
                core: VaultSessionCore::new(session_id, signer, policy_json, now_ms()),
            })
        }

        /// Session identifier as lowercase hex (32 chars).
        #[wasm_bindgen(js_name = sessionIdHex)]
        pub fn session_id_hex(&self) -> String {
            self.core.session_id_hex()
        }

        /// Ed25519 session public key as lowercase hex (64 chars).
        #[wasm_bindgen(js_name = publicKeyHex)]
        pub fn public_key_hex(&self) -> String {
            self.core.public_key_hex()
        }

        /// Append an operation (name + params JSON) to the ledger and return
        /// its sequence number. Throws once the session is sealed.
        #[wasm_bindgen(js_name = logOperation)]
        pub fn log_operation(&mut self, op: &str, params_json: &str) -> Result<u64, JsValue> {
            self.core
                .log_operation(now_ms(), op, params_json)
                .map_err(js_err)
        }

        /// Record the observed cumulative traffic totals in bytes (absolute
        /// totals, not deltas). Silently ignored after sealing.
        #[wasm_bindgen(js_name = noteTraffic)]
        pub fn note_traffic(&mut self, ingress_total: f64, egress_total: f64) {
            self.core.note_traffic(
                traffic_total_to_u64(ingress_total),
                traffic_total_to_u64(egress_total),
            );
        }

        /// Number of operations recorded so far.
        #[wasm_bindgen(js_name = operationCount)]
        pub fn operation_count(&self) -> u32 {
            self.core.operation_count()
        }

        /// Current head of the hash chain as lowercase hex.
        #[wasm_bindgen(js_name = headHashHex)]
        pub fn head_hash_hex(&self) -> String {
            self.core.head_hash_hex()
        }

        /// Seal the session: sign the ledger commitment and return the
        /// attestation as pretty-printed JSON. The ledger is immutable
        /// afterwards; sealing twice throws.
        pub fn seal(&mut self) -> Result<String, JsValue> {
            self.core.seal(now_ms()).map_err(js_err)
        }
    }

    /// Independently re-verify an attestation JSON; returns a
    /// `VerificationReport` JSON with independent `chain_ok` / `merkle_ok` /
    /// `signature_ok` flags. Throws only on unparseable input.
    #[wasm_bindgen(js_name = verifyAttestation)]
    pub fn verify_attestation_js(json: &str) -> Result<String, JsValue> {
        super::verify_report_json(json).map_err(js_err)
    }

    /// blake3 digest of a dropped file's bytes as lowercase hex (matches
    /// `b3sum`), for binding file identity into `file.open` ledger entries.
    #[wasm_bindgen(js_name = fileDigestHex)]
    pub fn file_digest_hex(bytes: &[u8]) -> String {
        super::file_digest_hex_string(bytes)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// Session id used by the B2 golden fixture
    /// (`oxigeo-security/tests/fixtures/attestation_v1.json`): bytes 0..=15.
    const FIXTURE_SESSION_ID: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    /// Deterministic signer seed used by the golden fixture.
    const FIXTURE_SEED: [u8; 32] = [42u8; 32];
    const FIXTURE_POLICY: &str =
        r#"{"csp":"default-src 'self'","enforcement":["csp-meta","fetch-hook"]}"#;
    const FIXTURE_STARTED_MS: u64 = 1_700_000_000_000;
    const FIXTURE_ENDED_MS: u64 = 1_700_000_002_000;

    // Golden values from the B2 fixture. The Ed25519 seal bytes cover the
    // format version, session id, timestamps, entry count, Merkle root, head
    // hash, traffic totals, and policy digest — but NOT app_name/app_version,
    // so these constants are stable across workspace version bumps.
    const GOLDEN_MERKLE_ROOT: &str =
        "95884866c8c6f6c2c629ccd22ea5d224d1762c9d6ce830fb114f9cd3a908b88b";
    const GOLDEN_HEAD_HASH: &str =
        "2fd72008ce1704ca526b0fc2bda359ddeb33a236b215bae0bb44f74937facdd8";
    const GOLDEN_PUBLIC_KEY: &str =
        "197f6b23e16c8532c6abc838facd5ea789be0c76b2920334039bfa8b3d368d61";
    const GOLDEN_SIGNATURE: &str = "c62919b6602738db0c8aa49677dbe558e1b451dca766eba17336605502ae542c76f7faffc9572d101e0ee50471f5ae82a2f27c57cba494cf6db7cf51a9f12804";

    /// Rebuild the exact session recorded in the golden fixture through the
    /// pure core (same code path the wasm surface drives).
    fn fixture_core() -> VaultSessionCore {
        let signer = SessionSigner::from_seed(FIXTURE_SEED);
        let mut core = VaultSessionCore::new(
            FIXTURE_SESSION_ID,
            signer,
            FIXTURE_POLICY,
            FIXTURE_STARTED_MS,
        );
        core.log_operation(
            1_700_000_000_000,
            "session.start",
            r#"{"policy":"default-src 'self'"}"#,
        )
        .unwrap();
        core.log_operation(
            1_700_000_000_500,
            "file.open",
            r#"{"name":"site-k7.tif","size":1048576}"#,
        )
        .unwrap();
        core.log_operation(
            1_700_000_001_000,
            "terrain.hillshade",
            r#"{"azimuth":315,"altitude":45}"#,
        )
        .unwrap();
        core.log_operation(
            1_700_000_001_500,
            "anomaly.detect",
            r#"{"method":"modified_zscore","threshold":3.5,"count":642}"#,
        )
        .unwrap();
        core.note_traffic(1_052_672, 0);
        core
    }

    fn sealed_fixture_json() -> String {
        fixture_core().seal(FIXTURE_ENDED_MS).unwrap()
    }

    fn parse(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn regenerated_fixture_matches_golden_values() {
        let attestation = parse(&sealed_fixture_json());
        assert_eq!(attestation["format"], "oxigeo-attestation");
        assert_eq!(attestation["version"], 1);
        assert_eq!(
            attestation["session_id"],
            "000102030405060708090a0b0c0d0e0f"
        );
        assert_eq!(attestation["merkle_root"], GOLDEN_MERKLE_ROOT);
        assert_eq!(attestation["head_hash"], GOLDEN_HEAD_HASH);
        assert_eq!(attestation["public_key"], GOLDEN_PUBLIC_KEY);
        assert_eq!(attestation["signature"], GOLDEN_SIGNATURE);
        assert_eq!(attestation["bytes_ingressed"], 1_052_672);
        assert_eq!(attestation["bytes_egressed"], 0);
        assert_eq!(attestation["operations"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn fixture_verifies_through_wrapped_code_path() {
        // verify_report_json is the exact function verify_attestation_js wraps.
        let report = parse(&verify_report_json(&sealed_fixture_json()).unwrap());
        assert_eq!(report["chain_ok"], true);
        assert_eq!(report["merkle_ok"], true);
        assert_eq!(report["signature_ok"], true);
        assert_eq!(report["entry_count"], 4);
        assert_eq!(report["session_id"], "000102030405060708090a0b0c0d0e0f");
        assert_eq!(report["bytes_egressed"], 0);
        assert_eq!(report["public_key"], GOLDEN_PUBLIC_KEY);
    }

    #[test]
    fn log_after_seal_is_rejected() {
        let mut core = fixture_core();
        core.seal(FIXTURE_ENDED_MS).unwrap();
        assert!(core.is_sealed());
        let err = core
            .log_operation(FIXTURE_ENDED_MS + 1, "late.op", "{}")
            .unwrap_err();
        assert!(matches!(err, VaultError::Sealed));
        // Sequence numbers and head are untouched by the rejected append.
        assert_eq!(core.operation_count(), 4);
        assert_eq!(core.head_hash_hex(), GOLDEN_HEAD_HASH);
    }

    #[test]
    fn double_seal_is_rejected() {
        let mut core = fixture_core();
        core.seal(FIXTURE_ENDED_MS).unwrap();
        assert!(matches!(
            core.seal(FIXTURE_ENDED_MS + 5).unwrap_err(),
            VaultError::Sealed
        ));
    }

    #[test]
    fn note_traffic_after_seal_is_ignored() {
        let mut core = fixture_core();
        core.seal(FIXTURE_ENDED_MS).unwrap();
        core.note_traffic(u64::MAX, u64::MAX);
        assert_eq!(core.traffic(), (1_052_672, 0));
    }

    #[test]
    fn tampered_params_trips_only_chain_flag() {
        let mut attestation = parse(&sealed_fixture_json());
        attestation["operations"][1]["params"] =
            serde_json::Value::String(r#"{"name":"evil.tif","size":1048576}"#.to_string());
        let tampered = serde_json::to_string(&attestation).unwrap();
        let report = parse(&verify_report_json(&tampered).unwrap());
        // Merkle leaves are built from the *stored* entry hashes and the
        // signature covers the claimed root/head, so a params edit is a pure
        // chain break (B2 semantics: flags are independent).
        assert_eq!(report["chain_ok"], false);
        assert_eq!(report["merkle_ok"], true);
        assert_eq!(report["signature_ok"], true);
    }

    #[test]
    fn flipped_signature_trips_only_signature_flag() {
        let mut attestation = parse(&sealed_fixture_json());
        let mut signature = GOLDEN_SIGNATURE.to_string();
        let first = if signature.starts_with('0') { "1" } else { "0" };
        signature.replace_range(0..1, first);
        attestation["signature"] = serde_json::Value::String(signature);
        let tampered = serde_json::to_string(&attestation).unwrap();
        let report = parse(&verify_report_json(&tampered).unwrap());
        assert_eq!(report["chain_ok"], true);
        assert_eq!(report["merkle_ok"], true);
        assert_eq!(report["signature_ok"], false);
    }

    #[test]
    fn malformed_json_is_a_typed_error() {
        let err = verify_report_json("{ not json").unwrap_err();
        assert!(matches!(
            err,
            VaultError::Attestation(AttestationError::Malformed(_))
        ));
    }

    #[test]
    fn empty_log_seal_verifies() {
        let signer = SessionSigner::from_seed([7u8; 32]);
        let mut core = VaultSessionCore::new([9u8; 16], signer, "{}", 1_000);
        let json = core.seal(2_000).unwrap();
        let report = parse(&verify_report_json(&json).unwrap());
        assert_eq!(report["chain_ok"], true);
        assert_eq!(report["merkle_ok"], true);
        assert_eq!(report["signature_ok"], true);
        assert_eq!(report["entry_count"], 0);
    }

    #[test]
    fn head_hash_and_count_progress_per_operation() {
        let signer = SessionSigner::from_seed([1u8; 32]);
        let mut core = VaultSessionCore::new([2u8; 16], signer, "{}", 0);
        let genesis = core.head_hash_hex();
        assert_eq!(core.operation_count(), 0);
        assert_eq!(core.log_operation(1, "a", "{}").unwrap(), 0);
        let head_one = core.head_hash_hex();
        assert_ne!(head_one, genesis);
        assert_eq!(core.log_operation(2, "b", "{}").unwrap(), 1);
        assert_ne!(core.head_hash_hex(), head_one);
        assert_eq!(core.operation_count(), 2);
    }

    #[test]
    fn identity_hex_forms_are_lowercase_and_sized() {
        let core = fixture_core();
        assert_eq!(core.session_id_hex(), "000102030405060708090a0b0c0d0e0f");
        assert_eq!(core.public_key_hex(), GOLDEN_PUBLIC_KEY);
        let head = core.head_hash_hex();
        assert_eq!(head.len(), 64);
        assert!(
            head.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn file_digest_matches_known_blake3_vector() {
        // Official blake3 test vector for empty input; matches `b3sum` output.
        assert_eq!(
            file_digest_hex_string(b""),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        let digest = file_digest_hex_string(b"GeoVault");
        assert_eq!(digest.len(), 64);
        assert_ne!(digest, file_digest_hex_string(b""));
        // Deterministic.
        assert_eq!(digest, file_digest_hex_string(b"GeoVault"));
    }

    #[test]
    fn traffic_total_conversion_is_saturating() {
        assert_eq!(traffic_total_to_u64(f64::NAN), 0);
        assert_eq!(traffic_total_to_u64(-1024.0), 0);
        assert_eq!(traffic_total_to_u64(1.9), 1);
        assert_eq!(traffic_total_to_u64(1_052_672.0), 1_052_672);
        assert_eq!(traffic_total_to_u64(f64::INFINITY), u64::MAX);
    }

    #[test]
    fn sealed_json_is_pretty_printed_for_download() {
        let json = sealed_fixture_json();
        assert!(json.contains('\n'));
        assert!(json.starts_with('{'));
        // Round-trips through serde untouched.
        let value = parse(&json);
        assert_eq!(value["app_name"], APP_NAME);
        assert_eq!(value["app_version"], APP_VERSION);
    }
}
