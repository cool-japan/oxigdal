//! TLS/mTLS configuration for data in transit.
//!
//! The [`TlsConfigBuilder::build_client`] and [`TlsConfigBuilder::build_server`] methods
//! are only available when the `tls` feature is enabled.  After the migration to
//! `oxitls-adapter-rustls-rustcrypto` the `tls` feature is 100% Pure Rust — no
//! `ring` / C / ASM code is required.
//!
//! [`TlsVersion`] and [`CertificateValidation`] are always available (pure Rust).

#[cfg(feature = "tls")]
use crate::error::{Result, SecurityError};
#[cfg(feature = "tls")]
use rustls::{ClientConfig, ServerConfig};
#[cfg(feature = "tls")]
use std::io::BufReader;
#[cfg(feature = "tls")]
use std::sync::Arc;

/// TLS configuration builder.
///
/// The builder itself is always available.  The `build_client` and `build_server`
/// methods that produce rustls configs are gated behind the `tls` feature.
pub struct TlsConfigBuilder {
    /// SNI server name (informational; SNI is set at connect-time by the caller).
    pub server_name: Option<String>,
    /// CA certificate in PEM format.
    pub ca_cert: Option<Vec<u8>>,
    /// Client certificate in PEM format.
    pub client_cert: Option<Vec<u8>>,
    /// Client private key in PEM format.
    pub client_key: Option<Vec<u8>>,
    /// Server certificate in PEM format.
    pub server_cert: Option<Vec<u8>>,
    /// Server private key in PEM format.
    pub server_key: Option<Vec<u8>>,
    /// Whether to verify the peer certificate.
    pub verify_peer: bool,
}

impl Default for TlsConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TlsConfigBuilder {
    /// Create a new TLS configuration builder.
    pub fn new() -> Self {
        Self {
            server_name: None,
            ca_cert: None,
            client_cert: None,
            client_key: None,
            server_cert: None,
            server_key: None,
            verify_peer: true,
        }
    }

    /// Set the server name for SNI (stored for documentation; SNI is passed at connect-time).
    pub fn server_name(mut self, name: String) -> Self {
        self.server_name = Some(name);
        self
    }

    /// Set the CA certificate (PEM format).
    pub fn ca_cert(mut self, cert: Vec<u8>) -> Self {
        self.ca_cert = Some(cert);
        self
    }

    /// Set the client certificate and key (PEM format).
    pub fn client_cert_and_key(mut self, cert: Vec<u8>, key: Vec<u8>) -> Self {
        self.client_cert = Some(cert);
        self.client_key = Some(key);
        self
    }

    /// Set the server certificate and key (PEM format).
    pub fn server_cert_and_key(mut self, cert: Vec<u8>, key: Vec<u8>) -> Self {
        self.server_cert = Some(cert);
        self.server_key = Some(key);
        self
    }

    /// Set whether to verify the peer certificate.
    pub fn verify_peer(mut self, verify: bool) -> Self {
        self.verify_peer = verify;
        self
    }

    /// Build a rustls [`ClientConfig`] using the pure-Rust OxiTLS RustCrypto provider.
    ///
    /// Only available with the `tls` Cargo feature.
    ///
    /// Behaviour:
    /// - If `ca_cert` is set, trusts only that CA.  Otherwise uses the Mozilla CA bundle.
    /// - If `client_cert` + `client_key` are set, enables mutual TLS (mTLS).
    /// - `verify_peer = false` is not supported for clients (always verifies the server).
    #[cfg(feature = "tls")]
    pub fn build_client(self) -> Result<Arc<ClientConfig>> {
        use oxitls_adapter_rustls_rustcrypto::RustcryptoClientConfigBuilder;
        use oxitls_webpki_roots::webpki_root_certs;
        use rustls::RootCertStore;

        // ── Root store ────────────────────────────────────────────────────────
        let root_store: RootCertStore = if let Some(ca_pem) = self.ca_cert {
            let mut store = RootCertStore::empty();
            let mut reader = BufReader::new(ca_pem.as_slice());
            let ca_certs: Vec<_> = rustls_pemfile::certs(&mut reader)
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| SecurityError::certificate(format!("Failed to parse CA cert: {e}")))?;
            for cert in ca_certs {
                store.add(cert).map_err(|e| {
                    SecurityError::certificate(format!("Failed to add CA cert: {e}"))
                })?;
            }
            store
        } else {
            webpki_root_certs()
        };

