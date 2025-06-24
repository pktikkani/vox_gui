use rand::{thread_rng, Rng};
use sha2::{Sha256, Digest};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

const CODE_LENGTH: usize = 6;
const CODE_VALIDITY_SECS: u64 = 300; // 5 minutes

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessCode {
    pub code: String,
    pub hashed: String,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthRequest {
    pub code: String,
    pub client_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthResponse {
    pub success: bool,
    pub session_token: Option<String>,
    pub message: String,
}

impl AccessCode {
    pub fn generate() -> Self {
        let mut rng = thread_rng();
        let code: String = (0..CODE_LENGTH)
            .map(|_| rng.gen_range(0..10).to_string())
            .collect();
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let hashed = Self::hash_code(&code);
        
        AccessCode {
            code: code.clone(),
            hashed,
            created_at: now,
            expires_at: now + CODE_VALIDITY_SECS,
        }
    }
    
    pub fn hash_code(code: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(code.as_bytes());
        format!("{:x}", hasher.finalize())
    }
    
    pub fn verify(&self, code: &str) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        if now > self.expires_at {
            return false;
        }
        
        Self::hash_code(code) == self.hashed
    }
    
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now > self.expires_at
    }
}

#[derive(Debug)]
pub struct SessionToken {
    pub token: String,
    pub created_at: u64,
    pub expires_at: u64,
}

impl SessionToken {
    pub fn generate(validity_hours: u64) -> Self {
        let token: String = thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        SessionToken {
            token,
            created_at: now,
            expires_at: now + (validity_hours * 3600),
        }
    }
    
    pub fn is_valid(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now <= self.expires_at
    }
}