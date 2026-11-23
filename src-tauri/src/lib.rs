//! Diviora app entry point, shared by desktop (Windows) and mobile (Android).

pub mod commands;
pub mod drop;
pub mod library;
pub mod playback;
pub mod security;
pub use diviora_client::apo_config;
pub use playback::manager::PlaybackManager;

use std::sync::{Arc, Mutex};
use tauri::Manager;

#[cfg(desktop)]
async fn auto_update(app: tauri::AppHandle) {
    use tauri_plugin_updater::UpdaterExt;
    let Ok(updater) = app.updater() else { return };
    match updater.check().await {
        Ok(Some(update)) => {
            eprintln!("[update] installing version {}", update.version);
            if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
                app.restart();
            }
        }
        Ok(None) => {}
        Err(e) => eprintln!("[update] check skipped: {}", e),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let manager = Arc::new(Mutex::new(PlaybackManager::new().expect("Failed to initialize PlaybackManager engine")));

    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());

    builder
        .manage(manager)
        .setup(|app| {
            // Per-app storage (Android has no %ProgramData%): library, licence state, cached config.
            if let Ok(dir) = app.path().app_data_dir() {
                library::init(dir.clone());
                #[cfg(mobile)]
                diviora_client::state::set_base_dir(dir);
            }
            commands::start_background_config_sync();
            // Automatic app updates (desktop): check the developer's latest signed release at
            // start-up and install it silently (see DEVELOPER_GUIDE.md).
            #[cfg(desktop)]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { auto_update(handle).await });
            }
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
            commands::set_watermark_params,
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
            commands::verify_watermark_in_file,
            commands::analyze_apo_profile,
            commands::import_track,
            commands::list_library,
            commands::delete_track,
            commands::platform_info,
            commands::licence_status,
            commands::lock_developer_session,
            commands::pause_playback
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
