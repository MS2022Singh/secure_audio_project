//! Developer (master) password, recovery key, device ID and local licence cache.
//!
//! * The developer password is BUILT INTO the app as a salted PBKDF2-HMAC-SHA256 hash
//!   (600 000 iterations) from `src-tauri/developer_access.json`. Only the developer knows the
//!   password; a user cannot create or reset a developer password on their own device.
//!   To change it: tools/set-developer-password.ps1, commit the new file, publish an update.
//! * (A build without that file falls back to a per-device password; not used for releases.)
//! * Five wrong attempts lock the login for 60 seconds, recorded on disk so restarting the
//!   app does not reset the count.
//! * Device ID = SHA-256 of the Windows MachineGuid (stable across user accounts and
//!   reboots, changes on a Windows reinstall), not the computer/user name.

use sha2::Sha256;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PBKDF2_ROUNDS: u32 = 600_000;
const MAX_FAILURES: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(60);

pub struct SecurityManager;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct StoredSecret {
    version: u32,
    rounds: u32,
    salt: String,          // hex
    password_hash: String, // hex
    recovery_salt: String,
    recovery_hash: String,
}

/// The developer's password verifier, compiled into the app.
const BUILT_IN_ACCESS: &str = include_str!("../../developer_access.json");

fn built_in() -> Option<StoredSecret> {
    let s: StoredSecret = serde_json::from_str(BUILT_IN_ACCESS).ok()?;
    (!s.password_hash.is_empty() && !s.salt.is_empty()).then_some(s)
}

struct Attempts {
    failures: u32,
    locked_until: Option<Instant>,
}
static ATTEMPTS: Mutex<Attempts> = Mutex::new(Attempts { failures: 0, locked_until: None });

/// Failed attempts survive an app restart (otherwise closing and reopening the app would
/// give unlimited guesses).
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct LockFile {
    failures: u32,
    locked_until_unix: u64,
}
fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("OS random generator unavailable");
    b
}

/// This device's changed developer password: its hash, and the built-in secret encrypted
/// with a key derived from it (see `secret_via_override`).
#[derive(serde::Serialize, serde::Deserialize)]
struct DevOverride {
    version: u32,
    rounds: u32,
    salt: String,
    password_hash: String,
    key_salt: String,
    enc_secret: String,
}

fn derive_len(secret: &str, salt: &[u8], rounds: u32, len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    pbkdf2::pbkdf2_hmac::<Sha256>(secret.as_bytes(), salt, rounds, &mut out);
    out
}

fn derive(secret: &str, salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(secret.as_bytes(), salt, rounds, &mut out);
    out
}

/// Constant-time comparison.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn check(secret: &str, salt_hex: &str, hash_hex: &str, rounds: u32) -> bool {
    match (unhex(salt_hex), unhex(hash_hex)) {
        (Some(salt), Some(hash)) => same(&derive(secret, &salt, rounds), &hash),
        _ => false,
    }
}

/// Recovery key like "DVR-7F3K-Q9MZ-2XWB-HT6P-RA4C" (no easily confused characters).
fn new_recovery_key() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let raw = random_bytes::<20>();
    let chars: String = raw.iter().map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect();
    format!("DVR-{}-{}-{}-{}-{}", &chars[0..4], &chars[4..8], &chars[8..12], &chars[12..16], &chars[16..20])
}

fn validate_new_password(p: &str) -> Result<(), String> {
    if p.chars().count() < 10 {
        return Err("The new password must be at least 10 characters.".into());
    }
    let classes = [p.chars().any(|c| c.is_lowercase()), p.chars().any(|c| c.is_uppercase()),
                   p.chars().any(|c| c.is_ascii_digit()), p.chars().any(|c| !c.is_alphanumeric())];
    if classes.iter().filter(|x| **x).count() < 3 {
        return Err("Use at least three of: lowercase, uppercase, digits, symbols.".into());
    }
    Ok(())
}

impl SecurityManager {
    pub fn get_sec_dir() -> PathBuf {
        let p = diviora_client::state::base_dir();
        let _ = fs::create_dir_all(&p);
        p
    }

    fn secret_file() -> PathBuf {
        Self::get_sec_dir().join("master_sec.json")
    }

