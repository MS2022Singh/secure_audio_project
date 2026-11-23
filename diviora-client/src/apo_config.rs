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
#[cfg(windows)]
const APO_DIR_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;LS)(A;OICI;FR;;;AC)(A;OICI;FR;;;S-1-15-2-2)(A;OICI;FR;;;S-1-5-80-2676549577-1911656217-2625096541-4178041876-1366760775)";
// audiodg (LOCAL SERVICE) may modify status files; users may read the (non-secret) status.
#[cfg(windows)]
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
/// Like `write_locked_config`, but when this process lacks the rights (a normal, non-elevated
/// app), Windows asks for administrator permission just for this step: the configuration is
/// handed to an elevated copy of the app through a temporary file encrypted for this Windows
/// user only (DPAPI), which is deleted afterwards.
pub fn write_locked_config_elevated(text: &str) -> Result<PathBuf, String> {
    match imp::write_locked_config(text) {
        Ok(p) => Ok(p),
        Err(e) if e.contains("error 5") || e.to_lowercase().contains("denied") || e.contains("Administrator") => imp::write_via_elevation(text),
        Err(e) => Err(e),
    }
}

/// Entry point of the elevated helper: `app.exe --apply-locked-config <file>`.
/// Returns the process exit code; the error text (if any) is written to `<file>.result`.
pub fn apply_locked_config_from_file(path: &std::path::Path) -> i32 {
    let r = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| imp::unprotect_user(&b)).and_then(|t| imp::write_locked_config(&t));
    match r {
        Ok(_) => 0,
        Err(e) => {
            let _ = std::fs::write(path.with_extension("result"), e);
            1
        }
    }
}

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
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
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

    /// DPAPI for the current Windows user only (used for the hand-over file).
    fn protect_user(text: &str) -> Result<Vec<u8>, String> {
        let input = CRYPT_INTEGER_BLOB { cbData: text.len() as u32, pbData: text.as_ptr() as *mut u8 };
        let entropy = CRYPT_INTEGER_BLOB { cbData: ENTROPY.len() as u32, pbData: ENTROPY.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: as in `protect`.
        unsafe {
            if CryptProtectData(&input, std::ptr::null(), &entropy, std::ptr::null(), std::ptr::null(),
                                CRYPTPROTECT_UI_FORBIDDEN, &mut out) == 0 {
                return Err(format!("encryption failed ({})", std::io::Error::last_os_error()));
            }
            let blob = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(out.pbData as *mut c_void as _);
            Ok(blob)
        }
    }

    pub fn unprotect_user(blob: &[u8]) -> Result<String, String> {
        let input = CRYPT_INTEGER_BLOB { cbData: blob.len() as u32, pbData: blob.as_ptr() as *mut u8 };
        let entropy = CRYPT_INTEGER_BLOB { cbData: ENTROPY.len() as u32, pbData: ENTROPY.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: input/entropy point to live buffers; out is freed with LocalFree.
        unsafe {
            if CryptUnprotectData(&input, std::ptr::null_mut(), &entropy, std::ptr::null(), std::ptr::null(),
                                  CRYPTPROTECT_UI_FORBIDDEN, &mut out) == 0 {
                return Err(format!("could not read the hand-over file ({}). Approve the prompt with your own Windows account.",
                    std::io::Error::last_os_error()));
            }
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(out.pbData as *mut c_void as _);
            String::from_utf8(bytes).map_err(|e| e.to_string())
        }
    }

    pub fn write_via_elevation(text: &str) -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut rnd = [0u8; 8];
        let _ = getrandom_fill(&mut rnd);
        let name: String = rnd.iter().map(|b| format!("{:02x}", b)).collect();
        let handover = std::env::temp_dir().join(format!("diviora-lock-{}.bin", name));
        std::fs::write(&handover, protect_user(text)?).map_err(|e| e.to_string())?;
        let result_file = handover.with_extension("result");
        let ps = format!(
            "try {{ $p = Start-Process -FilePath '{}' -ArgumentList '--apply-locked-config','\"{}\"' -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode }} catch {{ exit 1223 }}",
            exe.display().to_string().replace('\'', "''"), handover.display()
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .creation_flags_no_window()
            .status();
        let detail = std::fs::read_to_string(&result_file).unwrap_or_default();
        let _ = std::fs::remove_file(&handover);
        let _ = std::fs::remove_file(&result_file);
        match status.map(|s| s.code().unwrap_or(1)) {
            Ok(0) => Ok(config_path()),
            Ok(1223) => Err("Administrator permission was not given (the Windows prompt was declined).".into()),
            Ok(_) => Err(if detail.is_empty() { "Locking with administrator rights failed.".into() } else { detail }),
            Err(e) => Err(format!("could not ask for administrator permission: {}", e)),
        }
    }

    fn getrandom_fill(buf: &mut [u8]) -> Result<(), ()> {
        // Uniqueness only (not secrecy): time and process id are enough for a temp name.
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let v = (t as u64) ^ ((std::process::id() as u64) << 32);
        for (i, b) in buf.iter_mut().enumerate() { *b = (v >> ((i % 8) * 8)) as u8; }
        Ok(())
    }

    trait NoWindow { fn creation_flags_no_window(&mut self) -> &mut Self; }
    impl NoWindow for std::process::Command {
        fn creation_flags_no_window(&mut self) -> &mut Self {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
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
    pub fn write_via_elevation(_text: &str) -> Result<PathBuf, String> {
        write_locked_config(_text)
    }
    pub fn unprotect_user(_b: &[u8]) -> Result<String, String> {
        Err("Windows only".into())
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
