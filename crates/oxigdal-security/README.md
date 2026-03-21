# oxigdal-security

[![Crates.io](https://img.shields.io/crates/v/oxigdal-security.svg)](https://crates.io/crates/oxigdal-security)
[![Documentation](https://docs.rs/oxigdal-security/badge.svg)](https://docs.rs/oxigdal-security)
[![License](https://img.shields.io/crates/l/oxigdal-security.svg)](LICENSE)
[![Pure Rust](https://img.shields.io/badge/Pure-Rust-orange.svg)](https://www.rust-lang.org/)

Enterprise-grade security features for OxiGDAL geospatial data processing. Provides comprehensive encryption, access control, compliance management, audit logging, and data protection capabilities for handling sensitive geospatial information.

## Features

- **End-to-End Encryption**: AES-256-GCM and ChaCha20-Poly1305 encryption for data at rest and in transit
- **Key Management**: Secure key derivation using Argon2id and PBKDF2 with OWASP-recommended settings
- **Access Control**: RBAC (Role-Based Access Control) and ABAC (Attribute-Based Access Control) frameworks
- **Audit Logging**: Comprehensive audit trail with queryable storage and event tracking
- **Data Lineage**: Track data provenance and transformations with graph-based lineage tracking
- **Multi-Tenancy**: Complete tenant isolation with quota management and multi-tenant security policies
- **Data Anonymization**: Differential privacy, data masking, and value generalization techniques
- **Compliance Reporting**: Support for GDPR, HIPAA, and FedRAMP compliance frameworks
- **Security Scanning**: Detect secrets, vulnerabilities, and potential malware in data
- **Pure Rust**: 100% Pure Rust implementation with no C/Fortran dependencies
- **No Unwrap Policy**: All fallible operations properly return `Result<T, E>` with descriptive error types
- **Async-First**: Built on tokio for high-performance async operations

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-security = "0.1.3"
```

## Quick Start

Configure security settings for your geospatial data:

```rust
use oxigdal_security::{SecurityConfig, encryption::KeyManager, encryption::EncryptionAlgorithm};

fn main() -> oxigdal_security::Result<()> {
    // Create a secure configuration
    let config = SecurityConfig::secure()
        .with_encryption(true)
        .with_access_control(true)
        .with_audit_logging(true)
        .with_lineage_tracking(true)
        .with_multitenancy(true);

    println!("Security configuration: {:?}", config);
    Ok(())
}
```

## Usage

### Basic Encryption

```rust
use oxigdal_security::encryption::{
    AtRestEncryptor, EncryptionAlgorithm, EncryptionMetadata
};

fn encrypt_data() -> oxigdal_security::Result<()> {
    // Create an encryptor for at-rest encryption
    let encryptor = AtRestEncryptor::new(EncryptionAlgorithm::Aes256Gcm)?;

    // Encrypt sensitive data
    let plaintext = b"Sensitive geospatial coordinates";
    let encrypted = encryptor.encrypt(plaintext)?;

    println!("Encrypted data length: {}", encrypted.ciphertext.len());

    // Decrypt data
    let decrypted = encryptor.decrypt(&encrypted)?;
    assert_eq!(decrypted, plaintext);

    Ok(())
}
```

### Key Derivation

```rust
use oxigdal_security::encryption::{derive_key, KeyDerivationParams};

fn derive_secure_key() -> oxigdal_security::Result<()> {
    // Derive a key from a password using Argon2id (recommended)
    let password = b"user_password";
    let salt = b"random_salt_value_16_bytes_long";

    let params = KeyDerivationParams::argon2_recommended(salt.to_vec());
    let key = derive_key(password, &params, 32)?; // 32 bytes for AES-256

    println!("Derived key length: {}", key.len());
    Ok(())
}
```

### Access Control with RBAC

```rust
use oxigdal_security::access_control::{Subject, SubjectType, Resource, ResourceType, Action, AccessContext, AccessRequest};

fn check_access() -> oxigdal_security::Result<()> {
    // Create a subject (user)
    let subject = Subject::new("user-123".to_string(), SubjectType::User)
        .with_attribute("department".to_string(), "geospatial-team".to_string());

    // Define a resource (dataset)
    let resource = Resource::new("dataset-456".to_string(), ResourceType::Dataset)
        .with_attribute("classification".to_string(), "confidential".to_string());

    // Create access request
    let context = AccessContext::new()
        .with_source_ip("192.168.1.100".to_string())
        .with_tenant_id("tenant-001".to_string());

    let request = AccessRequest::new(
        subject,
        resource,
        Action::Read,
        context
    );

    println!("Access request created: {:?}", request.subject.id);
    Ok(())
}
```

### Audit Logging

```rust
use oxigdal_security::audit::{AuditLogEntry, AuditEventType, AuditResult};

fn log_access_event() -> oxigdal_security::Result<()> {
    // Create an audit log entry
    let audit_entry = AuditLogEntry::new(AuditEventType::DataAccess, AuditResult::Success)
        .with_subject("user-123".to_string())
        .with_resource("dataset-456".to_string())
        .with_action("read_features".to_string())
        .with_source_ip("192.168.1.100".to_string())
        .with_tenant_id("tenant-001".to_string())
        .with_message("Successfully accessed vector features".to_string());

    println!("Audit entry ID: {}", audit_entry.id);
    println!("Event type: {:?}", audit_entry.event_type);
    Ok(())
}
```

### Data Anonymization

```rust
use oxigdal_security::anonymization::*;

fn anonymize_coordinates() -> oxigdal_security::Result<()> {
    // Apply data masking to sensitive coordinates
    let original_data = "39.7392,-104.9903"; // Denver coordinates

    // Create a masking provider
    let masker = PatternMaskingProvider::new("*".to_string());

    // Mask the data
    let masked = masker.mask(original_data)?;
    println!("Original: {}", original_data);
    println!("Masked: {}", masked);

    Ok(())
}
```

### Multi-Tenancy with Isolation

```rust
use oxigdal_security::multitenancy::{TenantContext, TenantIsolationPolicy};

fn setup_tenant() -> oxigdal_security::Result<()> {
    // Create a tenant context
    let tenant = TenantContext::new("tenant-001".to_string(), "Acme Corp".to_string());

    // Define isolation policy
    let policy = TenantIsolationPolicy::strict();

    println!("Tenant ID: {}", tenant.id);
    println!("Tenant name: {}", tenant.name);
    println!("Isolation policy: {:?}", policy);

    Ok(())
}
```

## API Overview

### Core Modules

| Module | Description |
|--------|-------------|
| `encryption` | Encryption infrastructure (at-rest, in-transit, key management, envelope encryption) |
| `access_control` | Access control framework (RBAC, ABAC, permissions, policies) |
| `audit` | Audit logging system (event logging, storage, querying) |
| `lineage` | Data lineage tracking (metadata, graph-based provenance) |
| `multitenancy` | Multi-tenant support (tenant isolation, quotas) |
| `anonymization` | Data anonymization (masking, generalization, differential privacy) |
| `compliance` | Compliance reporting (GDPR, HIPAA, FedRAMP) |
| `scanning` | Security scanning (vulnerability detection, secrets detection, malware scanning) |

### Key Types

#### Encryption Types
- `EncryptionAlgorithm`: AES-256-GCM (default) or ChaCha20-Poly1305
- `KeyDerivationFunction`: PBKDF2-SHA256 or Argon2id
- `AtRestEncryptor`: Encrypt/decrypt data at rest
- `EnvelopeEncryptor`: Asymmetric key wrapping
- `TlsConfigBuilder`: Configure TLS for in-transit encryption

#### Access Control Types
- `Subject`: Identifies user, service, or API key
- `Resource`: Identifies geospatial resource (dataset, layer, feature)
- `Action`: Eight standard actions (Read, Write, Delete, Execute, List, Create, Update, Admin)
- `AccessRequest`: Request evaluation with context
- `AccessDecision`: Allow or Deny decision

#### Audit Types
- `AuditLogEntry`: Complete audit log record with metadata
- `AuditEventType`: Authentication, authorization, data access, modifications, etc.
- `AuditResult`: Success, Failure, or Denied
- `AuditSeverity`: Info, Warning, Error, Critical

### Error Handling

All operations return `Result<T>` with comprehensive error types:

```rust
pub enum SecurityError {
    Encryption(String),
    Decryption(String),
    KeyManagement(String),
    Authorization(String),
    AccessDenied(String),
    TenantIsolationViolation(String),
    QuotaExceeded(String),
    ComplianceViolation(String),
    GdprCompliance(String),
    HipaaCompliance(String),
    FedRampCompliance(String),
    VulnerabilityDetected(String),
    SecretDetected(String),
    MalwareDetected(String),
    // ... and more
}
```

## Security Considerations

### Encryption Best Practices

- **AES-256-GCM** is recommended for most use cases
- **ChaCha20-Poly1305** is faster on systems without AES hardware acceleration
- All encryption keys should be at least 32 bytes (256 bits)
- Always use random, unique nonces/IVs for each encryption operation

### Key Derivation

- **Argon2id** is the recommended key derivation function (memory-hard, resistant to side-channels)
- Default settings: 19456 KiB memory, 2 time cost, 1 parallelism (suitable for production)
- **PBKDF2-SHA256** uses OWASP-recommended 600,000 iterations
- Minimum recommended salt: 16 bytes of cryptographically secure random data

### Access Control

- RBAC is suitable for role-based permission models
- ABAC provides fine-grained control based on attributes
- Combine with audit logging for complete accountability
- Always validate both subject identity and resource classification

### Compliance

- GDPR: Data minimization, right to be forgotten, data portability
- HIPAA: Protected health information handling and access controls
- FedRAMP: Federal information security requirements
- Enable audit logging for compliance proof

## Performance

The library is optimized for high-performance secure operations:

- Ring-based cryptographic primitives for optimal performance
- Async-first design with tokio for non-blocking I/O
- Efficient memory handling with pre-allocated buffers
- Optimized key derivation with hardware acceleration support

For benchmarks, run:

```bash
cargo bench --bench security_bench
```

## Examples

See the [examples](examples/) directory for complete working examples:

- Basic encryption and decryption
- Key derivation from passwords
- RBAC and ABAC implementations
- Audit logging and querying
- Multi-tenant isolation
- Data anonymization techniques
- Compliance reporting

## Integration with OxiGDAL

OxiGDAL-Security integrates seamlessly with other OxiGDAL crates:

```rust
use oxigdal_core::data::GeoDataFrame;
use oxigdal_security::encryption::AtRestEncryptor;
use oxigdal_security::access_control::*;

async fn secure_data_processing() -> oxigdal_security::Result<()> {
    // Load GeoDataFrame
    let _gdf: GeoDataFrame = todo!("load from source");

    // Apply encryption
    let _encryptor = AtRestEncryptor::new(oxigdal_security::encryption::EncryptionAlgorithm::Aes256Gcm)?;

    // Apply access control
    let _access_control = true; // Would integrate with your access control system

    // Log audit trail
    let _audit = true; // Would log to audit system

    Ok(())
}
```

## Documentation

Full documentation is available at [docs.rs/oxigdal-security](https://docs.rs/oxigdal-security).

Generate local documentation with:

```bash
cargo doc --no-deps --open
```

## Testing

Run the test suite:

```bash
cargo test --all-features
```

With logging:

```bash
RUST_LOG=debug cargo test -- --nocapture
```

## Contributing

Contributions are welcome! Please ensure:

- No `unwrap()` usage (use `?` operator or explicit error handling)
- All public APIs are documented
- Tests are included for new functionality
- Code follows COOLJAPAN policies (Pure Rust, no C/Fortran dependencies by default)

See [CONTRIBUTING.md](../../../CONTRIBUTING.md) for detailed guidelines.

## License

This project is licensed under [Apache-2.0](LICENSE).

## Compliance

This crate is designed to help meet security compliance requirements:

- **GDPR**: Personal data protection and privacy regulations
- **HIPAA**: Health information security requirements
- **FedRAMP**: Federal information security standards
- **SOC 2**: Security and privacy controls
- **ISO 27001**: Information security management

## Related Projects

- [OxiGDAL](https://github.com/cool-japan/oxigdal) - Geospatial data processing
- [OxiBLAS](https://github.com/cool-japan/oxiblas) - Pure Rust linear algebra
- [SciRS2](https://github.com/cool-japan/scirs) - Scientific computing ecosystem
- [Oxicode](https://github.com/cool-japan/oxicode) - Pure Rust serialization (bincode replacement)

## Support

For issues, questions, or security concerns:

- Open an issue on [GitHub](https://github.com/cool-japan/oxigdal/issues)
- Email: security@cool-japan.org
- Security vulnerabilities should be reported responsibly per [SECURITY.md](SECURITY.md)

---

Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem - Enterprise-grade Rust libraries for geospatial computing.