    fn lock_file() -> PathBuf {
        Self::get_sec_dir().join("dev_lock.json")
    }

    fn load() -> Option<StoredSecret> {
        built_in().or_else(|| serde_json::from_str(&fs::read_to_string(Self::secret_file()).ok()?).ok())
    }

    /// True when the developer password is compiled into the app (release setup).
    pub fn password_is_built_in() -> bool {
        built_in().is_some()
    }

    pub fn is_password_set() -> bool {
        Self::load().is_some()
    }

    /// Stable per-machine ID (shared with the sync service).
    pub fn generate_hwid() -> String {
        diviora_client::device::device_id()
    }

    /// Ok(true) on success. Err while locked out or when no password has been set yet.
    pub fn verify_master_pass(pass: &str) -> Result<bool, String> {
        let mut disk: LockFile = fs::read_to_string(Self::lock_file()).ok()
            .and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        {
            let a = ATTEMPTS.lock().unwrap();
            if let Some(until) = a.locked_until {
                if Instant::now() < until {
                    return Err(format!("Too many attempts. Try again in {} s.", (until - Instant::now()).as_secs() + 1));
                }
            }
        }
        if disk.locked_until_unix > unix_now() {
            return Err(format!("Too many attempts. Try again in {} s.", disk.locked_until_unix - unix_now()));
        }
        let stored = Self::load().ok_or_else(|| {
            "No developer password is set yet. Click \"Reset Password\" and leave the current-password box empty to create one.".to_string()
        })?;
        let ok = Self::password_matches(pass, &stored);
        let mut a = ATTEMPTS.lock().unwrap();
        if ok {
            a.failures = 0;
            a.locked_until = None;
            disk = LockFile::default();
        } else {
            a.failures += 1;
            disk.failures += 1;
            if a.failures >= MAX_FAILURES || disk.failures >= MAX_FAILURES {
                a.failures = 0;
                a.locked_until = Some(Instant::now() + LOCKOUT);
                disk.failures = 0;
                disk.locked_until_unix = unix_now() + LOCKOUT.as_secs();
            }
        }
        if let Ok(t) = serde_json::to_string(&disk) {
            let _ = fs::write(Self::lock_file(), t);
        }
        Ok(ok)
    }

    fn override_file() -> PathBuf {
        Self::get_sec_dir().join("dev_override.json")
    }

    fn load_override() -> Option<DevOverride> {
        serde_json::from_str(&fs::read_to_string(Self::override_file()).ok()?).ok()
    }

    /// The developer's built-in secret (password or recovery key) unlocked by `pass` through
    /// this device's override, if `pass` is the override password.
    fn secret_via_override(pass: &str, stored: &StoredSecret) -> Option<String> {
        let ov = Self::load_override()?;
        if !check(pass, &ov.salt, &ov.password_hash, ov.rounds) {
            return None;
        }
        let enc = unhex(&ov.enc_secret)?;
        let key = derive_len(pass, &unhex(&ov.key_salt)?, ov.rounds, enc.len());
        let secret = String::from_utf8(enc.iter().zip(key.iter()).map(|(a, b)| a ^ b).collect()).ok()?;
        // Only valid if it really is the built-in password or recovery key: a user cannot
        // create an override without knowing one of them.
        let genuine = check(&secret, &stored.salt, &stored.password_hash, stored.rounds)
            || check(&secret, &stored.recovery_salt, &stored.recovery_hash, stored.rounds);
        genuine.then_some(secret)
    }

    /// Login check: this device's changed password (if any), the built-in password (only
    /// while this device has no changed password), or the built-in recovery key.
    fn password_matches(pass: &str, stored: &StoredSecret) -> bool {
        let pass = pass.trim();
        if pass.is_empty() {
            return false;
        }
        let recovery = check(&pass.to_uppercase(), &stored.recovery_salt, &stored.recovery_hash, stored.rounds);
        if Self::load_override().is_some() {
            return Self::secret_via_override(pass, stored).is_some() || recovery;
        }
        check(pass, &stored.salt, &stored.password_hash, stored.rounds) || recovery
    }

