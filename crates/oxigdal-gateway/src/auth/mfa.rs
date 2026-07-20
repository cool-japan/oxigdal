//! Multi-factor authentication (MFA) implementation.

use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use hmac::KeyInit;
use std::sync::Arc;

/// MFA method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MfaMethod {
    /// Time-based One-Time Password (TOTP)
    Totp,
    /// SMS-based verification
    Sms,
    /// Email-based verification
    Email,
    /// Backup codes
    BackupCode,
}

/// MFA authenticator.
pub struct MfaAuthenticator {
    totp_secrets: Arc<DashMap<String, TotpSecret>>,
    pending_challenges: Arc<DashMap<String, MfaChallenge>>,
    backup_codes: Arc<DashMap<String, Vec<String>>>,
}

/// TOTP secret information.
#[derive(Debug, Clone)]
pub struct TotpSecret {
    /// User ID
    pub user_id: String,
    /// Secret key
    pub secret: Vec<u8>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// MFA challenge.
#[derive(Debug, Clone)]
pub struct MfaChallenge {
    /// User ID
    pub user_id: String,
    /// Challenge method
    pub method: MfaMethod,
    /// Challenge code
    pub code: String,
    /// Expiration timestamp
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Compares two byte slices for equality in constant time (with respect to
/// their contents).
///
/// `subtle` is not part of this workspace's dependency set, so this is a
/// small local implementation rather than pulling in a new external crate
/// (COOLJAPAN Pure-Rust / minimal-dependency policy). Lengths are compared
/// first and this is *not* constant-time with respect to length: every
/// value compared here (TOTP codes, backup codes) has a fixed, publicly
/// known format/length, so the length itself is not a secret. Once lengths
/// match, every byte pair is visited and folded into a single accumulator
/// with bitwise OR, so no early return reveals which byte position (if
/// any) first differed.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }

    diff == 0
}

impl MfaAuthenticator {
    /// Creates a new MFA authenticator.
    pub fn new() -> Self {
        Self {
            totp_secrets: Arc::new(DashMap::new()),
            pending_challenges: Arc::new(DashMap::new()),
            backup_codes: Arc::new(DashMap::new()),
        }
    }

    /// Generates a TOTP secret for a user.
    pub fn generate_totp_secret(&self, user_id: String) -> Result<Vec<u8>> {
        use getrandom::fill;

        let mut secret = vec![0u8; 20];
        fill(&mut secret).map_err(|e| {
            GatewayError::InternalError(format!("Failed to generate secret: {}", e))
        })?;

        let totp_secret = TotpSecret {
            user_id: user_id.clone(),
            secret: secret.clone(),
            created_at: chrono::Utc::now(),
        };

        self.totp_secrets.insert(user_id, totp_secret);

        Ok(secret)
    }

    /// Gets the TOTP URI for QR code generation.
    pub fn get_totp_uri(&self, user_id: &str, issuer: &str) -> Result<String> {
        let secret = self
            .totp_secrets
            .get(user_id)
            .ok_or_else(|| GatewayError::InvalidToken("TOTP not configured".to_string()))?;

        let encoded_secret =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &secret.secret);

        Ok(format!(
            "otpauth://totp/{}:{}?secret={}&issuer={}",
            issuer, user_id, encoded_secret, issuer
        ))
    }

    /// Verifies a TOTP code.
    ///
    /// Per [RFC 6238 §5.2](https://www.rfc-editor.org/rfc/rfc6238#section-5.2),
    /// a validation system should accept a small number of time-steps
    /// adjacent to the current one to tolerate clock skew between the
    /// client and server. This checks the previous, current, and next
    /// 30-second time-step (a ±30s window) against the submitted code.
    ///
    /// The submitted code is compared against every candidate with a
    /// constant-time comparison, and all three candidates are always
    /// evaluated (no short-circuiting on the first match), so neither
    /// which window matched nor the content of the code can be inferred
    /// from response timing.
    pub fn verify_totp(&self, user_id: &str, code: &str) -> Result<bool> {
        let secret = self
            .totp_secrets
            .get(user_id)
            .ok_or_else(|| GatewayError::InvalidToken("TOTP not configured".to_string()))?;

        let current_time_step = chrono::Utc::now().timestamp() / 30;
        let submitted = code.as_bytes();

        let mut matched = false;
        for offset in [-1i64, 0, 1] {
            let candidate =
                self.generate_totp_code_for_counter(&secret.secret, current_time_step + offset)?;
            matched |= constant_time_eq(submitted, candidate.as_bytes());
        }

        Ok(matched)
    }

