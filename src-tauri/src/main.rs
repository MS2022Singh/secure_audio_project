#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod playback;
mod drop;
mod security;
mod commands;

use std::sync::{Arc, Mutex};
use playback::manager::PlaybackManager;
use commands::*;

fn main() {
    let manager = Arc::new(Mutex::new(PlaybackManager::new()));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(manager)
        .invoke_handler(tauri::generate_handler![
            upload_apo_profile,
            export_cpp_apo_bootstrap_script,
            set_watermark_time_frame,
            clear_watermark_signature,
            set_native_track_path,
            resolve_youtube_stream_url,
            detect_audio_hardware_devices,
            trigger_hardware_audio_playback,
            ingest_dropped_paths,
            generate_apo_verification_report,
            generate_watermark_audit_report,
            perform_real_dll_apo_audit,
            perform_real_watermark_fft_audit,
            check_playback_queue_status,
            get_live_amplitude_metrics,
            enforce_global_hardware_apo,
            get_device_hwid,
            authenticate_user_session,
            reset_client_token,
            upload_signature,
            toggle_watermark,
            verify_watermark_presence,
            verify_master_password,
            reset_master_password,
            generate_invite_token,
            sync_project_to_github,
            export_processed_audio_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}