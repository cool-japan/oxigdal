# TODO: oxigdal-security

> **Purpose:** Enterprise security features — encryption, access control, audit, compliance, multi-tenancy.
> **Status (2026-05-16):** 7,538 LoC · 99 tests · 2 narrative stubs in `scanning/`
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Implement JWT (HS256/RS256/ES256) token validation for API authentication
  - **Goal:** `JwtValidator` that decodes a compact JWS, verifies signature against a JWK (HS256 = HMAC-SHA256, RS256 = RSASSA-PKCS1-v1_5, ES256 = ECDSA P-256), checks `exp`/`nbf`/`iss`/`aud` per RFC 7519 §4.1.
  - **Design:** Pure Rust — `hmac` + `sha2` for HS256; `rsa` crate for RS256 (workspace already uses `rustls`/`webpki` for cert PKI but no JWT). `ed25519-dalek` for EdDSA optional. Define `JwkSet` (RFC 7517) loader with kid lookup; expose `validate(token: &str, options: &ValidationOptions) -> Result<Claims>`. Reject `alg: none` outright (CVE-2015-9235 pattern).
  - **Files:** `src/auth/jwt.rs` (new), `src/auth/jwk.rs` (new), `src/auth/mod.rs` (new), `src/lib.rs` (add `pub mod auth`).
  - **Tests:** *(proposed)* `test_jwt_hs256_roundtrip`, `test_jwt_rs256_valid_signature`, `test_jwt_rejects_alg_none`, `test_jwt_expired_token`, `test_jwt_nbf_future_token`, `test_jwt_audience_mismatch`, `test_jwk_kid_lookup`.
  - **Risk:** Algorithm-confusion attacks if `JwkSet` doesn't bind alg per key — JWT validator must reject mismatched (alg, kty) pairs.
  - **Prerequisites:** Add `hmac`, `rsa`, `ecdsa`, `p256` workspace deps; `sha2` already present.

- [ ] Implement audit log tamper detection via Merkle hash chain
  - **Goal:** Tamper-evident audit log: each `AuditLogEntry` carries `prev_hash` (SHA-256 of previous entry) and a Merkle root recomputable from any entry range. Detect inserted/deleted/modified entries in O(log n) per check.
  - **Design:** Hash-linked list per RFC 6962 §2.1 Merkle tree mechanics: leaf = SHA-256(canonical_serialize(entry)); parent = SHA-256(0x01 ‖ left ‖ right). Add `chain_hash: [u8; 32]` field to `AuditLogEntry`; `MerkleAuditStorage` wraps `InMemoryAuditStorage` and maintains a rolling root. Verify with `verify_range(start, end) -> Result<MerkleProof>`.
  - **Files:** `src/audit/chain.rs` (new ~300 LoC), `src/audit/mod.rs` (extend `AuditLogEntry`), `src/audit/storage.rs` (add `MerkleAuditStorage` impl).
  - **Tests:** *(proposed)* `test_merkle_single_entry_root`, `test_merkle_chain_grows_monotonically`, `test_merkle_detects_modified_entry`, `test_merkle_detects_deleted_entry`, `test_merkle_inclusion_proof_verifies`, `test_merkle_root_serializable`.
  - **Risk:** Canonical serialization stability — `serde_json` field ordering is BTreeMap-stable but timestamps/IDs must be normalized; specify RFC 8785 (JCS) or fix custom order in rustdoc.
  - **Prerequisites:** None — `sha2` already a dep.

- [ ] Implement GDPR data subject access request (DSAR) workflow
  - **Goal:** End-to-end DSAR processor: given a `data_subject_id`, enumerate Access/Erasure/Portability per GDPR Art. 15/17/20; emit export bundle or erasure manifest; record outcome via audit logger.
  - **Design:** `DsarProcessor` trait with `find_records`/`export_bundle`/`erase_records` hooks (registered by tenant). Engine orchestrates: status `Pending` → `Processing` → `Completed` / `Rejected` (struct already at `src/compliance/gdpr.rs:107-132`). Bundle format: JSON manifest + per-record payload, signed with Ed25519 (existing crypto). Erasure: returns cryptographic proof of deletion (record-id, hash-before, tombstone-hash).
  - **Files:** `src/compliance/dsar.rs` (new), `src/compliance/gdpr.rs` (extend with engine API), `src/compliance/mod.rs` (re-export).
  - **Tests:** *(proposed)* `test_dsar_access_returns_bundle`, `test_dsar_erasure_emits_tombstone`, `test_dsar_portability_includes_metadata`, `test_dsar_rejected_outside_scope`, `test_dsar_audit_trail_recorded`.
  - **Risk:** Cross-tenant data leakage if `find_records` hook lacks tenant scoping — enforce `tenant_id` filter in trait signature.
  - **Prerequisites:** None.

- [ ] Replace stub scanners with real implementations
  - **Verified gap:** `src/scanning/malware.rs:17` — `// Implementation would integrate with antivirus engines` (returns empty findings); `src/scanning/vulnerability.rs:17` — `// Implementation would integrate with vulnerability databases`.
  - **Goal:** `MalwareScanner` runs ClamAV-style signature scan on file bytes (Pure Rust hash-based YARA-lite subset); `VulnerabilityScanner` queries OSV.dev API or local SBOM index, mapping CVE → finding.
  - **Design:** Malware — load signature DB at construction (list of SHA-256 hashes + glob name patterns), scan in 64 KiB blocks. Vulnerability — accept `Vec<DependencyAdvisory>` from caller (decoupled from network); produce findings per matched (package, version, cve_id). Both return real `Finding` instances per existing struct.
  - **Files:** `src/scanning/malware.rs` (replace body), `src/scanning/vulnerability.rs` (replace body), `src/scanning/signatures.rs` (new — signature DB loader).
  - **Tests:** *(proposed)* `test_malware_known_hash_detected`, `test_malware_clean_file_no_findings`, `test_vulnerability_cve_matched`, `test_vulnerability_unaffected_version_skipped`.
  - **Risk:** YARA-lite scope creep — keep to SHA-256 + glob; defer full YARA grammar to a `oxigdal-yara` crate.
  - **Prerequisites:** None.