    /// Generates a TOTP code from a secret for an explicit 30-second time
    /// step ("counter" in RFC 6238 terms), so [`Self::verify_totp`] can
    /// evaluate the adjacent time-steps for clock-skew tolerance.
    fn generate_totp_code_for_counter(&self, secret: &[u8], time_step: i64) -> Result<String> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        type HmacSha256 = Hmac<Sha256>;

        let time_bytes = time_step.to_be_bytes();

        // HMAC-SHA256
        let mut mac = HmacSha256::new_from_slice(secret)
            .map_err(|e| GatewayError::InternalError(format!("HMAC error: {}", e)))?;
        mac.update(&time_bytes);
        let result = mac.finalize();
        let bytes = result.into_bytes();

        // Dynamic truncation
        let offset = (bytes[bytes.len() - 1] & 0x0f) as usize;
        let code = u32::from_be_bytes([
            bytes[offset] & 0x7f,
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]);

        // Generate 6-digit code
        Ok(format!("{:06}", code % 1_000_000))
    }

    /// Sends an SMS challenge.
    pub fn send_sms_challenge(&self, user_id: String, phone_number: &str) -> Result<String> {
        use getrandom::fill;

        // Generate 6-digit code
        let mut code_bytes = [0u8; 4];
        fill(&mut code_bytes)
            .map_err(|e| GatewayError::InternalError(format!("Failed to generate code: {}", e)))?;
        let code_num = u32::from_le_bytes(code_bytes) % 1_000_000;
        let code = format!("{:06}", code_num);

        let challenge_id = format!("sms_{}", uuid::Uuid::new_v4());
        let challenge = MfaChallenge {
            user_id: user_id.clone(),
            method: MfaMethod::Sms,
            code: code.clone(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        };

        self.pending_challenges
            .insert(challenge_id.clone(), challenge);

        // In a real implementation, send SMS via provider
        tracing::info!(
            "Sending SMS code {} to {} for user {}",
            code,
            phone_number,
            user_id
        );

        Ok(challenge_id)
    }

    /// Verifies an SMS challenge.
    pub fn verify_sms_challenge(&self, challenge_id: &str, code: &str) -> Result<bool> {
        let challenge = self
            .pending_challenges
            .get(challenge_id)
            .ok_or_else(|| GatewayError::InvalidToken("Challenge not found".to_string()))?;

        if chrono::Utc::now() > challenge.expires_at {
            drop(challenge);
            self.pending_challenges.remove(challenge_id);
            return Err(GatewayError::TokenExpired);
        }

        let is_valid = challenge.code == code;

        if is_valid {
            drop(challenge);
            self.pending_challenges.remove(challenge_id);
        }

        Ok(is_valid)
    }

    /// Generates backup codes for a user.
    pub fn generate_backup_codes(&self, user_id: String, count: usize) -> Result<Vec<String>> {
        use getrandom::fill;

        let mut codes = Vec::with_capacity(count);

        for _ in 0..count {
            let mut code_bytes = vec![0u8; 8];
            fill(&mut code_bytes).map_err(|e| {
                GatewayError::InternalError(format!("Failed to generate backup code: {}", e))
            })?;

            let code = base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                &code_bytes,
            );

            codes.push(code);
        }

        self.backup_codes.insert(user_id, codes.clone());

        Ok(codes)
    }

