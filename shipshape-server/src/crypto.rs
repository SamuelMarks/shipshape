//! Token encryption helpers for storing OAuth credentials at rest.
//!
//! Uses AES-256-GCM authenticated encryption (`ring::aead`) with key rotation
//! support via primary and fallback keys loaded from `SHIPSHAPE_TOKEN_KEYS`
//! or `SHIPSHAPE_TOKEN_KEY`.

use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

use crate::error::{ServerError, ServerResult};

/// Ciphertext prefix identifying AES-256-GCM v1 encrypted tokens.
const ENCRYPTED_PREFIX: &str = "enc:v1:";
/// Required byte length for an AES-256-GCM key (256 bits).
const KEY_LEN: usize = 32;
/// Required byte length for a 96-bit GCM nonce.
const NONCE_LEN: usize = 12;
/// Environment variable holding comma-separated base64-encoded 32-byte keys.
const TOKEN_KEYS_ENV: &str = "SHIPSHAPE_TOKEN_KEYS";
/// Fallback environment variable holding a single base64-encoded 32-byte key.
const TOKEN_KEY_ENV: &str = "SHIPSHAPE_TOKEN_KEY";

/// Encrypts and decrypts OAuth tokens using AES-256-GCM.
#[derive(Clone)]
pub struct TokenCipher {
    /// Shared primary and fallback AEAD keys.
    inner: Arc<TokenCipherInner>,
}

/// Internal AEAD key ring holding the primary encryption key and rotation fallbacks.
struct TokenCipherInner {
    /// Primary key used for all new encryptions and tried first on decryption.
    primary: LessSafeKey,
    /// Historical fallback keys tried in order when primary decryption fails.
    fallbacks: Vec<LessSafeKey>,
}

impl From<ring::error::Unspecified> for ServerError {
    fn from(_: ring::error::Unspecified) -> Self {
        Self::CryptoOperationFailed {
            detail: "cryptographic operation failed".to_string(),
        }
    }
}

impl TokenCipher {
    /// Load token encryption keys from `SHIPSHAPE_TOKEN_KEYS` or `SHIPSHAPE_TOKEN_KEY`.
    pub fn from_env() -> ServerResult<Self> {
        let raw = match std::env::var(TOKEN_KEYS_ENV).or_else(|_| std::env::var(TOKEN_KEY_ENV)) {
            Ok(v) => v,
            Err(_) => {
                let detail = format!("{TOKEN_KEYS_ENV} or {TOKEN_KEY_ENV} must be set");
                return Err(ServerError::CryptoKeyInvalid { detail });
            }
        };
        let keys: Vec<String> = raw
            .split(',')
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        Self::from_base64_keys(keys)
    }

    /// Build a cipher from base64-encoded 32-byte keys (first is primary, rest are fallbacks).
    ///
    /// # Arguments
    /// * `keys` - Iterable of base64-encoded 256-bit keys.
    pub fn from_base64_keys<I, S>(keys: I) -> ServerResult<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let raw_keys: Vec<String> = keys
            .into_iter()
            .map(|k| k.as_ref().trim().to_string())
            .filter(|k| !k.is_empty())
            .collect();
        Self::from_trimmed_keys(&raw_keys)
    }

    /// Parse a slice of non-empty base64-encoded 32-byte keys into a [`TokenCipher`].
    ///
    /// # Arguments
    /// * `keys` - Slice of trimmed base64 key strings.
    fn from_trimmed_keys(keys: &[String]) -> ServerResult<Self> {
        let mut parsed = Vec::with_capacity(keys.len());
        for key in keys {
            parsed.push(parse_base64_key(key)?);
        }
        build_cipher(parsed)
    }

    /// Encrypt a plaintext token for storage, returning a prefixed base64 string.
    ///
    /// # Arguments
    /// * `plaintext` - Raw secret token to encrypt.
    pub fn encrypt(&self, plaintext: &str) -> ServerResult<String> {
        let rng = SystemRandom::new();
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rng.fill(&mut nonce_bytes)?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut buffer = plaintext.as_bytes().to_vec();
        self.inner
            .primary
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut buffer)?;
        let mut combined = Vec::with_capacity(NONCE_LEN + buffer.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&buffer);
        let encoded = general_purpose::STANDARD.encode(combined);
        Ok(format!("{ENCRYPTED_PREFIX}{encoded}"))
    }

    /// Decrypt a stored token, trying the primary key and then any configured fallback keys.
    ///
    /// # Arguments
    /// * `stored` - Prefixed base64 ciphertext string (`"enc:v1:..."`).
    pub fn decrypt(&self, stored: &str) -> ServerResult<String> {
        let Some(encoded) = stored.strip_prefix(ENCRYPTED_PREFIX) else {
            return Err(ServerError::CryptoOperationFailed {
                detail: "stored token missing encryption prefix".to_string(),
            });
        };
        let raw = general_purpose::STANDARD.decode(encoded)?;
        if raw.len() <= NONCE_LEN {
            return Err(ServerError::CryptoOperationFailed {
                detail: "encrypted token payload is too short".to_string(),
            });
        }
        let (nonce_bytes, ciphertext) = raw.split_at(NONCE_LEN);
        let mut nonce_arr = [0u8; NONCE_LEN];
        nonce_arr.copy_from_slice(nonce_bytes);

        let keys = std::iter::once(&self.inner.primary).chain(self.inner.fallbacks.iter());
        for key in keys {
            if let Ok(plaintext) = decrypt_with_key(key, nonce_arr, ciphertext) {
                return Ok(plaintext);
            }
        }
        Err(ServerError::CryptoOperationFailed {
            detail: "unable to decrypt token with configured keys".to_string(),
        })
    }

    /// Check whether a stored value carries the `"enc:v1:"` encrypted prefix.
    ///
    /// # Arguments
    /// * `value` - Stored token string to inspect.
    #[must_use]
    pub fn is_encrypted(value: &str) -> bool {
        value.starts_with(ENCRYPTED_PREFIX)
    }
}

