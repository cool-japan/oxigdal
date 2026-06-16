//! Integration tests for HMAC-SHA256 webhook signature validation.
//!
//! These tests cover the real `validate_signature` implementation in
//! `WebhookTrigger`, verifying correct HMAC-SHA256 computation, uppercase
//! hex normalization, and constant-time comparison semantics.

use oxigdal_workflow::integrations::external::WebhookTrigger;

// ---------------------------------------------------------------------------
// RFC 4231 compliance
// ---------------------------------------------------------------------------

/// RFC 4231 test case 2:
///   key  = "Jefe" (0x4a656665)
///   data = "what do ya want for nothing?"
///   HMAC-SHA-256 = 5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843
#[test]
fn test_hmac_sha256_matches_rfc4231_test_case_2() {
    let trigger = WebhookTrigger::new("wf-rfc4231").with_secret("Jefe");
    let payload = b"what do ya want for nothing?";
    let sig = "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";
    assert!(
        trigger.validate_signature(payload, sig),
        "RFC 4231 test case 2 HMAC value must be accepted"
    );
}

// ---------------------------------------------------------------------------
// Positive / accept cases
// ---------------------------------------------------------------------------

/// A correctly signed payload with a known secret must be accepted.
#[test]
fn test_webhook_valid_signature_accepted() {
    // HMAC-SHA256("test-secret", "test payload")
    // = 2f94a757d2246073e26781d117ce0183ebd87b4d66c460494376d5c37d71985b
    let trigger = WebhookTrigger::new("wf1").with_secret("test-secret");
    let payload = b"test payload";
    let sig = "sha256=2f94a757d2246073e26781d117ce0183ebd87b4d66c460494376d5c37d71985b";
    assert!(
        trigger.validate_signature(payload, sig),
        "A correctly-signed payload must validate successfully"
    );
}

/// Signatures sent with uppercase hex digits must be accepted (normalization).
#[test]
fn test_webhook_signature_uppercase_hex_accepted() {
    // Same payload + secret as test_webhook_valid_signature_accepted, but uppercase.
    let trigger = WebhookTrigger::new("wf1").with_secret("test-secret");
    let payload = b"test payload";
    let sig_upper = "sha256=2F94A757D2246073E26781D117CE0183EBD87B4D66C460494376D5C37D71985B";
    assert!(
        trigger.validate_signature(payload, sig_upper),
        "Uppercase hex signature must be accepted after lowercase normalization"
    );
}

/// An empty payload must produce the correct HMAC and validate.
#[test]
fn test_webhook_empty_payload_signature() {
    // HMAC-SHA256("test-secret", b"")
    // = a41bc6d81d6413576ae0994995e0ad89a416ec97389515c3604f47722122eeeb
    let trigger = WebhookTrigger::new("wf1").with_secret("test-secret");
    let sig = "sha256=a41bc6d81d6413576ae0994995e0ad89a416ec97389515c3604f47722122eeeb";
    assert!(
        trigger.validate_signature(b"", sig),
        "Empty payload with correct HMAC must validate"
    );
}

// ---------------------------------------------------------------------------
// Negative / reject cases
// ---------------------------------------------------------------------------

/// A tampered payload must not match the original signature.
#[test]
fn test_webhook_tampered_payload_rejected() {
    let trigger = WebhookTrigger::new("wf1").with_secret("test-secret");
    // Correct sig is for "test payload"; we send "test payload!" instead.
    let sig = "sha256=2f94a757d2246073e26781d117ce0183ebd87b4d66c460494376d5c37d71985b";
    assert!(
        !trigger.validate_signature(b"test payload!", sig),
        "A tampered payload must not validate"
    );
}

/// A valid payload signed with the wrong secret must be rejected.
#[test]
fn test_webhook_wrong_secret_rejected() {
    let trigger = WebhookTrigger::new("wf1").with_secret("wrong-secret");
    // Signature was computed with "test-secret".
    let sig = "sha256=2f94a757d2246073e26781d117ce0183ebd87b4d66c460494376d5c37d71985b";
    assert!(
        !trigger.validate_signature(b"test payload", sig),
        "A signature computed with a different secret must be rejected"
    );
}

/// A signature string that is missing the `sha256=` prefix must be rejected.
#[test]
fn test_webhook_signature_missing_prefix_rejected() {
    let trigger = WebhookTrigger::new("wf1").with_secret("test-secret");
    // The raw hex without the "sha256=" prefix should not match.
    let sig_no_prefix = "2f94a757d2246073e26781d117ce0183ebd87b4d66c460494376d5c37d71985b";
    assert!(
        !trigger.validate_signature(b"test payload", sig_no_prefix),
        "A signature without the 'sha256=' prefix must be rejected"
    );
}

// ---------------------------------------------------------------------------
// Secret-less webhook (skip validation)
// ---------------------------------------------------------------------------

/// When no secret is configured validation is skipped and the method returns `true`
/// regardless of the signature value.
#[test]
fn test_webhook_no_secret_skips_validation_returns_true() {
    let trigger = WebhookTrigger::new("wf1"); // no .with_secret()
    assert!(
        trigger.validate_signature(b"anything", "sha256=wrong"),
        "With no secret configured, validate_signature must return true unconditionally"
    );
    assert!(
        trigger.validate_signature(b"anything", "garbage"),
        "With no secret configured, even a garbage signature must return true"
    );
}

// ---------------------------------------------------------------------------
// Constant-time comparison (via validate_signature semantics)
// ---------------------------------------------------------------------------

/// Verify that equal signatures are accepted and that any single-bit difference
/// causes rejection — exercising the constant-time comparison path.
#[test]
fn test_constant_time_compare_preserved() {
    // Equal signatures → true.
    let trigger = WebhookTrigger::new("wf1").with_secret("Jefe");
    let payload = b"what do ya want for nothing?";
    let correct_sig = "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";
    assert!(
        trigger.validate_signature(payload, correct_sig),
        "Identical signatures must compare equal"
    );

    // Single-character difference → false.
    let wrong_last_char = "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3842";
    assert!(
        !trigger.validate_signature(payload, wrong_last_char),
        "A signature differing by one hex digit must be rejected"
    );

    // Different length (missing last two chars) → false.
    let truncated = "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec38";
    assert!(
        !trigger.validate_signature(payload, truncated),
        "A truncated signature must be rejected"
    );
}
