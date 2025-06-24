use vox_gui::common::crypto::{CryptoSession, KeyExchange};

#[test]
fn test_encryption_decryption() {
    // Simulate client and server key exchange
    let client_key_exchange = KeyExchange::new();
    let server_key_exchange = KeyExchange::new();
    
    // Exchange public keys (clone to avoid borrow issues)
    let client_public = client_key_exchange.public_key().clone();
    let server_public = server_key_exchange.public_key().clone();
    
    // Compute shared secrets
    let client_shared = client_key_exchange.compute_shared_secret(&server_public);
    let server_shared = server_key_exchange.compute_shared_secret(&client_public);
    
    // Shared secrets should be identical
    assert_eq!(client_shared, server_shared);
    
    // Create crypto sessions
    let client_crypto = CryptoSession::from_shared_secret(&client_shared).unwrap();
    let server_crypto = CryptoSession::from_shared_secret(&server_shared).unwrap();
    
    // Test encryption/decryption
    let plaintext = b"Hello, secure world!";
    
    // Client encrypts, server decrypts
    let encrypted = client_crypto.encrypt(plaintext).unwrap();
    let decrypted = server_crypto.decrypt(&encrypted).unwrap();
    assert_eq!(plaintext, &decrypted[..]);
    
    // Server encrypts, client decrypts
    let encrypted = server_crypto.encrypt(plaintext).unwrap();
    let decrypted = client_crypto.decrypt(&encrypted).unwrap();
    assert_eq!(plaintext, &decrypted[..]);
}

#[test]
fn test_nonce_uniqueness() {
    let shared_secret = [42u8; 32];
    let crypto = CryptoSession::from_shared_secret(&shared_secret).unwrap();
    
    let plaintext = b"Test message";
    
    // Encrypt the same message multiple times
    let encrypted1 = crypto.encrypt(plaintext).unwrap();
    let encrypted2 = crypto.encrypt(plaintext).unwrap();
    
    // Due to unique nonces, encrypted data should be different
    assert_ne!(encrypted1, encrypted2);
    
    // But both should decrypt to the same plaintext
    let decrypted1 = crypto.decrypt(&encrypted1).unwrap();
    let decrypted2 = crypto.decrypt(&encrypted2).unwrap();
    assert_eq!(decrypted1, decrypted2);
    assert_eq!(plaintext, &decrypted1[..]);
}