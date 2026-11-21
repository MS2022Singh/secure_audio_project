//! Device ID = SHA-256 of the Windows MachineGuid: stable across user accounts and
//! reboots, different on every Windows installation.
use sha2::{Digest, Sha256};

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
    if source.is_empty() {
        source = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "UNKNOWN".into());
    }
    let digest = Sha256::digest(format!("DIVIORA-HWID-v2|{}", source.trim().to_lowercase()).as_bytes());
    let hex: String = digest.iter().map(|b| format!("{:02X}", b)).collect();
    format!("DIVIORA-HWID-{}", &hex[..32])
}
