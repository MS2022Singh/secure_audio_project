//! Code shared by the Diviora app (src-tauri) and the background sync service (diviora-sync).
//!
//! * [`apo_config`] – encrypted, ACL-locked configuration file read by the native APO.
//! * [`device`]     – stable device ID (one licence ID = one device).
//! * [`state`]      – client licence/session state in %ProgramData%\Diviora\client\client.json.
//! * [`server`]     – HTTPS client for the licensing / configuration server (backend/server.py).
//! * [`sync`]       – one synchronisation pass: refresh session, download and lock new configs.

pub mod apo_config;
pub mod device;
pub mod server;
pub mod state;
pub mod sync;
