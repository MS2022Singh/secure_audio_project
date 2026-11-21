//! Device ID = SHA-256 of the Windows MachineGuid: stable across user accounts and
//! reboots, different on every Windows installation.
use sha2::{Digest, Sha256};

#[allow(unused_assignments)]
pub fn device_id() -> String {
    let mut source = String::new();
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
        if let Ok(k) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags("SOFTWARE\\Microsoft\\Cryptography", KEY_READ | KEY_WOW64_64KEY)
        {
            if let Ok(guid) = k.get_value::<String, _>("MachineGuid") {
                source = guid;
            }
        }
    }
    #[cfg(not(windows))]
    {
        // Android: a random ID created on first start and kept in the app's private storage
        // (removed only when the app is uninstalled or its data is cleared).
        let path = crate::state::base_dir().join("device_id");
        source = std::fs::read_to_string(&path).unwrap_or_default().trim().to_string();
        if source.is_empty() {
            let mut b = [0u8; 16];
            if getrandom::getrandom(&mut b).is_ok() {
                source = b.iter().map(|x| format!("{:02x}", x)).collect();
                let _ = std::fs::create_dir_all(crate::state::base_dir());
                let _ = std::fs::write(&path, &source);
            }
        }
    }
    if source.is_empty() {
        source = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "UNKNOWN".into());
    }
    let digest = Sha256::digest(format!("DIVIORA-HWID-v2|{}", source.trim().to_lowercase()).as_bytes());
    let hex: String = digest.iter().map(|b| format!("{:02X}", b)).collect();
    format!("DIVIORA-HWID-{}", &hex[..32])
}