    /// Change the developer password on this device, effective immediately. `current` is the
    /// current password (or recovery key). Also returns the built-in access file for the
    /// next app version (so the new password then works on every device) and its new
    /// recovery key.
    pub fn change_password_here(current: &str, new_pass: &str) -> Result<(String, String), String> {
        let stored = Self::load().ok_or("No developer password is configured in this build.")?;
        Self::change_with(&stored, current, new_pass)
    }

    fn change_with(stored: &StoredSecret, current: &str, new_pass: &str) -> Result<(String, String), String> {
        let stored = stored.clone();
        let current = current.trim();
        let secret = if check(current, &stored.salt, &stored.password_hash, stored.rounds) && Self::load_override().is_none() {
            current.to_string()
        } else if check(&current.to_uppercase(), &stored.recovery_salt, &stored.recovery_hash, stored.rounds) {
            current.to_uppercase()
        } else if let Some(s) = Self::secret_via_override(current, &stored) {
            s
        } else {
            return Err("The current password (or recovery key) is wrong.".into());
        };
        validate_new_password(new_pass)?;
        let salt = random_bytes::<16>();
        let key_salt = random_bytes::<16>();
        let key = derive_len(new_pass, &key_salt, PBKDF2_ROUNDS, secret.len());
        let ov = DevOverride {
            version: 1,
            rounds: PBKDF2_ROUNDS,
            salt: hex(&salt),
            password_hash: hex(&derive(new_pass, &salt, PBKDF2_ROUNDS)),
            key_salt: hex(&key_salt),
            enc_secret: hex(&secret.as_bytes().iter().zip(key.iter()).map(|(a, b)| a ^ b).collect::<Vec<u8>>()),
        };
        fs::write(Self::override_file(), serde_json::to_string_pretty(&ov).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Cannot save the new password: {}", e))?;
        Self::make_access_file(new_pass)
    }

    /// Developer password change (developer panel): returns the new built-in access file
    /// (salted hashes only, never the password) and a new recovery key. The file must be
    /// built into the next app version to take effect on every device.
    pub fn make_access_file(new_pass: &str) -> Result<(String, String), String> {
        validate_new_password(new_pass)?;
        let salt = random_bytes::<16>();
        let recovery = new_recovery_key();
        let recovery_salt = random_bytes::<16>();
        let record = StoredSecret {
            version: 2,
            rounds: PBKDF2_ROUNDS,
            salt: hex(&salt),
            password_hash: hex(&derive(new_pass, &salt, PBKDF2_ROUNDS)),
            recovery_salt: hex(&recovery_salt),
            recovery_hash: hex(&derive(&recovery, &recovery_salt, PBKDF2_ROUNDS)),
        };
        let json = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())? + "\n";
        Ok((json, recovery))
    }

    /// Set the first password (old_pass empty), or change it with the current password
    /// or the recovery key. Returns a message that includes a NEW recovery key.
    pub fn reset_master_pass(old_pass: &str, new_pass: &str) -> Result<String, String> {
        if Self::password_is_built_in() {
            return Err("The developer password is built into the app, so it cannot be created or changed on this device. \
                        The developer changes it with tools/set-developer-password.ps1 and publishes an update.".into());
        }
        validate_new_password(new_pass)?;
        if let Some(stored) = Self::load() {
            let by_password = !old_pass.is_empty() && check(old_pass, &stored.salt, &stored.password_hash, stored.rounds);
            let by_recovery = !old_pass.is_empty()
                && check(old_pass.trim().to_uppercase().as_str(), &stored.recovery_salt, &stored.recovery_hash, stored.rounds);
            if !by_password && !by_recovery {
                return Err("Current password or recovery key is wrong.".into());
            }
        } else if !old_pass.is_empty() {
            return Err("No password is set yet: leave the current-password box empty to create the first one.".into());
        }

        let salt = random_bytes::<16>();
        let recovery = new_recovery_key();
        let recovery_salt = random_bytes::<16>();
        let record = StoredSecret {
            version: 2,
            rounds: PBKDF2_ROUNDS,
            salt: hex(&salt),
            password_hash: hex(&derive(new_pass, &salt, PBKDF2_ROUNDS)),
            recovery_salt: hex(&recovery_salt),
            recovery_hash: hex(&derive(&recovery, &recovery_salt, PBKDF2_ROUNDS)),
        };
        let json = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
        fs::write(Self::secret_file(), json).map_err(|e| format!("Cannot save password: {}", e))?;
        let _ = fs::remove_file(Self::get_sec_dir().join("master_sec.hash")); // old unsalted SHA-256 file
        Ok(format!(
            "Developer password saved. RECOVERY KEY (shown only once, write it down and keep it offline): {}",
            recovery
        ))
    }

