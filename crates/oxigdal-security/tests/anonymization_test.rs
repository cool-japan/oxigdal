//! Integration tests for anonymization.

use oxigdal_security::anonymization::{generalization::KAnonymity, masking::MaskingStrategy};

#[test]
fn test_data_masking() {
    let strategy = MaskingStrategy::Email;
    let masked = strategy.apply("user@example.com");
    assert_ne!(masked, "user@example.com");
    assert!(masked.contains("@example.com"));
}

#[test]
fn test_k_anonymity() {
    let checker = KAnonymity::new(2);

    let records = vec![
        vec!["Alice".to_string(), "30".to_string()],
        vec!["Bob".to_string(), "30".to_string()],
        vec!["Charlie".to_string(), "40".to_string()],
        vec!["David".to_string(), "40".to_string()],
    ];

    assert!(checker.check(&records, &[1]));
}
