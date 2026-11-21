//! Developer (master) password, recovery key, device ID and local licence cache.
//!
//! * No built-in or default passwords. The first password is set by the developer on
//!   first use; a one-time recovery key is shown at that moment (store it offline).
//! * Passwords and recovery keys are stored only as salted PBKDF2-HMAC-SHA256 hashes
//!   (600 000 iterations) in %ProgramData%\Diviora\master_sec.json.
//! * Five wrong attempts lock the login for 60 seconds.
//! * Device ID = SHA-256 of the Windows MachineGuid (stable across user accounts and
//!   reboots, changes on a Windows reinstall), not the computer/user name.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PBKDF2_ROUNDS: u32 = 600_000;
const MAX_FAILURES: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(60);

pub struct SecurityManager;

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredSecret {
    version: u32,
    rounds: u32,
    salt: String,          // hex
    password_hash: String, // hex
    recovery_salt: String,
    recovery_hash: String,
}

struct Attempts {
    failures: u32,
    locked_until: Option<Instant>,
}
static ATTEMPTS: Mutex<Attempts> = Mutex::new(Attempts { failures: 0, locked_until: None });

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
        let pd = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
        let p = PathBuf::from(pd).join("Diviora");
        let _ = fs::create_dir_all(&p);
        p
    }

    fn secret_file() -> PathBuf {
        Self::get_sec_dir().join("master_sec.json")
    }

    fn load() -> Option<StoredSecret> {
        serde_json::from_str(&fs::read_to_string(Self::secret_file()).ok()?).ok()
    }

    pub fn is_password_set() -> bool {
        Self::load().is_some()
    }

    /// Stable per-machine ID.
    pub fn generate_hwid() -> String {
        let mut source = String::new();
        #[cfg(windows)]
        {
            if let Ok(k) = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
                .open_subkey_with_flags("SOFTWARE\\Microsoft\\Cryptography", winreg::enums::KEY_READ | winreg::enums::KEY_WOW64_64KEY)
            {
                if let Ok(guid) = k.get_value::<String, _>("MachineGuid") {
                    source = guid;
                }
            }
        }
        if source.is_empty() {
            source = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "UNKNOWN".into());
        }
        let digest = Sha256::digest(format!("DIVIORA-HWID-v2|{}", source.trim().to_lowercase()).as_bytes());
        format!("DIVIORA-HWID-{}", hex(&digest)[..32].to_uppercase())
    }

    /// Ok(true) on success. Err while locked out or when no password has been set yet.
    pub fn verify_master_pass(pass: &str) -> Result<bool, String> {
        {
            let a = ATTEMPTS.lock().unwrap();
            if let Some(until) = a.locked_until {
                if Instant::now() < until {
                    return Err(format!("Too many attempts. Try again in {} s.", (until - Instant::now()).as_secs() + 1));
                }
            }
        }
        let stored = Self::load().ok_or_else(|| {
            "No developer password is set yet. Click \"Reset Password\" and leave the current-password box empty to create one.".to_string()
        })?;
        let ok = check(pass, &stored.salt, &stored.password_hash, stored.rounds);
        let mut a = ATTEMPTS.lock().unwrap();
        if ok {
            a.failures = 0;
            a.locked_until = None;
        } else {
            a.failures += 1;
            if a.failures >= MAX_FAILURES {
                a.failures = 0;
                a.locked_until = Some(Instant::now() + LOCKOUT);
            }
        }
        Ok(ok)
    }

    /// Set the first password (old_pass empty), or change it with the current password
    /// or the recovery key. Returns a message that includes a NEW recovery key.
    pub fn reset_master_pass(old_pass: &str, new_pass: &str) -> Result<String, String> {
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