    /// Local cache of "one licence ID = one device". The licensing server is authoritative
    /// when configured (see commands::authenticate_user_session).
    pub fn verify_session_exclusivity(user_id: &str, hwid: &str) -> Result<bool, String> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Err("Enter a licence key.".into());
        }
        let db_file = Self::get_sec_dir().join("license.db");
        let content = fs::read_to_string(&db_file).unwrap_or_default();
        for line in content.lines() {
            if let Some((id, dev)) = line.strip_prefix("ID:").and_then(|r| r.split_once(",HWID:")) {
                if id == user_id {
                    return if dev == hwid {
                        Ok(true)
                    } else {
                        Err("Licence error: this licence key is already bound to another device (one ID = one device).".into())
                    };
                }
            }
        }
        let mut updated = content;
        updated.push_str(&format!("ID:{},HWID:{}\n", user_id, hwid));
        fs::write(&db_file, updated).map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_password_works_here_and_cannot_be_forged() {
        let dir = std::env::temp_dir().join(format!("diviora-sec-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        diviora_client::state::set_base_dir(dir.clone());
        let _ = fs::remove_file(SecurityManager::override_file());
        // A made-up "built-in" secret for the test.
        let (salt, rsalt) = ([1u8; 16], [2u8; 16]);
        let stored = StoredSecret {
            version: 2, rounds: 1000, salt: hex(&salt), password_hash: hex(&derive("Old-Passw0rd!x", &salt, 1000)),
            recovery_salt: hex(&rsalt), recovery_hash: hex(&derive("DVR-AAAA-BBBB-CCCC-DDDD-EEEE", &rsalt, 1000)),
        };
        assert!(SecurityManager::password_matches("Old-Passw0rd!x", &stored));
        assert!(SecurityManager::password_matches("dvr-aaaa-bbbb-cccc-dddd-eeee", &stored)); // recovery key logs in
        assert!(SecurityManager::change_with(&stored, "wrong", "New-Passw0rd!y").is_err());
        SecurityManager::change_with(&stored, "Old-Passw0rd!x", "New-Passw0rd!y").unwrap();
        assert!(SecurityManager::password_matches("New-Passw0rd!y", &stored)); // works immediately
        assert!(!SecurityManager::password_matches("Old-Passw0rd!x", &stored)); // old one retired here
        assert!(SecurityManager::password_matches("DVR-AAAA-BBBB-CCCC-DDDD-EEEE", &stored));
        // Change again, using the changed password as the current one.
        SecurityManager::change_with(&stored, "New-Passw0rd!y", "Third-Passw0rd!z").unwrap();
        assert!(SecurityManager::password_matches("Third-Passw0rd!z", &stored));
        assert!(!SecurityManager::password_matches("New-Passw0rd!y", &stored));
        // A user writing their own override file (without the real secret) gets nothing.
        let fake_salt = [3u8; 16];
        let forged = DevOverride {
            version: 1, rounds: 1000, salt: hex(&fake_salt), password_hash: hex(&derive("Hacker-Passw0rd!", &fake_salt, 1000)),
            key_salt: hex(&[4u8; 16]), enc_secret: hex(b"guess-the-password"),
        };
        fs::write(SecurityManager::override_file(), serde_json::to_string(&forged).unwrap()).unwrap();
        assert!(!SecurityManager::password_matches("Hacker-Passw0rd!", &stored));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hashing_and_policy() {
        let salt = [7u8; 16];
        let h = hex(&derive("Correct-Horse-9", &salt, 1000));
        assert!(check("Correct-Horse-9", &hex(&salt), &h, 1000));
        assert!(!check("correct-horse-9", &hex(&salt), &h, 1000));
        assert!(validate_new_password("short").is_err());
        assert!(validate_new_password("alllowercaseletters").is_err());
        assert!(validate_new_password("Diviora-Dev-2026").is_ok());
        let k = new_recovery_key();
        assert!(k.starts_with("DVR-") && k.len() == 28);
    }
}
