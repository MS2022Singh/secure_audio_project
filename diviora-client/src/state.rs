//! Licence/session state shared by the app (writes it on activation) and the sync service
//! (refreshes the session, records the applied config version).
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientState {
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub license_key: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub session_expires_at: i64,
    #[serde(default)]
    pub licence_expires_at: i64,
    /// Last configuration version applied from the server.
    #[serde(default)]
    pub config_version: i64,
    #[serde(default)]
    pub last_sync: String,
}

static BASE_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Mobile: use the app's private data folder instead of %ProgramData%\Diviora.
pub fn set_base_dir(dir: PathBuf) {
    let _ = BASE_DIR.set(dir);
}

/// %ProgramData%\Diviora on Windows, or the folder set with `set_base_dir`.
pub fn base_dir() -> PathBuf {
    if let Some(d) = BASE_DIR.get() {
        return d.clone();
    }
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Diviora")
}

pub fn client_dir() -> PathBuf {
    base_dir().join("client")
}

pub fn state_path() -> PathBuf {
    client_dir().join("client.json")
}

impl ClientState {
    pub fn load() -> ClientState {
        std::fs::read_to_string(state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        std::fs::create_dir_all(client_dir()).map_err(|e| e.to_string())?;
        let tmp = client_dir().join("client.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, state_path()).map_err(|e| e.to_string())
    }

    pub fn is_activated(&self) -> bool {
        !self.license_key.is_empty() && !self.session.is_empty()
    }
}

/// Server URL: client.json, else the one baked in at build time (DIVIORA_SERVER_URL).
pub fn server_url(state: &ClientState) -> Option<String> {
    let u = if !state.server_url.is_empty() {
        state.server_url.clone()
    } else {
        option_env!("DIVIORA_SERVER_URL").unwrap_or("").to_string()
    };
    if u.is_empty() { None } else { Some(u.trim_end_matches('/').to_string()) }
}

/// Developer credentials (developer machine only): %LOCALAPPDATA%\Diviora\developer.json
/// {"server_url": "...", "admin_token": "..."}, or the DIVIORA_ADMIN_TOKEN environment variable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeveloperCredentials {
    #[serde(default)]
    pub server_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub admin_token: String,
    /// The token encrypted for this Windows user (DPAPI), base64. Written by the app.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub admin_token_protected: String,
}

fn developer_file() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA").ok().map(|d| PathBuf::from(d).join("Diviora").join("developer.json"))
}

/// Save the licence-server address and developer token for this Windows user only.
pub fn save_developer_credentials(server_url: &str, admin_token: &str) -> Result<(), String> {
    let path = developer_file().ok_or("No user profile folder.")?;
    #[cfg(windows)]
    let prot = crate::seal::b64(&crate::apo_config::protect_secret(admin_token, false)?);
    #[cfg(not(windows))]
    let prot = crate::seal::b64(admin_token.as_bytes());
    let c = DeveloperCredentials { server_url: server_url.trim().trim_end_matches('/').to_string(), admin_token: String::new(), admin_token_protected: prot };
    if let Some(d) = path.parent() { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
    std::fs::write(&path, serde_json::to_string_pretty(&c).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub fn developer_credentials() -> Option<DeveloperCredentials> {
    let mut c = developer_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<DeveloperCredentials>(&s).ok())
        .unwrap_or_default();
    if c.admin_token.is_empty() && !c.admin_token_protected.is_empty() {
        if let Ok(b) = crate::seal::unb64(&c.admin_token_protected) {
            #[cfg(windows)]
            { c.admin_token = crate::apo_config::unprotect_secret(&b, false).unwrap_or_default(); }
            #[cfg(not(windows))]
            { c.admin_token = String::from_utf8(b).unwrap_or_default(); }
        }
    }
    if let Ok(t) = std::env::var("DIVIORA_ADMIN_TOKEN") { c.admin_token = t; }
    if c.server_url.is_empty() {
        c.server_url = server_url(&ClientState::load()).unwrap_or_default();
    }
    if c.admin_token.is_empty() || c.server_url.is_empty() { None } else {
        c.server_url = c.server_url.trim_end_matches('/').to_string();
        Some(c)
    }
}