    /// Verifies and consumes a backup code.
    ///
    /// Each stored code is compared against the submitted code with a
    /// constant-time comparison, and every stored code is checked (no
    /// early exit on the first match), so response timing does not reveal
    /// which stored code (if any) matched or how many bytes of a near
    /// match were correct.
    pub fn verify_backup_code(&self, user_id: &str, code: &str) -> Result<bool> {
        let mut codes = self
            .backup_codes
            .get_mut(user_id)
            .ok_or_else(|| GatewayError::InvalidToken("Backup codes not configured".to_string()))?;

        let submitted = code.as_bytes();
        let mut matched_pos: Option<usize> = None;
        for (index, stored) in codes.iter().enumerate() {
            if constant_time_eq(stored.as_bytes(), submitted) {
                matched_pos = Some(index);
            }
        }

        if let Some(pos) = matched_pos {
            codes.remove(pos);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Gets remaining backup code count.
    pub fn get_backup_code_count(&self, user_id: &str) -> usize {
        self.backup_codes
            .get(user_id)
            .map(|codes| codes.len())
            .unwrap_or(0)
    }
}

impl Default for MfaAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_totp_secret() {
        let mfa = MfaAuthenticator::new();
        let secret = mfa.generate_totp_secret("user123".to_string());

        assert!(secret.is_ok());
        let secret = secret.ok();
        assert!(secret.is_some());
        let secret = secret.unwrap_or_default();
        assert_eq!(secret.len(), 20);
    }

    #[test]
    fn test_get_totp_uri() {
        let mfa = MfaAuthenticator::new();
        let _secret = mfa.generate_totp_secret("user123".to_string());

        let uri = mfa.get_totp_uri("user123", "OxiGDAL");
        assert!(uri.is_ok());

        let uri = uri.ok();
        assert!(uri.is_some());
        let uri = uri.unwrap_or_default();
        assert!(uri.starts_with("otpauth://totp/OxiGDAL:user123"));
    }

    #[test]
    fn test_verify_totp() {
        let mfa = MfaAuthenticator::new();
        let _secret = mfa.generate_totp_secret("user123".to_string());

        // Generate current code
        let secret = mfa.totp_secrets.get("user123");
        assert!(secret.is_some());

        let current_time_step = chrono::Utc::now().timestamp() / 30;
        let code = if let Some(secret_ref) = secret {
            mfa.generate_totp_code_for_counter(&secret_ref.secret, current_time_step)
        } else {
            return; // Test fails if no secret
        };
        assert!(code.is_ok());

        let code = code.ok();
        assert!(code.is_some());
        let code = code.unwrap_or_default();

        // Verify the code
        let result = mfa.verify_totp("user123", &code);
        assert!(result.is_ok());
        assert!(result.unwrap_or(false));
    }

    #[test]
    fn test_sms_challenge() {
        let mfa = MfaAuthenticator::new();
        let challenge_id = mfa.send_sms_challenge("user123".to_string(), "+1234567890");

        assert!(challenge_id.is_ok());
        let challenge_id = challenge_id.ok();
        assert!(challenge_id.is_some());
        let challenge_id = challenge_id.unwrap_or_default();

        // Get the code from the challenge, then drop the Ref to release the
        // DashMap read lock. Without this, verify_sms_challenge's remove()
        // call would deadlock waiting for a write lock on the same shard.
        let code = {
            let challenge_ref = mfa.pending_challenges.get(&challenge_id);
            assert!(challenge_ref.is_some(), "Challenge not found");
            let challenge_ref = match challenge_ref {
                Some(r) => r,
                None => return,
            };
            challenge_ref.code.clone()
            // challenge_ref (Ref) is dropped here, releasing the read lock
        };

        // Verify with correct code
        let result = mfa.verify_sms_challenge(&challenge_id, &code);
        assert!(result.is_ok());
        assert!(result.unwrap_or(false));

        // Challenge should be consumed
        assert!(mfa.pending_challenges.get(&challenge_id).is_none());
    }

    #[test]
    fn test_backup_codes() {
        let mfa = MfaAuthenticator::new();
        let codes = mfa.generate_backup_codes("user123".to_string(), 10);

        assert!(codes.is_ok());
        let codes = codes.ok();
        assert!(codes.is_some());
        let codes = codes.unwrap_or_default();
        assert_eq!(codes.len(), 10);

        // Verify a code
        let code = codes[0].clone();
        let result = mfa.verify_backup_code("user123", &code);
        assert!(result.is_ok());
        assert!(result.unwrap_or(false));

        // Code should be consumed
        let count = mfa.get_backup_code_count("user123");
        assert_eq!(count, 9);

        // Same code should not work again
        let result = mfa.verify_backup_code("user123", &code);
        assert!(result.is_ok());
        assert!(!result.unwrap_or(true));
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"123456", b"123456"));
        assert!(!constant_time_eq(b"123456", b"123457"));
        // Different lengths must never compare equal.
        assert!(!constant_time_eq(b"123456", b"1234567"));
        assert!(!constant_time_eq(b"1234567", b"123456"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_verify_totp_rejects_wrong_code() {
        let mfa = MfaAuthenticator::new();
        let _secret = mfa.generate_totp_secret("user123".to_string());

        let secret = mfa.totp_secrets.get("user123");
        let secret_bytes = match secret {
            Some(secret_ref) => secret_ref.secret.clone(),
            None => panic!("secret was not created"),
        };

        let current_time_step = chrono::Utc::now().timestamp() / 30;
        let current_code = mfa
            .generate_totp_code_for_counter(&secret_bytes, current_time_step)
            .unwrap_or_default();

        // Flip the last digit to obtain a code that is guaranteed wrong.
        let mut wrong_code: Vec<u8> = current_code.into_bytes();
        if let Some(last) = wrong_code.last_mut() {
            *last = if *last == b'9' { b'0' } else { *last + 1 };
        }
        let wrong_code = String::from_utf8(wrong_code).unwrap_or_default();

        let result = mfa.verify_totp("user123", &wrong_code);
        assert!(result.is_ok());
        assert!(!result.unwrap_or(true));

        // A code of the wrong length must also be rejected (exercises the
        // constant-time comparison's length-mismatch path).
        let result = mfa.verify_totp("user123", "1");
        assert!(result.is_ok());
        assert!(!result.unwrap_or(true));
    }

    #[test]
    fn test_verify_totp_accepts_previous_time_step_skew() {
        let mfa = MfaAuthenticator::new();
        let _secret = mfa.generate_totp_secret("user123".to_string());

        let secret = mfa.totp_secrets.get("user123");
        let secret_bytes = match secret {
            Some(secret_ref) => secret_ref.secret.clone(),
            None => panic!("secret was not created"),
        };

        // Simulate a client whose clock is ~30s behind the server: the
        // client computes its code for the *previous* 30-second time-step,
        // but the server verifies "now". Per RFC 6238 §5.2 this must still
        // be accepted.
        let current_time_step = chrono::Utc::now().timestamp() / 30;
        let previous_step_code = mfa
            .generate_totp_code_for_counter(&secret_bytes, current_time_step - 1)
            .unwrap_or_default();

        let result = mfa.verify_totp("user123", &previous_step_code);
        assert!(result.is_ok());
        assert!(result.unwrap_or(false));

        // A code that is two time-steps (60s) skewed falls outside the
        // ±1 window and must be rejected.
        let far_step_code = mfa
            .generate_totp_code_for_counter(&secret_bytes, current_time_step - 2)
            .unwrap_or_default();
        // Guard against the astronomically unlikely case that the far-step
        // code happens to collide with one of the three accepted codes.
        if far_step_code != previous_step_code {
            let result = mfa.verify_totp("user123", &far_step_code);
            assert!(result.is_ok());
            assert!(!result.unwrap_or(true));
        }
    }

    #[test]
    fn test_verify_backup_code_rejects_wrong_code() {
        let mfa = MfaAuthenticator::new();
        let codes = mfa
            .generate_backup_codes("user123".to_string(), 5)
            .unwrap_or_default();
        assert_eq!(codes.len(), 5);

        let result = mfa.verify_backup_code("user123", "not-a-real-backup-code");
        assert!(result.is_ok());
        assert!(!result.unwrap_or(true));

        // All original codes must remain unconsumed.
        assert_eq!(mfa.get_backup_code_count("user123"), 5);
    }
}
