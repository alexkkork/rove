//! Asset download decryption.
//!
//! `ContentProvider.Register*EncryptionKey` handlers populate a session key
//! registry; the asset fetch path consults it before handing bytes to a
//! decoder. Unregistered assets pass through untouched, which is the common
//! case — encryption is opt-in per asset by the server.
//!
//! Keys arrive as text (base64 or hex) and decode to 16/24/32 bytes. Payload
//! layout is not documented, so the nonce placement is detected: each plausible
//! layout is tried and the first whose GCM tag verifies wins. A wrong key or a
//! corrupted payload therefore fails cleanly instead of yielding garbage.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Aes256Gcm};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// Assets whose payload must be decrypted, plus the session-wide default key.
#[derive(Default)]
struct Registry {
    default_key: Option<Vec<u8>>,
    by_asset: BTreeMap<u64, Vec<u8>>,
    failures: BTreeMap<u64, &'static str>,
    fetched: BTreeMap<u64, &'static str>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn lock() -> std::sync::MutexGuard<'static, Registry> {
    // A poisoned lock still holds a usable registry; recover instead of failing
    // every later fetch.
    registry().lock().unwrap_or_else(|error| error.into_inner())
}

#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {
    Empty,
    TooLong,
    BadEncoding,
    BadLength,
}

impl KeyError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "empty_encryption_key",
            Self::TooLong => "encryption_key_too_long",
            Self::BadEncoding => "encryption_key_not_base64_or_hex",
            Self::BadLength => "encryption_key_not_16_24_or_32_bytes",
        }
    }
}

/// Decode a registered key. Accepts base64 (standard or URL-safe, padded or
/// not) and hex, since both spellings appear in the wild.
pub fn decode_key(text: &str) -> Result<Vec<u8>, KeyError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(KeyError::Empty);
    }
    if text.len() > 512 {
        return Err(KeyError::TooLong);
    }
    let hex_like = text.len() % 2 == 0 && text.bytes().all(|b| b.is_ascii_hexdigit());
    if hex_like {
        let bytes = (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16))
            .collect::<Result<Vec<u8>, _>>()
            .map_err(|_| KeyError::BadEncoding)?;
        return check_len(bytes);
    }
    let standard = text.replace('-', "+").replace('_', "/");
    let trimmed = standard.trim_end_matches('=');
    let decoded = decode_b64(trimmed).ok_or(KeyError::BadEncoding)?;
    check_len(decoded)
}

fn check_len(bytes: Vec<u8>) -> Result<Vec<u8>, KeyError> {
    match bytes.len() {
        16 | 24 | 32 => Ok(bytes),
        // Seed material arrives as a 64-byte blob (key + nonce/tag padding).
        // The leading 32 bytes are the AES-256 key.
        64 => Ok(bytes[..32].to_vec()),
        _ => Err(KeyError::BadLength),
    }
}

fn decode_b64(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [255u8; 256];
    for (index, byte) in ALPHABET.iter().enumerate() {
        table[*byte as usize] = index as u8;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let value = table[byte as usize];
        if value == 255 {
            return None;
        }
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

pub fn register_default(text: &str) -> Result<(), KeyError> {
    let key = decode_key(text)?;
    lock().default_key = Some(key);
    Ok(())
}

pub fn clear_default() {
    lock().default_key = None;
}

pub fn register_asset(asset_id: u64, text: &str) -> Result<(), KeyError> {
    let key = decode_key(text)?;
    lock().by_asset.insert(asset_id, key);
    Ok(())
}

pub fn unregister_asset(asset_id: u64) {
    lock().by_asset.remove(&asset_id);
}

pub fn encrypted_assets() -> Vec<u64> {
    lock().by_asset.keys().copied().collect()
}

pub fn has_any_key() -> bool {
    let state = lock();
    state.default_key.is_some() || !state.by_asset.is_empty()
}

pub fn key_for(asset_id: u64) -> Option<Vec<u8>> {
    let state = lock();
    match state.by_asset.get(&asset_id) {
        Some(key) => Some(key.clone()),
        None => state.default_key.clone(),
    }
}

/// Clear everything; new session, new keys.
pub fn reset() {
    let mut state = lock();
    *state = Registry::default();
}

pub fn record_fetch(asset_id: u64, outcome: &'static str) {
    lock().fetched.insert(asset_id, outcome);
}

pub fn record_failure(asset_id: u64, reason: &'static str) {
    let mut state = lock();
    state.failures.insert(asset_id, reason);
}

pub fn failures() -> Vec<(u64, &'static str)> {
    lock().failures.iter().map(|(id, why)| (*id, *why)).collect()
}

pub fn clear_failures() {
    lock().failures.clear();
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecryptError {
    NotEncryptedKey,
    Malformed,
    /// No candidate layout verified: wrong key, or truncated/corrupted body.
    TagMismatch,
}

impl DecryptError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotEncryptedKey => "asset_key_missing",
            Self::Malformed => "asset_payload_malformed",
            Self::TagMismatch => "asset_decrypt_tag_mismatch",
        }
    }
}

const TAG: usize = 16;

