pub mod playback;
pub mod drop;
pub mod security;
pub mod commands;
pub mod system_relay;

use std::sync::{Arc, Mutex};
use tauri::Manager;
use crate::playback::manager::PlaybackManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let playback_manager = Arc::new(Mutex::new(PlaybackManager::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(system_relay::SystemRelayState::default())
        .setup(move |app| {
            app.manage(playback_manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::set_native_track_path,
            commands::trigger_hardware_audio_playback,
            commands::ingest_dropped_paths,
            commands::get_live_amplitude_metrics,
            commands::enforce_global_hardware_apo,
            commands::get_device_hwid,
            commands::authenticate_user_session,
            commands::upload_apo_profile,
            commands::upload_signature,
            commands::toggle_watermark,
            commands::verify_watermark_presence,
            system_relay::list_relay_audio_devices,
            system_relay::start_system_relay,
            system_relay::stop_system_relay,
            system_relay::get_system_relay_status,
            system_relay::export_camilladsp_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}