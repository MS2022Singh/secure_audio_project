//! HTTPS client for backend/server.py (API v3).
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct Session {
    pub license_key: String,
    pub licence_expires_at: i64,
    pub session: String,
    pub session_expires_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigResponse {
    pub version: i64,
    pub changed: bool,
    #[serde(default)]
    pub text: String,
}

fn agent() -> ureq::Agent {
    // Windows' own TLS stack and certificate store (Schannel).
    let tls = native_tls::TlsConnector::new().map(std::sync::Arc::new);
    let b = ureq::AgentBuilder::new().timeout(Duration::from_secs(20));
    match tls { Ok(t) => b.tls_connector(t).build(), Err(_) => b.build() }
}

fn err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let detail = resp.into_json::<serde_json::Value>().ok()
                .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(|s| s.to_string()))
                .unwrap_or_default();
            format!("Server refused ({}): {}", code, detail)
        }
        other => format!("Cannot reach the licence server: {}", other),
    }
}

pub fn activate(url: &str, license_key: &str, device_id: &str) -> Result<Session, String> {
    agent().post(&format!("{}/api/v3/activate", url))
        .send_json(serde_json::json!({ "license_key": license_key, "device_id": device_id }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())
}

pub fn redeem_invite(url: &str, invite: &str, device_id: &str) -> Result<Session, String> {
    agent().post(&format!("{}/api/v3/redeem", url))
        .send_json(serde_json::json!({ "invite_token": invite, "device_id": device_id }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())
}

pub fn fetch_config(url: &str, session: &str, since: i64) -> Result<ConfigResponse, String> {
    agent().get(&format!("{}/api/v3/config?since={}", url, since))
        .set("Authorization", &format!("Bearer {}", session))
        .call().map_err(err)?.into_json().map_err(|e| e.to_string())
}

pub fn admin_create_invite(url: &str, admin_token: &str, days: u32) -> Result<String, String> {
    let v: serde_json::Value = agent().post(&format!("{}/api/v3/admin/invites", url))
        .set("Authorization", &format!("Bearer {}", admin_token))
        .send_json(serde_json::json!({ "days": days }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())?;
    v.get("invite_token").and_then(|t| t.as_str()).map(|s| s.to_string()).ok_or_else(|| "Bad server reply".into())
}

pub fn admin_publish_config(url: &str, admin_token: &str, text: &str) -> Result<i64, String> {
    let v: serde_json::Value = agent().post(&format!("{}/api/v3/admin/config", url))
        .set("Authorization", &format!("Bearer {}", admin_token))
        .send_json(serde_json::json!({ "text": text }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())?;
    v.get("version").and_then(|t| t.as_i64()).ok_or_else(|| "Bad server reply".into())
}