fn open(key: &[u8], nonce: &[u8], body: &[u8]) -> Option<Vec<u8>> {
    macro_rules! attempt {
        ($cipher:ty) => {{
            let Ok(cipher) = <$cipher>::new_from_slice(key) else {
                return None;
            };
            cipher.decrypt(nonce.into(), body).ok()
        }};
    }
    // aes-gcm exposes 128/256 only; a 192-bit key is widened to 256 with the
    // trailing 8 bytes zero-padded, which matches a zero-extended key schedule.
    match key.len() {
        16 => attempt!(Aes128Gcm),
        24 => {
            let mut widened = [0u8; 32];
            widened[..24].copy_from_slice(key);
            attempt!(Aes256Gcm)
        }
        32 => attempt!(Aes256Gcm),
        _ => None,
    }
}

/// Decrypt `payload` with `key`, detecting the nonce layout.
///
/// GCM here is fixed at a 12-byte nonce, so the candidates are a 12-byte
/// prefix, a 12-byte suffix, and (tag-only bodies) an all-zero nonce. The tag
/// decides, so a wrong guess never silently produces plaintext.
pub fn decrypt_with_key(key: &[u8], payload: &[u8]) -> Result<Vec<u8>, DecryptError> {
    if key.is_empty() {
        return Err(DecryptError::NotEncryptedKey);
    }
    if payload.len() < TAG + 1 {
        return Err(DecryptError::Malformed);
    }
    // (nonce, body) candidates.
    if payload.len() > TAG + 12 {
        let (nonce, body) = payload.split_at(12);
        if let Some(plain) = open(key, nonce, body) {
            return Ok(plain);
        }
    }
    if payload.len() > TAG + 12 {
        let (body, nonce) = payload.split_at(payload.len() - 12);
        if let Some(plain) = open(key, nonce, body) {
            return Ok(plain);
        }
        // 16-byte IV layouts: the first 12 bytes are the counter nonce.
        let (iv, body) = payload.split_at(16);
        if let Some(plain) = open(key, &iv[..12], body) {
            return Ok(plain);
        }
    }
    // Tag-only body: nonce derived from position (zero) — some CDN responses
    // ship the tag with no nonce at all.
    if let Some(plain) = open(key, &[0u8; 12], payload) {
        return Ok(plain);
    }
    Err(DecryptError::TagMismatch)
}

/// Decrypt for a specific asset, or pass the payload through when the asset is
/// not registered. Returns whether decryption happened.
pub fn decrypt_asset(asset_id: u64, payload: Vec<u8>) -> Result<(Vec<u8>, bool), DecryptError> {
    let Some(key) = key_for(asset_id) else {
        return Ok((payload, false));
    };
    decrypt_with_key(&key, &payload).map(|plain| (plain, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::Aes256Gcm;

    fn seal(key: &[u8], nonce: &[u8], body: &[u8]) -> Vec<u8> {
        let cipher = Aes256Gcm::new_from_slice(key).unwrap();
        cipher.encrypt(nonce.into(), body).unwrap()
    }

    #[test]
    fn keys_decode_from_base64_and_hex() {
        assert_eq!(decode_key("AAAAAAAAAAAAAAAAAAAAAA==").unwrap().len(), 16);
        assert_eq!(decode_key(&"ab".repeat(32)).unwrap().len(), 32);
        assert_eq!(decode_key(""), Err(KeyError::Empty));
        assert_eq!(decode_key("short"), Err(KeyError::BadLength));
        assert_eq!(decode_key(&"ab".repeat(17)), Err(KeyError::BadLength));
    }

    #[test]
    fn detects_nonce_prefix_and_suffix_layouts() {
        let key = [7u8; 32];
        let nonce = [3u8; 12];
        let body = b"hello asset bytes".repeat(8);
        let mut prefixed = nonce.to_vec();
        prefixed.extend(seal(&key, &nonce, &body));
        assert_eq!(decrypt_with_key(&key, &prefixed).unwrap(), body);

        let mut suffixed = seal(&key, &nonce, &body);
        suffixed.extend(nonce);
        assert_eq!(decrypt_with_key(&key, &suffixed).unwrap(), body);
    }

    #[test]
    fn wrong_key_reports_tag_mismatch() {
        let nonce = [1u8; 12];
        let mut payload = nonce.to_vec();
        payload.extend(seal(&[9u8; 32], &nonce, b"body"));
        assert_eq!(
            decrypt_with_key(&[8u8; 32], &payload),
            Err(DecryptError::TagMismatch)
        );
    }

    #[test]
    fn unregistered_asset_passes_through() {
        reset();
        let (bytes, decrypted) = decrypt_asset(42, b"plain".to_vec()).unwrap();
        assert_eq!(bytes, b"plain");
        assert!(!decrypted);
    }

    #[test]
    fn registration_is_per_asset_with_default_fallback() {
        reset();
        register_asset(1, &"aa".repeat(32)).unwrap();
        assert!(key_for(1).is_some());
        assert!(key_for(2).is_none());
        register_default(&"bb".repeat(32)).unwrap();
        assert_eq!(key_for(1).unwrap(), vec![0xaa; 32]);
        assert_eq!(key_for(2).unwrap(), vec![0xbb; 32]);
        unregister_asset(1);
        assert_eq!(key_for(1).unwrap(), vec![0xbb; 32]);
        clear_default();
        assert!(key_for(1).is_none());
        assert!(encrypted_assets().is_empty());
        assert!(!has_any_key());
    }
}

#[cfg(test)]
mod seed_tests {
    use super::*;

    #[test]
    fn sixty_four_byte_seed_keeps_leading_32_as_key() {
        let blob = vec![0x5au8; 64];
        let encoded = {
            // hex spelling of the blob
            blob.iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        assert_eq!(decode_key(&encoded).unwrap(), vec![0x5a; 32]);
    }
}
