# TODO: oxigdal-security

## High Priority
- [ ] Implement AES-256-GCM encryption for data at rest (Pure Rust via RustCrypto)
- [ ] Add TLS certificate management for data in transit
- [ ] Implement JWT token validation for API authentication
- [ ] Add RBAC policy engine with role hierarchy and permission inheritance
- [ ] Implement audit log tamper detection (Merkle chain)
- [ ] Add GDPR data subject access request (DSAR) workflow

## Medium Priority
- [ ] Implement ABAC (Attribute-Based Access Control) policy evaluation
- [ ] Add data masking/redaction for PII fields in vector features
- [ ] Implement k-anonymity and differential privacy for spatial data
- [ ] Add multi-tenant data isolation with tenant-scoped encryption keys
- [ ] Implement HIPAA compliance checking for health-related geospatial data
- [ ] Add FedRAMP control mapping and evidence collection
- [ ] Implement API key management (generation, rotation, revocation)
- [ ] Add OAuth 2.0 / OIDC client for SSO integration
- [ ] Implement data lineage tracking with cryptographic provenance

## Low Priority / Future
- [ ] Add FIPS 140-2 mode with validated cryptographic modules
- [ ] Implement geographic access restrictions (geofencing for data access)
- [ ] Add security scanning for common geospatial vulnerabilities (XXE in GML/KML)
- [ ] Implement SOC2 Type II evidence auto-collection
- [ ] Add data classification labels (public/internal/confidential/restricted)
- [ ] Implement zero-knowledge proof for location verification
