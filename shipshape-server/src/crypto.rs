//! Token encryption helpers for storing OAuth credentials at rest.

use base64::{Engine as _, engine::general_purpose};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use std::sync::Arc;

/// Environment variable holding base64-encoded token encryption keys.
pub(crate) const TOKEN_KEY_ENV: &str = "SHIPSHAPE_TOKEN_KEYS";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const TOKEN_PREFIX: &str = "enc:v1:";

/// Encrypts and decrypts OAuth tokens for storage in Postgres.
#[derive(Clone)]
pub(crate) struct TokenCipher(Arc<TokenCipherInner>);

impl std::fmt::Debug for TokenCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenCipher").finish_non_exhaustive()
    }
}

struct TokenCipherInner {
    primary: LessSafeKey,
    #[allow(dead_code)]
    fallbacks: Vec<LessSafeKey>,
}

impl TokenCipher {
    /// Build a cipher from `SHIPSHAPE_TOKEN_KEYS` (comma-separated base64 32-byte keys).
    pub(crate) fn from_env() -> Result<Self, String> {
        let raw = std::env::var(TOKEN_KEY_ENV)
            .map_err(|_| format!("{TOKEN_KEY_ENV} must be set to base64-encoded 32-byte keys"))?;
        Self::from_base64_keys(raw.split(',').map(str::trim))
    }

    /// Build a cipher from base64-encoded keys (primary first).
    pub(crate) fn from_base64_keys<I, S>(keys: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut decoded = Vec::new();
        for key in keys {
            let trimmed = key.as_ref().trim();
            if trimmed.is_empty() {
                continue;
            }
            decoded.push(parse_base64_key(trimmed)?);
        }
        if decoded.is_empty() {
            return Err("no token encryption keys provided".to_string());
        }
        let mut iter = decoded.into_iter();
        let primary = build_cipher(iter.next().expect("primary key"));
        let fallbacks = iter.map(build_cipher).collect();
        Ok(Self(Arc::new(TokenCipherInner { primary, fallbacks })))
    }

    /// Encrypt a token string for storage.
    pub(crate) fn encrypt(&self, plaintext: &str) -> String {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .expect("nonce generation failed");
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut in_out = plaintext.as_bytes().to_vec();
        self.0
            .primary
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .expect("token encryption failed");
        let mut payload = Vec::with_capacity(NONCE_LEN + in_out.len());
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&in_out);
        let encoded = general_purpose::STANDARD.encode(payload);
        format!("{TOKEN_PREFIX}{encoded}")
    }

    /// Decrypt a stored token, passing through legacy plaintext values.
    #[allow(dead_code)]
    pub(crate) fn decrypt(&self, stored: &str) -> Result<String, String> {
        let Some(encoded) = stored.strip_prefix(TOKEN_PREFIX) else {
            return Ok(stored.to_string());
        };
        let payload = general_purpose::STANDARD
            .decode(encoded)
            .map_err(|err| format!("token payload base64 decode failed: {err}"))?;
        if payload.len() <= NONCE_LEN {
            return Err("token payload missing nonce".to_string());
        }
        let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
        if let Ok(plaintext) = decrypt_with_key(&self.0.primary, nonce_bytes, ciphertext) {
            return Ok(plaintext);
        }
        for cipher in &self.0.fallbacks {
            if let Ok(plaintext) = decrypt_with_key(cipher, nonce_bytes, ciphertext) {
                return Ok(plaintext);
            }
        }
        Err("token decryption failed".to_string())
    }

    /// Return true when the stored token includes the encryption prefix.
    #[allow(dead_code)]
    pub(crate) fn is_encrypted(stored: &str) -> bool {
        stored.starts_with(TOKEN_PREFIX)
    }
}

