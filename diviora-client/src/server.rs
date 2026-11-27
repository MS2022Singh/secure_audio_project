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
    let b = ureq::AgentBuilder::new().timeout(Duration::from_secs(20));
    #[cfg(windows)]
    {
        // Windows' own TLS stack and certificate store (Schannel).
        if let Ok(t) = native_tls::TlsConnector::new() {
            return b.tls_connector(std::sync::Arc::new(t)).build();
        }
    }
    b.build()
}

/// Licence-server addresses must use HTTPS (plain HTTP only for this computer, for testing), so
/// tokens and configurations are never sent readable over the network.
fn secure(url: &str) -> Result<&str, String> {
    let u = url.trim_end_matches('/');
    let local = ["http://127.0.0.1", "http://localhost", "http://[::1]"].iter().any(|p| u.starts_with(p));
    if u.starts_with("https://") || local { Ok(u) } else { Err("The licence server address must start with https://".into()) }
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
    let url = secure(url)?;
    agent().post(&format!("{}/api/v3/activate", url))
        .send_json(serde_json::json!({ "license_key": license_key, "device_id": device_id }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())
}

pub fn redeem_invite(url: &str, invite: &str, device_id: &str) -> Result<Session, String> {
    let url = secure(url)?;
    agent().post(&format!("{}/api/v3/redeem", url))
        .send_json(serde_json::json!({ "invite_token": invite, "device_id": device_id }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())
}

pub fn fetch_config(url: &str, session: &str, since: i64) -> Result<ConfigResponse, String> {
    let url = secure(url)?;
    agent().get(&format!("{}/api/v3/config?since={}", url, since))
        .set("Authorization", &format!("Bearer {}", session))
        .call().map_err(err)?.into_json().map_err(|e| e.to_string())
}

pub fn admin_create_invite(url: &str, admin_token: &str, days: u32) -> Result<String, String> {
    let url = secure(url)?;
    let v: serde_json::Value = agent().post(&format!("{}/api/v3/admin/invites", url))
        .set("Authorization", &format!("Bearer {}", admin_token))
        .send_json(serde_json::json!({ "days": days }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())?;
    v.get("invite_token").and_then(|t| t.as_str()).map(|s| s.to_string()).ok_or_else(|| "Bad server reply".into())
}

pub fn admin_publish_config(url: &str, admin_token: &str, text: &str) -> Result<i64, String> {
    let url = secure(url)?;
    let v: serde_json::Value = agent().post(&format!("{}/api/v3/admin/config", url))
        .set("Authorization", &format!("Bearer {}", admin_token))
        .send_json(serde_json::json!({ "text": text }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())?;
    v.get("version").and_then(|t| t.as_i64()).ok_or_else(|| "Bad server reply".into())
}

/// Fetch a JSON document (e.g. the app's update manifest). Follows redirects.
pub fn get_json(url: &str) -> Result<serde_json::Value, String> {
    agent().get(url).call().map_err(err)?.into_json().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_https_or_this_computer() {
        assert!(super::secure("https://x.example").is_ok());
        assert!(super::secure("http://127.0.0.1:8000").is_ok());
        assert!(super::secure("http://x.example").is_err());
        assert!(super::secure("ftp://x").is_err());
    }
}
