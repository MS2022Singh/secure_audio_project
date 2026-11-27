//! End-to-end encryption of configurations published by the developer.
//!
//! * Every activated device has its own X25519 key pair. The private key never leaves the
//!   device (Windows: DPAPI, machine scope; Android: the app's private storage). The public
//!   key is sent to the licence server at activation, bound to the licence (one licence = one
//!   device).
//! * Publishing: the developer app picks a random content key, encrypts the configuration
//!   with it (AES-256-GCM) and wraps the content key separately for each activated device
//!   (X25519 + HKDF-SHA256 + AES-256-GCM). The licence server only stores ciphertext and
//!   cannot read the configuration; a device can only open what was wrapped for it.
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

const CONFIG_AAD: &[u8] = b"diviora-config-v1";
const WRAP_INFO: &[u8] = b"diviora-wrap-v1";

pub fn random32() -> [u8; 32] {
    let mut k = [0u8; 32];
    getrandom::getrandom(&mut k).expect("system random generator");
    k
}

fn gcm(key: &[u8; 32]) -> Aes256Gcm {
    Aes256Gcm::new_from_slice(key).expect("32-byte key")
}

/// nonce(12) || AES-256-GCM(content_key, text)
pub fn seal_config(text: &str, content_key: &[u8; 32]) -> Vec<u8> {
    let nonce: [u8; 12] = random32()[..12].try_into().unwrap();
    let ct = gcm(content_key).encrypt(Nonce::from_slice(&nonce), Payload { msg: text.as_bytes(), aad: CONFIG_AAD })
        .expect("encryption");
    [nonce.to_vec(), ct].concat()
}

pub fn open_config(sealed: &[u8], content_key: &[u8; 32]) -> Result<String, String> {
    if sealed.len() < 28 { return Err("damaged configuration".into()); }
    let pt = gcm(content_key)
        .decrypt(Nonce::from_slice(&sealed[..12]), Payload { msg: &sealed[12..], aad: CONFIG_AAD })
        .map_err(|_| "This configuration was not encrypted for this device.".to_string())?;
    String::from_utf8(pt).map_err(|e| e.to_string())
}

fn wrap_key(shared: &[u8; 32], eph: &[u8; 32], dev: &[u8; 32]) -> [u8; 32] {
    let mut salt = eph.to_vec();
    salt.extend_from_slice(dev);
    let mut k = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&salt), shared).expand(WRAP_INFO, &mut k).expect("hkdf");
    k
}

/// eph_pub(32) || nonce(12) || AES-256-GCM(kek, content_key)
pub fn wrap_for_device(content_key: &[u8; 32], device_public: &[u8; 32]) -> Vec<u8> {
    let eph = StaticSecret::from(random32());
    let eph_pub = PublicKey::from(&eph).to_bytes();
    let shared = eph.diffie_hellman(&PublicKey::from(*device_public)).to_bytes();
    let kek = wrap_key(&shared, &eph_pub, device_public);
    let nonce: [u8; 12] = random32()[..12].try_into().unwrap();
    let ct = gcm(&kek).encrypt(Nonce::from_slice(&nonce), Payload { msg: content_key, aad: WRAP_INFO }).expect("wrap");
    [eph_pub.to_vec(), nonce.to_vec(), ct].concat()
}

pub fn unwrap_on_device(wrapped: &[u8], device_secret: &[u8; 32]) -> Result<[u8; 32], String> {
    if wrapped.len() < 32 + 12 + 16 + 32 { return Err("damaged key".into()); }
    let eph_pub: [u8; 32] = wrapped[..32].try_into().unwrap();
    let secret = StaticSecret::from(*device_secret);
    let dev_pub = PublicKey::from(&secret).to_bytes();
    let shared = secret.diffie_hellman(&PublicKey::from(eph_pub)).to_bytes();
    let kek = wrap_key(&shared, &eph_pub, &dev_pub);
    let ck = gcm(&kek)
        .decrypt(Nonce::from_slice(&wrapped[32..44]), Payload { msg: &wrapped[44..], aad: WRAP_INFO })
        .map_err(|_| "This configuration was not encrypted for this device.".to_string())?;
    ck.try_into().map_err(|_| "damaged key".to_string())
}

pub fn b64(b: &[u8]) -> String { B64.encode(b) }
pub fn unb64(s: &str) -> Result<Vec<u8>, String> { B64.decode(s.trim()).map_err(|e| e.to_string()) }
pub fn key32(s: &str) -> Result<[u8; 32], String> {
    unb64(s)?.try_into().map_err(|_| "bad key length".to_string())
}

// ---------------------------------------------------------------- secrets at rest

fn store(path: &std::path::Path, secret_b64: &str, machine: bool) -> Result<(), String> {
    if let Some(d) = path.parent() { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
    #[cfg(windows)]
    let bytes = crate::apo_config::protect_secret(secret_b64, machine)?;
    #[cfg(not(windows))]
    let bytes = { let _ = machine; secret_b64.as_bytes().to_vec() };
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

fn load(path: &std::path::Path, machine: bool) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    #[cfg(windows)]
    { crate::apo_config::unprotect_secret(&bytes, machine).ok() }
    #[cfg(not(windows))]
    { let _ = machine; String::from_utf8(bytes).ok() }
}

fn device_key_path() -> std::path::PathBuf { crate::state::client_dir().join("device_key.dvk") }

/// This device's private key, created on first use.
pub fn device_secret() -> Result<[u8; 32], String> {
    let p = device_key_path();
    if let Some(k) = load(&p, true).and_then(|s| key32(&s).ok()) {
        return Ok(k);
    }
    let k = random32();
    store(&p, &b64(&k), true)?;
    Ok(k)
}

/// Public key sent to the licence server at activation (base64).
pub fn device_public_b64() -> Result<String, String> {
    Ok(b64(&PublicKey::from(&StaticSecret::from(device_secret()?)).to_bytes()))
}

/// Developer machine: content key of the latest published version, so devices activated later
/// can be given access without publishing again. User-scope protection.
fn publish_key_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(|_| crate::state::base_dir());
    base.join("Diviora").join("published_key.dvk")
}
pub fn save_publish_key(version: i64, content_key: &[u8; 32]) -> Result<(), String> {
    store(&publish_key_path(), &format!("{}:{}", version, b64(content_key)), false)
}
pub fn load_publish_key() -> Option<(i64, [u8; 32])> {
    let s = load(&publish_key_path(), false)?;
    let (v, k) = s.split_once(':')?;
    Some((v.parse().ok()?, key32(k).ok()?))
}
pub fn forget_publish_key() { let _ = std::fs::remove_file(publish_key_path()); }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_device_it_was_wrapped_for_can_open_it() {
        let dev_a = random32();
        let dev_b = random32();
        let pub_a = PublicKey::from(&StaticSecret::from(dev_a)).to_bytes();
        let ck = random32();
        let sealed = seal_config("Preamp: -3 dB", &ck);
        assert!(!String::from_utf8_lossy(&sealed).contains("Preamp"));
        let w = wrap_for_device(&ck, &pub_a);
        let got = unwrap_on_device(&w, &dev_a).unwrap();
        assert_eq!(open_config(&sealed, &got).unwrap(), "Preamp: -3 dB");
        assert!(unwrap_on_device(&w, &dev_b).is_err());
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(open_config(&tampered, &ck).is_err());
    }
}
