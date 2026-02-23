//! TLS/mTLS configuration for data in transit.

use crate::error::{Result, SecurityError};
use rustls::{ClientConfig, ServerConfig};
use std::io::BufReader;
use std::sync::Arc;

/// TLS configuration builder.
pub struct TlsConfigBuilder {
    server_name: Option<String>,
    ca_cert: Option<Vec<u8>>,
    client_cert: Option<Vec<u8>>,
    client_key: Option<Vec<u8>>,
    server_cert: Option<Vec<u8>>,
    server_key: Option<Vec<u8>>,
    verify_peer: bool,
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

    /// Set the server name for SNI.
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

    /// Build a client configuration.
    pub fn build_client(self) -> Result<Arc<ClientConfig>> {
        let mut root_store = rustls::RootCertStore::empty();

        if let Some(ca_cert) = self.ca_cert {
            let mut reader = BufReader::new(ca_cert.as_slice());
            let certs = rustls_pemfile::certs(&mut reader)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| {
                    SecurityError::certificate(format!("Failed to parse CA cert: {}", e))
                })?;

            for cert in certs {
                root_store.add(cert).map_err(|e| {
                    SecurityError::certificate(format!("Failed to add CA cert: {}", e))
                })?;
            }
        } else {
            // Use system root certificates
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        }

        // Build client config with or without client authentication
        let config = if let (Some(cert), Some(key)) = (self.client_cert, self.client_key) {
            let mut cert_reader = BufReader::new(cert.as_slice());
            let certs = rustls_pemfile::certs(&mut cert_reader)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| {
                    SecurityError::certificate(format!("Failed to parse client cert: {}", e))
                })?;

            let mut key_reader = BufReader::new(key.as_slice());
            let key = rustls_pemfile::private_key(&mut key_reader)
                .map_err(|e| {
                    SecurityError::certificate(format!("Failed to parse private key: {}", e))
                })?
                .ok_or_else(|| SecurityError::certificate("No private key found"))?;

            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_client_auth_cert(certs, key)
                .map_err(|e| {
                    SecurityError::certificate(format!("Failed to set client auth: {}", e))
                })?
        } else {
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };

        Ok(Arc::new(config))
    }

    /// Build a server configuration.
    pub fn build_server(self) -> Result<Arc<ServerConfig>> {
        let cert = self
            .server_cert
            .ok_or_else(|| SecurityError::certificate("Server certificate required"))?;
        let key = self
            .server_key
            .ok_or_else(|| SecurityError::certificate("Server private key required"))?;

        let mut cert_reader = BufReader::new(cert.as_slice());
        let certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                SecurityError::certificate(format!("Failed to parse server cert: {}", e))
            })?;

        let mut key_reader = BufReader::new(key.as_slice());
        let private_key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| SecurityError::certificate(format!("Failed to parse private key: {}", e)))?
            .ok_or_else(|| SecurityError::certificate("No private key found"))?;

        let config = if self.verify_peer {
            // mTLS - require client certificate
            if let Some(ca_cert) = self.ca_cert {
                let mut root_store = rustls::RootCertStore::empty();
                let mut reader = BufReader::new(ca_cert.as_slice());
                let ca_certs = rustls_pemfile::certs(&mut reader)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| {
                        SecurityError::certificate(format!("Failed to parse CA cert: {}", e))
                    })?;

                for cert in ca_certs {
                    root_store.add(cert).map_err(|e| {
                        SecurityError::certificate(format!("Failed to add CA cert: {}", e))
                    })?;
                }

                let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store))
                    .build()
                    .map_err(|e| {
                        SecurityError::certificate(format!("Failed to build verifier: {}", e))
                    })?;

                ServerConfig::builder()
                    .with_client_cert_verifier(verifier)
                    .with_single_cert(certs, private_key)
                    .map_err(|e| {
                        SecurityError::certificate(format!("Failed to build server config: {}", e))
                    })?
            } else {
                return Err(SecurityError::certificate(
                    "CA certificate required for client verification",
                ));
            }
        } else {
            // TLS only - no client certificate required
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, private_key)
                .map_err(|e| {
                    SecurityError::certificate(format!("Failed to build server config: {}", e))
                })?
        };

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