## Medium Priority
- [ ] ABAC condition evaluator extension (boolean expressions over attributes)
  - **Goal:** Augment `src/access_control/abac.rs` operators with AND/OR/NOT compositions and arithmetic over numeric attributes.
  - **Files:** `src/access_control/abac.rs` (existing, 412 LoC).
  - **Why deferred:** RBAC + simple ABAC already covers 80% of policy use cases; advanced composition is small-yield until customer demand surfaces.

- [ ] k-anonymity / l-diversity over spatial feature attributes
  - **Goal:** Generalization-based anonymizer; group records until each equivalence class has k indistinguishable members.
  - **Files:** `src/anonymization/generalization.rs` (existing, 108 LoC).
  - **Why deferred:** Differential privacy (`src/anonymization/differential_privacy.rs`) covers stronger guarantees for most cases.

- [ ] Multi-tenant data isolation with tenant-scoped encryption keys
  - **Goal:** Key derivation per tenant via HKDF-SHA256 from root KEK; rotate without re-encrypting historical data via key-wrapping.
  - **Files:** `src/multitenancy/isolation.rs` (existing), `src/encryption/key_management.rs` (existing, 351 LoC).
  - **Why deferred:** Single-tenant encryption is production-ready; multi-tenant key isolation needs careful threat-model review.

- [ ] FIPS 140-2 / FIPS 140-3 mode (validated cryptographic modules)
  - **Goal:** Optional `fips` feature switching `aes-gcm`/`sha2` to `aws-lc-rs` (FIPS-validated). Current workspace uses `ring` which has aws-lc-rs backend.
  - **Files:** workspace `Cargo.toml`, `src/encryption/at_rest.rs` (existing, 401 LoC).
  - **Why deferred:** Requires aws-lc-rs FIPS certified build pipeline; defer to v1.0.0.

- [ ] OAuth 2.0 / OIDC client (RFC 6749 + OIDC Core 1.0)
  - **Goal:** Authorization code + PKCE flow client, with discovery endpoint, JWK fetch, and ID-token validation.
  - **Files:** `src/auth/oauth.rs` (new), `src/auth/oidc.rs` (new).
  - **Why deferred:** Blocked by JWT validator (High Priority item above).

## Low Priority / Future (one-liners)
- [ ] Geographic access restrictions (geofencing for data access)
- [ ] Differential privacy budget tracker (ε accounting across queries)
- [ ] Security scanning for XXE in GML/KML parsers
- [ ] SOC 2 Type II evidence auto-collection (control mapping → audit-log queries)
- [ ] Data classification labels (public/internal/confidential/restricted) propagated through lineage graph
- [ ] Zero-knowledge proof for location verification (Bulletproofs or zk-SNARK)
- [ ] API key generation/rotation/revocation with constant-time lookup

## Cross-crate dependencies
- **Blocks:** oxigdal-services (JWT-protected endpoints), oxigdal-workflow (HMAC webhook validator at `external.rs:1057`)
- **Blocked by:** None

## Recently completed (verbatim)
- [x] Implement AES-256-GCM encryption for data at rest (Pure Rust via RustCrypto) — `src/encryption/at_rest.rs` (401 LoC), supports AES-256-GCM and ChaCha20-Poly1305, AAD optional, in-place mode, FieldEncryptor wrapper
- [x] Add TLS certificate management for data in transit — `src/encryption/in_transit.rs` (272 LoC), rustls-based client + server config, mTLS support, webpki-roots integration
- [x] Add RBAC policy engine with role hierarchy and permission inheritance — `src/access_control/rbac.rs` (381 LoC), `roles.rs` (159 LoC), `permissions.rs` (278 LoC), `policies.rs` (364 LoC); circular inheritance detection
- [x] In-memory audit storage with query filters (subject/resource/event-type/tenant/time-range) — `src/audit/storage.rs` (248 LoC)
- [x] Differential privacy Laplace + Gaussian mechanisms (RFC SP 800-188 conceptual basis) — `src/anonymization/differential_privacy.rs` (73 LoC), uses `scirs2_core::random` (SciRS2 policy)
- [x] Secret scanner with regex patterns (AWS access keys, generic API keys, PEM private keys) — `src/scanning/secrets.rs` (83 LoC)
- [x] Data lineage graph with petgraph DAG — `src/lineage/graph.rs` (346 LoC), `query.rs` (260 LoC)
- [x] GDPR compliance struct skeleton (encryption/audit/consent/retention checklist) — `src/compliance/gdpr.rs` (150 LoC)
- [x] HIPAA + FedRAMP compliance checkers (control-mapping skeleton) — `src/compliance/hipaa.rs` (61 LoC), `fedramp.rs` (54 LoC)
- [x] Field-level string and JSON encryption (base64 envelope) — `FieldEncryptor` in `at_rest.rs`

---
*Last audited: 2026-05-16*
