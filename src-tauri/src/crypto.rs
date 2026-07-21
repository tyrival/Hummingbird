use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::Rng;
use sha2::{Digest, Sha256};

use crate::error::{AppError, ErrorCode};

const NONCE_BYTES: usize = 12;
const SALT: &[u8] = b"hummingbird-ssh-config-v1";

/// Derive a 32-byte AES key from the system hostname and fixed salt.
/// This gives a deterministic, machine-specific key without storing secrets.
fn derive_key() -> [u8; 32] {
    let host = hostname().unwrap_or_else(|| "hummingbird-default".into());
    let mut hasher = Sha256::new();
    hasher.update(host.as_bytes());
    hasher.update(SALT);
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

fn hostname() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("hostname").output().ok()?;
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .ok()
    }
}

pub fn encrypt_ssh_config(plaintext: &str) -> Result<String, AppError> {
    let key = derive_key();
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    let mut nonce_bytes = [0u8; NONCE_BYTES];
    rand::thread_rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(&combined))
}

pub fn decrypt_ssh_config(ciphertext_b64: &str) -> Result<String, AppError> {
    if ciphertext_b64.trim().is_empty() {
        return Ok(String::new());
    }
    let combined = BASE64
        .decode(ciphertext_b64.trim())
        .map_err(|_| AppError::new(ErrorCode::InvalidSettings))?;
    if combined.len() < NONCE_BYTES + 16 {
        // GCM tag is at least 16 bytes, so minimum is nonce + tag
        return Err(AppError::new(ErrorCode::InvalidSettings));
    }
    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_BYTES);
    let key = derive_key();
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::new(ErrorCode::InvalidSettings))?;
    String::from_utf8(plaintext).map_err(|_| AppError::new(ErrorCode::InvalidSettings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_ssh_config() {
        let original =
            r#"[{"name":"test","host":"192.168.1.1","port":22,"user":"root","password":"secret"}]"#;
        let encrypted = encrypt_ssh_config(original).unwrap();
        let decrypted = decrypt_ssh_config(&encrypted).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn encrypt_produces_different_output() {
        let data = "test";
        let e1 = encrypt_ssh_config(data).unwrap();
        let e2 = encrypt_ssh_config(data).unwrap();
        assert_ne!(e1, e2);
        // But both decrypt to the same value
        assert_eq!(decrypt_ssh_config(&e1).unwrap(), data);
        assert_eq!(decrypt_ssh_config(&e2).unwrap(), data);
    }

    #[test]
    fn empty_string_round_trips() {
        let encrypted = encrypt_ssh_config("").unwrap();
        let decrypted = decrypt_ssh_config(&encrypted).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn empty_ciphertext_returns_empty() {
        assert_eq!(decrypt_ssh_config("").unwrap(), "");
        assert_eq!(decrypt_ssh_config("  ").unwrap(), "");
    }

    #[test]
    fn rejects_tampered_ciphertext() {
        let encrypted = encrypt_ssh_config("data").unwrap();
        let mut tampered = encrypted.clone();
        tampered.push('X');
        assert!(decrypt_ssh_config(&tampered).is_err());
    }
}
