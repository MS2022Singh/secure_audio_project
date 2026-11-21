//! Encrypted, ACL-locked hand-off of the master configuration to the native APO.
//!
//! Layout (created on first lock; needs the app to run as Administrator):
//!   %ProgramData%\Diviora\apo\              SYSTEM + Administrators: full, Windows audio (LOCAL SERVICE, app packages): read.
//!                                            Normal users and consumers: no access at all.
//!   %ProgramData%\Diviora\apo\config.dvr    "DVRC" + u32 version 1 + DPAPI blob (machine scope,
//!                                            so a copied file cannot be decrypted on another PC).
//!   %ProgramData%\Diviora\apo\status\       APO writes apo_status.json here (no secrets).
//!
//! The format must match windows-apo/src/ConfigManager.cpp.

use std::path::PathBuf;

pub const ENTROPY: &[u8] = b"Diviora.APO.Config.v1";
pub const MAGIC: &[u8; 4] = b"DVRC";
pub const FORMAT_VERSION: u32 = 1;

// SDDL: protected DACL, inherited by files created inside.
// LS = LOCAL SERVICE (audiodg); AC / S-1-15-2-2 = (restricted) app-package identities;
// S-1-5-80-2676... = NT SERVICE\\Audiosrv and S-1-5-33 = WRITE RESTRICTED: audiodg runs with a
// write-restricted token, so writes need one of these in addition to LOCAL SERVICE.
const APO_DIR_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;LS)(A;OICI;FR;;;AC)(A;OICI;FR;;;S-1-15-2-2)(A;OICI;FR;;;S-1-5-80-2676549577-1911656217-2625096541-4178041876-1366760775)";
// audiodg (LOCAL SERVICE) may modify status files; users may read the (non-secret) status.
const STATUS_DIR_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1301bf;;;LS)(A;OICI;0x1301bf;;;AC)(A;OICI;0x1301bf;;;S-1-15-2-2)(A;OICI;FR;;;BU)(A;OICI;0x1301bf;;;S-1-5-80-2676549577-1911656217-2625096541-4178041876-1366760775)(A;OICI;0x1301bf;;;S-1-5-33)";

pub fn base_dir() -> PathBuf {
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Diviora").join("apo")
}
pub fn config_path() -> PathBuf { base_dir().join("config.dvr") }
pub fn status_path() -> PathBuf { base_dir().join("status").join("apo_status.json") }

/// Bytes of a config.dvr file for the given (already encrypted) DPAPI blob.
pub fn frame(blob: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + blob.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(blob);
    out
}

/// Encrypt `text` and atomically replace config.dvr. An empty `text` means pass-through.
/// The APO picks the change up within about a second; no restart or reboot needed.
pub fn write_locked_config(text: &str) -> Result<PathBuf, String> {
    imp::write_locked_config(text)
}

/// Remove plaintext configs left by older builds (they exposed the master config to any user).
pub fn remove_legacy_plaintext() {
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
    for name in ["active.txt", "config.txt"] {
        let _ = std::fs::remove_file(PathBuf::from(&pd).join("Diviora").join(name));
    }
}

/// Status written by DivioraAPO.dll from inside audiodg.exe, if any.
pub fn read_status() -> Option<serde_json::Value> {
    let s = std::fs::read_to_string(status_path()).ok()?;
    serde_json::from_str(&s).ok()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR,
    };

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn apply_sddl(path: &std::path::Path, sddl: &str) -> Result<(), String> {
        let sddl_w = wide(std::ffi::OsStr::new(sddl));
        let path_w = wide(path.as_os_str());
        let mut psd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: plain Win32 calls with valid NUL-terminated buffers; psd freed below.
        unsafe {
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl_w.as_ptr(), SDDL_REVISION_1, &mut psd, std::ptr::null_mut()) == 0 {
                return Err("invalid security descriptor".into());
            }
            let (mut present, mut defaulted) = (0i32, 0i32);
            let mut dacl: *mut ACL = std::ptr::null_mut();
            let ok = GetSecurityDescriptorDacl(psd, &mut present, &mut dacl, &mut defaulted);
            let rc = if ok != 0 {
                SetNamedSecurityInfoW(
                    path_w.as_ptr(), SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(), std::ptr::null_mut(), dacl, std::ptr::null(),
                )
            } else {
                1
            };
            LocalFree(psd as _);
            if rc != ERROR_SUCCESS {
                return Err(format!(
                    "could not lock permissions on {} (error {}). Run Diviora as Administrator to lock the configuration.",
                    path.display(), rc
                ));
            }
        }
        Ok(())
    }

    fn protect(text: &str) -> Result<Vec<u8>, String> {
        let input = CRYPT_INTEGER_BLOB { cbData: text.len() as u32, pbData: text.as_ptr() as *mut u8 };
        let entropy = CRYPT_INTEGER_BLOB { cbData: ENTROPY.len() as u32, pbData: ENTROPY.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let desc = wide(std::ffi::OsStr::new("Diviora"));
        // SAFETY: input/entropy point to live buffers; out is freed with LocalFree.
        unsafe {
            if CryptProtectData(&input, desc.as_ptr(), &entropy, std::ptr::null(), std::ptr::null(),
                                CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN, &mut out) == 0 {
                return Err(format!("encryption failed (error {})", std::io::Error::last_os_error()));
            }
            let blob = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(out.pbData as *mut c_void as _);
            Ok(blob)
        }
    }

    pub fn write_locked_config(text: &str) -> Result<PathBuf, String> {
        let dir = base_dir();
        let status = dir.join("status");
        std::fs::create_dir_all(&status).map_err(|e| format!("cannot create {}: {}. Run Diviora as Administrator.", status.display(), e))?;
        // Lock the folders every time, so a tampered ACL is repaired.
        apply_sddl(&dir, APO_DIR_SDDL)?;
        apply_sddl(&status, STATUS_DIR_SDDL)?;

        let bytes = frame(&protect(text)?);
        let target = config_path();
        let tmp = dir.join("config.dvr.tmp");
        std::fs::write(&tmp, &bytes).map_err(|e| format!("cannot write config: {}. Run Diviora as Administrator.", e))?;
        // Atomic replace, so the APO never reads a half-written file.
        std::fs::rename(&tmp, &target).map_err(|e| format!("cannot replace config: {}", e))?;
        Ok(target)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn write_locked_config(_text: &str) -> Result<PathBuf, String> {
        Err("The system-wide APO configuration is only available on Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_header_matches_apo_reader() {
        let f = frame(&[0xAA, 0xBB]);
        assert_eq!(&f[..4], b"DVRC");
        assert_eq!(u32::from_le_bytes([f[4], f[5], f[6], f[7]]), 1);
        assert_eq!(&f[8..], &[0xAA, 0xBB]);
    }
}