        // ── mTLS vs anonymous client ──────────────────────────────────────────
        let config = if let (Some(cert_pem), Some(key_pem)) = (self.client_cert, self.client_key) {
            // mTLS: parse client cert chain + private key and inject via the
            // raw rustls builder (RustcryptoClientConfigBuilder always uses
            // with_no_client_auth; drop down to the provider-aware raw API).
            let certs: Vec<_> = {
                let mut reader = BufReader::new(cert_pem.as_slice());
                rustls_pemfile::certs(&mut reader)
                    .collect::<std::result::Result<_, _>>()
                    .map_err(|e| {
                        SecurityError::certificate(format!("Failed to parse client cert: {e}"))
                    })?
            };
            let private_key = {
                let mut reader = BufReader::new(key_pem.as_slice());
                rustls_pemfile::private_key(&mut reader)
                    .map_err(|e| {
                        SecurityError::certificate(format!("Failed to parse private key: {e}"))
                    })?
                    .ok_or_else(|| SecurityError::certificate("No private key found"))?
            };
            let provider = oxitls_adapter_rustls_rustcrypto::pure_provider();
            ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| SecurityError::tls(format!("Protocol version error: {e}")))?
                .with_root_certificates(root_store)
                .with_client_auth_cert(certs, private_key)
                .map_err(|e| {
                    SecurityError::certificate(format!("Failed to set client auth: {e}"))
                })?
        } else {
            // Anonymous client — use the fluent OxiTLS builder.
            RustcryptoClientConfigBuilder::new()
                .with_roots(root_store)
                .build()
                .map_err(|e| SecurityError::tls(e.to_string()))?
        };

        Ok(Arc::new(config))
    }

    /// Build a rustls [`ServerConfig`] using the pure-Rust OxiTLS RustCrypto provider.
    ///
    /// Only available with the `tls` Cargo feature.
    ///
    /// Behaviour:
    /// - `server_cert` + `server_key` (PEM) are required.
    /// - If `ca_cert` is set and `verify_peer = true`, enables mutual TLS (mTLS) requiring
    ///   client certificates validated against the supplied CA.
    /// - If `verify_peer = false`, no client certificate is required.
    #[cfg(feature = "tls")]
    pub fn build_server(self) -> Result<Arc<ServerConfig>> {
        use oxitls_adapter_rustls_rustcrypto::RustcryptoServerConfigBuilder;
        use rustls::RootCertStore;

        let cert_pem = self
            .server_cert
            .ok_or_else(|| SecurityError::certificate("Server certificate required"))?;
        let key_pem = self
            .server_key
            .ok_or_else(|| SecurityError::certificate("Server private key required"))?;

        // ── Parse server cert chain ──────────────────────────────────────────
        let certs: Vec<_> = {
            let mut reader = BufReader::new(cert_pem.as_slice());
            rustls_pemfile::certs(&mut reader)
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| {
                    SecurityError::certificate(format!("Failed to parse server cert: {e}"))
                })?
        };

        // ── Parse private key ────────────────────────────────────────────────
        let private_key = {
            let mut reader = BufReader::new(key_pem.as_slice());
            rustls_pemfile::private_key(&mut reader)
                .map_err(|e| {
                    SecurityError::certificate(format!("Failed to parse private key: {e}"))
                })?
                .ok_or_else(|| SecurityError::certificate("No private key found"))?
        };

        // ── Build server config via OxiTLS fluent builder ────────────────────
        let mut builder =
            RustcryptoServerConfigBuilder::new().with_cert_and_key(certs, private_key);

        if self.verify_peer {
            // mTLS: require client certificates validated against the supplied CA.
            let ca_pem = self.ca_cert.ok_or_else(|| {
                SecurityError::certificate("CA certificate required for client verification")
            })?;
            let mut root_store = RootCertStore::empty();
            let mut reader = BufReader::new(ca_pem.as_slice());
            let ca_certs: Vec<_> = rustls_pemfile::certs(&mut reader)
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| SecurityError::certificate(format!("Failed to parse CA cert: {e}")))?;
            for cert in ca_certs {
                root_store.add(cert).map_err(|e| {
                    SecurityError::certificate(format!("Failed to add CA cert: {e}"))
                })?;
            }
            // required = true: clients without a certificate are rejected.
            builder = builder.with_client_auth(true, root_store);
        }
        // When verify_peer = false, no client_auth is configured (no-client-auth is the default).

        let config = builder
            .build()
            .map_err(|e| SecurityError::tls(e.to_string()))?;

        Ok(Arc::new(config))
    }
}

/// TLS version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVersion {
    /// TLS 1.2
    Tls12,
    /// TLS 1.3
    Tls13,
}

/// Certificate validation result.
#[derive(Debug, Clone)]
pub struct CertificateValidation {
    /// Whether the certificate is valid.
    pub valid: bool,
    /// Validation errors if any.
    pub errors: Vec<String>,
    /// Certificate subject.
    pub subject: Option<String>,
    /// Certificate issuer.
    pub issuer: Option<String>,
    /// Certificate expiration date.
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl CertificateValidation {
    /// Create a valid certificate validation result.
    pub fn valid() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            subject: None,
            issuer: None,
            expires_at: None,
        }
    }

    /// Create an invalid certificate validation result.
    pub fn invalid(errors: Vec<String>) -> Self {
        Self {
            valid: false,
            errors,
            subject: None,
            issuer: None,
            expires_at: None,
        }
    }

    /// Add error to validation result.
    pub fn add_error(&mut self, error: String) {
        self.valid = false;
        self.errors.push(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_config_builder() {
        let builder = TlsConfigBuilder::new()
            .server_name("example.com".to_string())
            .verify_peer(true);

        // Cannot test without actual certificates
        assert_eq!(builder.server_name, Some("example.com".to_string()));
        assert!(builder.verify_peer);
    }

    #[test]
    fn test_certificate_validation() {
        let valid = CertificateValidation::valid();
        assert!(valid.valid);
        assert!(valid.errors.is_empty());

        let invalid = CertificateValidation::invalid(vec!["expired".to_string()]);
        assert!(!invalid.valid);
        assert_eq!(invalid.errors.len(), 1);
    }
}
