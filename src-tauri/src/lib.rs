pub mod playback;
pub mod drop;
pub mod security;
pub mod commands;
pub mod apo_config;

use std::sync::{Arc, Mutex};
use tauri::Manager;
use crate::playback::manager::PlaybackManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let playback_manager = Arc::new(Mutex::new(PlaybackManager::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            commands::verify_watermark_in_file,
            commands::analyze_apo_profile
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}