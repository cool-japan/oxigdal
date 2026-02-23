//! Integration tests for encryption.

use oxigdal_security::encryption::{
    EncryptionAlgorithm, at_rest::AtRestEncryptor, envelope::EnvelopeEncryptor,
    envelope::InMemoryKekProvider, key_management::KeyManager,
};

#[test]
fn test_encryption_end_to_end() {
    let key = AtRestEncryptor::generate_key(EncryptionAlgorithm::Aes256Gcm);
    let encryptor =
        AtRestEncryptor::new(EncryptionAlgorithm::Aes256Gcm, key, "test-key".to_string())
            .expect("Failed to create encryptor");

    let plaintext = b"Sensitive geospatial data";
    let encrypted = encryptor
        .encrypt(plaintext, None)
        .expect("Encryption failed");
    let decrypted = encryptor.decrypt(&encrypted).expect("Decryption failed");

    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_key_rotation() {
    let manager = KeyManager::new();

    let key1 = manager
        .generate_key(EncryptionAlgorithm::Aes256Gcm, Some(365))
        .expect("Failed to generate key");

    let key2 = manager.rotate_key().expect("Failed to rotate key");
    assert_ne!(key1, key2);

    let (current_id, _, _) = manager
        .get_current_key()
        .expect("Failed to get current key");
    assert_eq!(current_id, key2);
}

#[test]
fn test_envelope_encryption() {
    let kek_provider =
        InMemoryKekProvider::new("test-kek".to_string()).expect("Failed to create KEK provider");
    let encryptor = EnvelopeEncryptor::new(Box::new(kek_provider), EncryptionAlgorithm::Aes256Gcm);

    let plaintext = b"Data with envelope encryption";
    let envelope = encryptor
        .encrypt(plaintext, None)
        .expect("Encryption failed");
    let decrypted = encryptor.decrypt(&envelope).expect("Decryption failed");

    assert_eq!(decrypted, plaintext);
}