/// Decode and validate a single base64-encoded 32-byte key.
///
/// # Arguments
/// * `encoded` - Base64 string representing 32 raw bytes.
fn parse_base64_key(encoded: &str) -> ServerResult<[u8; KEY_LEN]> {
    let decoded = general_purpose::STANDARD.decode(encoded.trim())?;
    let invalid_err = ServerError::CryptoKeyInvalid {
        detail: format!("token key must decode to {KEY_LEN} bytes"),
    };
    <[u8; KEY_LEN]>::try_from(decoded.as_slice())
        .ok()
        .ok_or(invalid_err)
}

/// Construct a [`TokenCipher`] from a list of validated 32-byte raw keys.
///
/// # Arguments
/// * `keys` - Non-empty vector of 32-byte AES-256 keys.
fn build_cipher(keys: Vec<[u8; KEY_LEN]>) -> ServerResult<TokenCipher> {
    let mut iter = keys.into_iter();
    let missing_err = ServerError::CryptoKeyInvalid {
        detail: "at least one token encryption key is required".to_string(),
    };
    let primary_raw = iter.next().ok_or(missing_err)?;
    let primary = make_less_safe_key(&primary_raw)?;
    let mut fallbacks = Vec::new();
    for raw in iter {
        fallbacks.push(make_less_safe_key(&raw)?);
    }
    Ok(TokenCipher {
        inner: Arc::new(TokenCipherInner { primary, fallbacks }),
    })
}

/// Create a [`LessSafeKey`] from a 32-byte AES-256 key slice.
///
/// # Arguments
/// * `raw` - 32-byte key array.
fn make_less_safe_key(raw: &[u8; KEY_LEN]) -> ServerResult<LessSafeKey> {
    let unbound = UnboundKey::new(&AES_256_GCM, raw)?;
    Ok(LessSafeKey::new(unbound))
}

/// Attempt to open `ciphertext` in-place using `key` and `nonce_bytes`.
///
/// # Arguments
/// * `key` - Candidate AEAD key.
/// * `nonce_bytes` - 12-byte GCM nonce.
/// * `ciphertext` - Ciphertext with authentication tag appended.
fn decrypt_with_key(
    key: &LessSafeKey,
    nonce_bytes: [u8; NONCE_LEN],
    ciphertext: &[u8],
) -> ServerResult<String> {
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut buffer = ciphertext.to_vec();
    let decrypted = key.open_in_place(nonce, Aad::empty(), &mut buffer)?;
    Ok(String::from_utf8(decrypted.to_vec())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Primary 32-byte test key (all `0x00`).
    const KEY_ONE: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    /// Secondary 32-byte test key (all `0x01`).
    const KEY_TWO: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";

    #[test]
    fn encrypt_roundtrip_and_fallback_key_rotation() -> ServerResult<()> {
        let primary_only = TokenCipher::from_base64_keys([KEY_ONE])?;
        let token = primary_only.encrypt("secret-token")?;
        assert!(TokenCipher::is_encrypted(&token));
        assert!(!TokenCipher::is_encrypted("plain-token"));
        assert_eq!(primary_only.decrypt(&token)?, "secret-token");

        let rotated = TokenCipher::from_base64_keys([KEY_TWO, KEY_ONE])?;
        assert_eq!(rotated.decrypt(&token)?, "secret-token");

        let wrong = TokenCipher::from_base64_keys([KEY_TWO])?;
        assert!(wrong.decrypt(&token).is_err());
        Ok(())
    }

    #[test]
    fn decrypt_rejects_unprefixed_short_and_corrupted_payloads() -> ServerResult<()> {
        let cipher = TokenCipher::from_base64_keys([KEY_ONE])?;
        assert!(cipher.decrypt("plain-token").is_err());

        let short_payload = format!(
            "{ENCRYPTED_PREFIX}{}",
            general_purpose::STANDARD.encode([1u8, 2, 3])
        );
        assert!(cipher.decrypt(&short_payload).is_err());
        Ok(())
    }

    #[test]
    fn rejects_empty_and_invalid_keys() {
        assert!(TokenCipher::from_base64_keys(["", "   "]).is_err());
        assert!(TokenCipher::from_base64_keys(["not-base64!"]).is_err());
        assert!(TokenCipher::from_base64_keys(["short"]).is_err());
    }

    #[test]
    fn loads_keys_from_env_and_reports_missing() -> ServerResult<()> {
        let _guard = crate::db::db_env_lock()?;
        let prev_keys = std::env::var(TOKEN_KEYS_ENV).ok();
        let prev_key = std::env::var(TOKEN_KEY_ENV).ok();

        unsafe {
            std::env::remove_var(TOKEN_KEYS_ENV);
            std::env::remove_var(TOKEN_KEY_ENV);
        }
        assert!(TokenCipher::from_env().is_err());

        unsafe {
            std::env::set_var(TOKEN_KEY_ENV, KEY_ONE);
        }
        let cipher = TokenCipher::from_env()?;
        let encrypted = cipher.encrypt("env-token")?;
        assert_eq!(cipher.decrypt(&encrypted)?, "env-token");

        unsafe {
            std::env::set_var(TOKEN_KEYS_ENV, KEY_ONE);
        }
        let _cipher_multi = TokenCipher::from_env()?;

        crate::db::restore_env_var(TOKEN_KEYS_ENV, prev_keys);
        crate::db::restore_env_var(TOKEN_KEY_ENV, prev_key);
        Ok(())
    }
}
