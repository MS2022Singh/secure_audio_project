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

pub fn client_dir() -> PathBuf {
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Diviora").join("client")
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
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DeveloperCredentials {
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub admin_token: String,
}

pub fn developer_credentials() -> Option<DeveloperCredentials> {
    let mut c = std::env::var("LOCALAPPDATA").ok()
        .map(|d| PathBuf::from(d).join("Diviora").join("developer.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<DeveloperCredentials>(&s).ok())
        .unwrap_or_default();
    if let Ok(t) = std::env::var("DIVIORA_ADMIN_TOKEN") { c.admin_token = t; }
    if c.server_url.is_empty() {
        c.server_url = server_url(&ClientState::load()).unwrap_or_default();
    }
    if c.admin_token.is_empty() || c.server_url.is_empty() { None } else {
        c.server_url = c.server_url.trim_end_matches('/').to_string();
        Some(c)
    }
}
