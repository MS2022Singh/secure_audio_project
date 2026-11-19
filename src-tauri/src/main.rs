use std::sync::{Arc, Mutex};
use tauri::Manager;

pub mod playback;
pub mod drop;
pub mod security;
pub mod commands;
pub mod system_relay;

pub use playback::manager::PlaybackManager;

fn main() {
    // PlaybackManager::new() returns a Result, so we unwrap it
    let playback_instance = PlaybackManager::new().expect("Failed to initialize PlaybackManager engine");
    let manager = Arc::new(Mutex::new(playback_instance));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(system_relay::SystemRelayState::default())
        .manage(manager.clone())
        .setup(move |app| {
            app.manage(manager.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::upload_apo_profile,
            commands::set_native_track_path,
            commands::trigger_hardware_audio_playback,
            commands::detect_audio_hardware_devices,
            commands::reset_client_token,
            commands::verify_master_password,
            commands::reset_master_password,
            commands::generate_invite_token,
            commands::sync_project_to_github,
            commands::export_processed_audio_file,
            commands::export_cpp_apo_bootstrap_script,
            commands::set_watermark_time_frame,
            commands::clear_watermark_signature,
            commands::ingest_dropped_paths,
            commands::generate_apo_verification_report,
            commands::generate_watermark_audit_report,
            commands::perform_real_dll_apo_audit,
            commands::perform_real_watermark_fft_audit,
            commands::check_playback_queue_status,
            commands::get_live_amplitude_metrics,
            commands::enforce_global_hardware_apo,
            commands::get_device_hwid,
            commands::authenticate_user_session,
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