fn parse_base64_key(key: &str) -> Result<[u8; KEY_LEN], String> {
    let bytes = general_purpose::STANDARD
        .decode(key)
        .map_err(|err| format!("token key base64 decode failed: {err}"))?;
    if bytes.len() != KEY_LEN {
        return Err(format!(
            "token key must be {KEY_LEN} bytes, got {}",
            bytes.len()
        ));
    }
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn build_cipher(key: [u8; KEY_LEN]) -> LessSafeKey {
    let unbound = UnboundKey::new(&AES_256_GCM, &key).expect("token key length invalid");
    LessSafeKey::new(unbound)
}

#[allow(dead_code)]
fn decrypt_with_key(
    key: &LessSafeKey,
    nonce_bytes: &[u8],
    ciphertext: &[u8],
) -> Result<String, String> {
    let mut nonce_array = [0u8; NONCE_LEN];
    nonce_array.copy_from_slice(nonce_bytes);
    let nonce = Nonce::assume_unique_for_key(nonce_array);
    let mut in_out = ciphertext.to_vec();
    let plaintext = key
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| "token decryption failed".to_string())?;
    Ok(String::from_utf8_lossy(plaintext).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_PRIMARY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    const KEY_FALLBACK: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_SHORT: &str = "AAAAAAAAAAAAAAAAAAAAAA==";

    fn cipher() -> TokenCipher {
        TokenCipher::from_base64_keys([KEY_PRIMARY]).expect("cipher")
    }

    #[test]
    fn from_base64_keys_rejects_empty() {
        let err = TokenCipher::from_base64_keys(["   "]).unwrap_err();
        assert!(err.contains("no token encryption keys"));
    }

    #[test]
    fn from_base64_keys_rejects_short_key() {
        let err = TokenCipher::from_base64_keys([KEY_SHORT]).unwrap_err();
        assert!(err.contains("token key must be"));
    }

    #[test]
    fn from_env_requires_keys() {
        let guard = env_lock();
        let previous = std::env::var(TOKEN_KEY_ENV).ok();
        unsafe {
            std::env::remove_var(TOKEN_KEY_ENV);
        }
        let err = TokenCipher::from_env().unwrap_err();
        assert!(err.contains("SHIPSHAPE_TOKEN_KEYS"));
        restore_env(previous);
        drop(guard);
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let cipher = cipher();
        let token = cipher.encrypt("secret-token");
        assert!(TokenCipher::is_encrypted(&token));
        let decrypted = cipher.decrypt(&token).expect("decrypt");
        assert_eq!(decrypted, "secret-token");
    }

    #[test]
    fn decrypt_passes_through_plaintext() {
        let cipher = cipher();
        let decrypted = cipher.decrypt("plain-token").expect("decrypt");
        assert_eq!(decrypted, "plain-token");
    }

    #[test]
    fn decrypt_uses_fallback_keys() {
        let fallback = TokenCipher::from_base64_keys([KEY_FALLBACK]).expect("fallback");
        let encrypted = fallback.encrypt("rotated-token");
        let multi = TokenCipher::from_base64_keys([KEY_PRIMARY, KEY_FALLBACK]).expect("multi");
        let decrypted = multi.decrypt(&encrypted).expect("decrypt");
        assert_eq!(decrypted, "rotated-token");
    }

    #[test]
    fn decrypt_rejects_bad_payload() {
        let cipher = cipher();
        let err = cipher.decrypt("enc:v1:bad*base64").unwrap_err();
        assert!(err.contains("base64"));
    }

    #[test]
    fn decrypt_rejects_short_payload() {
        let cipher = cipher();
        let err = cipher.decrypt("enc:v1:AA==").unwrap_err();
        assert!(err.contains("nonce"));
    }

    #[test]
    fn decrypt_rejects_wrong_key() {
        let primary = TokenCipher::from_base64_keys([KEY_PRIMARY]).expect("primary");
        let other = TokenCipher::from_base64_keys([KEY_FALLBACK]).expect("other");
        let encrypted = other.encrypt("secret");
        let err = primary.decrypt(&encrypted).unwrap_err();
        assert!(err.contains("decryption failed"));
    }

    #[test]
    fn from_env_parses_keys() {
        let guard = env_lock();
        let previous = std::env::var(TOKEN_KEY_ENV).ok();
        unsafe {
            std::env::set_var(TOKEN_KEY_ENV, format!("{KEY_PRIMARY},{KEY_FALLBACK}"));
        }
        let cipher = TokenCipher::from_env().expect("env cipher");
        let token = cipher.encrypt("env-token");
        let decrypted = cipher.decrypt(&token).expect("decrypt");
        assert_eq!(decrypted, "env-token");
        restore_env(previous);
        drop(guard);
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};

        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock")
    }

    fn restore_env(previous: Option<String>) {
        match previous {
            Some(value) => unsafe {
                std::env::set_var(TOKEN_KEY_ENV, value);
            },
            None => unsafe {
                std::env::remove_var(TOKEN_KEY_ENV);
            },
        }
    }
}
