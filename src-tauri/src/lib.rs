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
    let set = |m: String| { if let Ok(mut s) = UPDATE_STATUS.lock() { *s = m; } };
    set("checking for updates…".into());
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => { set(format!("updates not configured in this build ({})", e)); return; }
    };
    match updater.check().await {
        Ok(Some(update)) => {
            set(format!("installing version {}…", update.version));
            match update.download_and_install(|_, _| {}, || {}).await {
                Ok(()) => app.restart(),
                Err(e) => set(format!("update to {} failed: {}", update.version, e)),
            }
        }
        Ok(None) => set(format!("up to date (checked {})", chrono::Local::now().format("%H:%M"))),
        Err(e) => set(format!("update check failed: {}", e)),
    }
}

#[tauri::command]
async fn check_updates_now(app: tauri::AppHandle) -> String {
    #[cfg(desktop)]
    {
        auto_update(app).await;
        UPDATE_STATUS.lock().map(|s| s.clone()).unwrap_or_default()
    }
    #[cfg(mobile)]
    {
        let _ = app;
        "phone: see the update banner".to_string()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let manager = Arc::new(Mutex::new(PlaybackManager::new().expect("Failed to initialize PlaybackManager engine")));

    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_apkinstaller::init());
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
            // Phones: remove the previous update's installer file (no clutter left behind).
            #[cfg(mobile)]
            if let Ok(cache) = app.path().app_cache_dir() {
                let _ = std::fs::remove_dir_all(cache.join("updates"));
            }

            // Automatic app updates (desktop): check the developer's latest signed release at
            // start-up and install it silently (see DEVELOPER_GUIDE.md).
            #[cfg(desktop)]
            {
                let handle = app.handle().clone();
                // At start-up and then every 6 hours while the app runs.
                tauri::async_runtime::spawn(async move {
                    loop {
                        auto_update(handle.clone()).await;
                        tokio_sleep(6 * 3600).await;
                    }
                });
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
            commands::pause_playback,
            commands::set_playback_volume,
            commands::check_for_update,
            commands::open_update_download,
            commands::install_phone_update,
            commands::restart_windows_audio,
            commands::verify_system_audio,
            commands::import_track_b64,
            commands::change_developer_password,
            commands::app_version_info,
            check_updates_now
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}


#[cfg(desktop)]
async fn tokio_sleep(secs: u64) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(std::time::Duration::from_secs(secs))).await;
}

/// Result of the last update check, shown in the app (Licence section).
pub static UPDATE_STATUS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
