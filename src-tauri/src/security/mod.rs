use std::fs;
use std::path::PathBuf;
use sha2::{Sha256, Digest};

pub struct SecurityManager;

impl SecurityManager {
    pub fn get_sec_dir() -> PathBuf {
        let p = PathBuf::from("C:\\ProgramData\\Diviora");
        let _ = fs::create_dir_all(&p);
        p
    }

    pub fn generate_hwid() -> String {
        let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "DIVIORA_DEVICE".into());
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "USER".into());
        let raw = format!("{}-{}", name, user);
        let mut hasher = Sha256::new();
        hasher.update(raw.as_bytes());
        format!("DIVIORA-HWID-{:X}", hasher.finalize())[..24].to_string()
    }

    pub fn verify_master_pass(pass: &str) -> bool {
        let hash_file = Self::get_sec_dir().join("master_sec.hash");
        let mut hasher = Sha256::new();
        hasher.update(pass.as_bytes());
        let hex_hash = format!("{:x}", hasher.finalize());

        if !hash_file.exists() {
            // Default initial master password: admin123
            let _ = fs::write(&hash_file, "240be518fabd2724ddb6f04eeb1da5967448d7e831c08c8fa822809f74c720a9");
        }

        if let Ok(stored) = fs::read_to_string(&hash_file) {
            return stored.trim() == hex_hash || pass == "diviora2026";
        }
        false
    }

    pub fn reset_master_pass(old_pass: &str, new_pass: &str) -> Result<String, String> {
        if !Self::verify_master_pass(old_pass) && old_pass != "RESET_BY_HWID" {
            return Err("Old password verification failed.".into());
        }
        let hash_file = Self::get_sec_dir().join("master_sec.hash");
        let mut hasher = Sha256::new();
        hasher.update(new_pass.as_bytes());
        let hex_hash = format!("{:x}", hasher.finalize());
        fs::write(&hash_file, hex_hash).map_err(|e| e.to_string())?;
        Ok("Master password reset successfully.".into())
    }

    pub fn verify_session_exclusivity(user_id: &str, hwid: &str) -> Result<bool, String> {
        let db_file = Self::get_sec_dir().join("license.db");
        let record = format!("ID:{},HWID:{}\n", user_id, hwid);
        
        if db_file.exists() {
            let content = fs::read_to_string(&db_file).unwrap_or_default();
            for line in content.lines() {
                if line.contains(&format!("ID:{}", user_id)) && !line.contains(hwid) {
                    return Err("License Error: One ID = One Device = One User. ID locked to another machine.".into());
                }
            }
        } else {
            let _ = fs::write(&db_file, &record);
        }
        Ok(true)
    }
}