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
    /// Configuration encrypted end-to-end (base64); only this device can open it.
    #[serde(default)]
    pub sealed: String,
    /// Content key wrapped for this device (base64).
    #[serde(default)]
    pub wrapped_key: String,
    /// Published before this device was activated; waiting for the developer app to add a key.
    #[serde(default)]
    pub pending: bool,
}

impl ConfigResponse {
    /// Decrypt with this device's private key.
    pub fn open(&self) -> Result<String, String> {
        let ck = crate::seal::unwrap_on_device(&crate::seal::unb64(&self.wrapped_key)?, &crate::seal::device_secret()?)?;
        crate::seal::open_config(&crate::seal::unb64(&self.sealed)?, &ck)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceEntry {
    pub license_key: String,
    #[serde(default)]
    pub device_pubkey: Option<String>,
    #[serde(default)]
    pub has_key: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Devices {
    pub latest_version: i64,
    pub devices: Vec<DeviceEntry>,
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
        .send_json(serde_json::json!({ "license_key": license_key, "device_id": device_id,
            "device_pubkey": crate::seal::device_public_b64().ok() }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())
}

pub fn redeem_invite(url: &str, invite: &str, device_id: &str) -> Result<Session, String> {
    let url = secure(url)?;
    agent().post(&format!("{}/api/v3/redeem", url))
        .send_json(serde_json::json!({ "invite_token": invite, "device_id": device_id,
            "device_pubkey": crate::seal::device_public_b64().ok() }))
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

pub fn admin_devices(url: &str, admin_token: &str) -> Result<Devices, String> {
    let url = secure(url)?;
    agent().get(&format!("{}/api/v3/admin/devices", url))
        .set("Authorization", &format!("Bearer {}", admin_token))
        .call().map_err(err)?.into_json().map_err(|e| e.to_string())
}

/// Wrap `content_key` for every device in `devices` that has a key and (if `only_missing`)
/// cannot open the latest version yet. Returns (licence -> wrapped key, devices that need an app update).
fn wrap_all(devices: &Devices, content_key: &[u8; 32], only_missing: bool) -> (serde_json::Map<String, serde_json::Value>, usize) {
    let mut keys = serde_json::Map::new();
    let mut old_apps = 0;
    for d in &devices.devices {
        if only_missing && d.has_key != 0 { continue; }
        match d.device_pubkey.as_deref().map(crate::seal::key32) {
            Some(Ok(pk)) => { keys.insert(d.license_key.clone(), crate::seal::b64(&crate::seal::wrap_for_device(content_key, &pk)).into()); }
            _ => old_apps += 1,
        }
    }
    (keys, old_apps)
}

/// Publish a configuration, encrypted end-to-end for every activated device.
/// Returns (version, devices that can open it, devices whose app must update first).
pub fn admin_publish_config(url: &str, admin_token: &str, text: &str) -> Result<(i64, usize, usize), String> {
    let url = secure(url)?;
    let devices = admin_devices(url, admin_token)?;
    let ck = crate::seal::random32();
    let sealed = crate::seal::b64(&crate::seal::seal_config(text, &ck));
    let (keys, old_apps) = wrap_all(&devices, &ck, false);
    let v: serde_json::Value = agent().post(&format!("{}/api/v3/admin/config", url))
        .set("Authorization", &format!("Bearer {}", admin_token))
        .send_json(serde_json::json!({ "sealed": sealed, "keys": keys }))
        .map_err(err)?.into_json().map_err(|e| e.to_string())?;
    let version = v.get("version").and_then(|t| t.as_i64()).ok_or("Bad server reply")?;
    crate::seal::save_publish_key(version, &ck)?;
    Ok((version, keys.len(), old_apps))
}

/// Give devices activated after the last publish access to it (no need to publish again).
/// Returns how many devices were added.
pub fn admin_deliver_to_new_devices(url: &str, admin_token: &str) -> Result<usize, String> {
    let url = secure(url)?;
    let Some((version, ck)) = crate::seal::load_publish_key() else { return Ok(0) };
    let devices = admin_devices(url, admin_token)?;
    if devices.latest_version != version { return Ok(0); }
    let (keys, _) = wrap_all(&devices, &ck, true);
    if keys.is_empty() { return Ok(0); }
    agent().post(&format!("{}/api/v3/admin/config/{}/keys", url, version))
        .set("Authorization", &format!("Bearer {}", admin_token))
        .send_json(serde_json::json!({ "keys": keys }))
        .map_err(err)?;
    Ok(keys.len())
}

/// Stop distributing the configuration to users.
pub fn admin_withdraw_config(url: &str, admin_token: &str) -> Result<(), String> {
    let url = secure(url)?;
    agent().delete(&format!("{}/api/v3/admin/config", url))
        .set("Authorization", &format!("Bearer {}", admin_token))
        .call().map_err(err)?;
    crate::seal::forget_publish_key();
    Ok(())
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
