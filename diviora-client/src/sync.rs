//! One synchronisation pass (run by the diviora-sync service every few minutes):
//! refresh the session if it is close to expiry, download a newer configuration from the
//! developer and lock it for the APO.
use crate::{apo_config, server, state};

#[derive(Debug, PartialEq)]
pub enum SyncOutcome {
    NotActivated,
    NoServer,
    UpToDate(i64),
    Applied(i64),
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn sync_once() -> Result<SyncOutcome, String> {
    let mut st = state::ClientState::load();
    if st.license_key.is_empty() {
        return Ok(SyncOutcome::NotActivated);
    }
    let url = state::server_url(&st).ok_or_else(|| "No server configured".to_string()).map_err(|_| "no server".to_string());
    let url = match url { Ok(u) => u, Err(_) => return Ok(SyncOutcome::NoServer) };

    if st.session.is_empty() || st.session_expires_at - now() < 24 * 3600 {
        let s = server::activate(&url, &st.license_key, &st.device_id)?;
        st.session = s.session;
        st.session_expires_at = s.session_expires_at;
        st.licence_expires_at = s.licence_expires_at;
    }
    let cfg = server::fetch_config(&url, &st.session, st.config_version)?;
    let outcome = if cfg.changed {
        apo_config::write_locked_config(&cfg.text)?;
        st.config_version = cfg.version;
        SyncOutcome::Applied(cfg.version)
    } else {
        SyncOutcome::UpToDate(cfg.version)
    };
    st.last_sync = format!("{}", now());
    st.save()?;
    Ok(outcome)
}
