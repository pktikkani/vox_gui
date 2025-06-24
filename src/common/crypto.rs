use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Result, anyhow};
use x25519_dalek::{EphemeralSecret, PublicKey};
use sha2::{Sha256, Digest};

pub struct CryptoSession {
    cipher: Aes256Gcm,
}

impl CryptoSession {
    pub fn from_shared_secret(shared_secret: &[u8]) -> Result<Self> {
        let mut hasher = Sha256::new();
        hasher.update(shared_secret);
        let key_bytes = hasher.finalize();
        
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        
        Ok(CryptoSession { cipher })
    }
    
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self.cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;
        
        // Prepend nonce to ciphertext
        let mut result = nonce.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }
    
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            return Err(anyhow!("Invalid encrypted data"));
        }
        
        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("Decryption failed: {}", e))
    }
}

pub struct KeyExchange {
    secret: EphemeralSecret,
    public: PublicKey,
}

impl KeyExchange {
    pub fn new() -> Self {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        
        KeyExchange { secret, public }
    }
    
    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }
    
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.public.to_bytes()
    }
    
    pub fn compute_shared_secret(self, their_public: &PublicKey) -> [u8; 32] {
        let shared_secret = self.secret.diffie_hellman(their_public);
        shared_secret.to_bytes()
    }
}