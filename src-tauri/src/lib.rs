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
    let checked = updater.check().await;
    // Direct connection blocked (VPN, proxy auto-configuration, firewall rules for unknown
    // apps): try again through Windows' own internet settings, like Chrome and Windows do.
    #[cfg(windows)]
    if let Err(e) = &checked {
        let first = error_chain(e);
        let h = app.clone();
        let r = tauri::async_runtime::spawn_blocking(move || windows_fallback_update(&h)).await;
        match r {
            Ok(Ok(Some(msg))) => { set(msg); return; }
            Ok(Ok(None)) => { set(format!("up to date (checked {} via Windows internet settings)", chrono::Local::now().format("%H:%M"))); return; }
            Ok(Err(e2)) => { set(format!("update check failed: {} | via Windows internet settings: {}", first, e2)); return; }
            Err(_) => {}
        }
    }
    match checked {
        Ok(Some(update)) => {
            set(format!("installing version {}…", update.version));
            match update.download_and_install(|_, _| {}, || {}).await {
                Ok(()) => app.restart(),
                Err(e) => set(format!("update to {} failed: {}", update.version, error_chain(&e))),
            }
        }
        Ok(None) => set(format!("up to date (checked {})", chrono::Local::now().format("%H:%M"))),
        Err(e) => set(format!("update check failed: {}", error_chain(&e))),
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

/// An error with all its causes ("error sending request" alone does not say why).
#[cfg(desktop)]
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        let t = c.to_string();
        if !out.contains(&t) {
            out.push_str(" → ");
            out.push_str(&t);
        }
        cur = c.source();
    }
    out
}

/// Command-line modes of the desktop app (no window): `--apply-locked-config <file>` is the
/// elevated helper behind Lock Config. Returns Some(exit code) if a mode ran.
pub fn handle_cli() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--apply-locked-config" {
        return Some(apo_config::apply_locked_config_from_file(std::path::Path::new(&args[2])));
    }
    None
}

/// Download with Windows' own networking (.NET WebClient: system proxy incl. auto-configuration,
/// system DNS and certificates), through Windows PowerShell.
#[cfg(windows)]
fn windows_download(url: &str, out: &std::path::Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let q = |s: &str| s.replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; \
         $w=New-Object Net.WebClient; $w.Proxy=[Net.WebRequest]::GetSystemWebProxy(); \
         $w.Proxy.Credentials=[Net.CredentialCache]::DefaultNetworkCredentials; $w.DownloadFile('{}','{}')",
        q(url), q(&out.display().to_string()));
    let o = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(0x0800_0000) // no console window
        .output()
        .map_err(|e| format!("PowerShell not available: {}", e))?;
    if o.status.success() && out.exists() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&o.stderr).lines().find(|l| !l.trim().is_empty()).unwrap_or("download failed").to_string();
        Err(err.trim().to_string())
    }
}

/// Update check + signed install without Tauri's HTTP client. Ok(None) = up to date.
#[cfg(windows)]
fn windows_fallback_update(app: &tauri::AppHandle) -> Result<Option<String>, String> {
    let dir = std::env::temp_dir().join("diviora-update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let updater_cfg = app.config().plugins.0.get("updater").cloned().unwrap_or_default();
    let endpoint = updater_cfg.get("endpoints").and_then(|e| e.get(0)).and_then(|e| e.as_str())
        .ok_or("no update address configured")?.to_string();
    let pubkey = updater_cfg.get("pubkey").and_then(|k| k.as_str()).unwrap_or("").to_string();
    if pubkey.is_empty() { return Err("updates are not signed in this build".into()); }

    let manifest_path = dir.join("latest.json");
    windows_download(&endpoint, &manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("bad update manifest: {}", e))?;
    let latest = manifest.get("version").and_then(|v| v.as_str()).unwrap_or("0.0.0").trim_start_matches('v').to_string();
    let parse = |v: &str| -> Vec<u64> { v.split(['.', '-']).map(|p| p.parse().unwrap_or(0)).take(3).collect() };
    let current = app.package_info().version.to_string();
    if parse(&latest) <= parse(&current) {
        return Ok(None);
    }
    let plat = manifest.pointer("/platforms/windows-x86_64").ok_or("no Windows update in the manifest")?;
    let url = plat.get("url").and_then(|u| u.as_str()).ok_or("no download address")?;
    let sig = plat.get("signature").and_then(|u| u.as_str()).ok_or("no signature")?;
    let setup = dir.join("Diviora-Setup.exe");
    windows_download(url, &setup)?;

    // Same check as Tauri's updater: the installer must be signed with the update key.
    let data = std::fs::read(&setup).map_err(|e| e.to_string())?;
    let b64 = |s: &str| diviora_dsp::watermark::base64_decode(s).and_then(|b| String::from_utf8(b).ok());
    let pk = minisign_verify::PublicKey::decode(&b64(&pubkey).ok_or("bad public key")?).map_err(|e| e.to_string())?;
    let signature = minisign_verify::Signature::decode(&b64(sig).ok_or("bad signature")?).map_err(|e| e.to_string())?;
    if pk.verify(&data, &signature, true).is_err() {
        let _ = std::fs::remove_file(&setup);
        return Err("the downloaded update's signature is not valid; it was deleted".into());
    }

    // Run it like Tauri's updater (passive, update mode, restart Diviora afterwards).
    let ps = format!("Start-Process -FilePath '{}' -ArgumentList '/P','/UPDATE','/R'",
        setup.display().to_string().replace('\'', "''"));
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .spawn()
        .map_err(|e| format!("could not start the installer: {}", e))?;
    // Give the installer a moment to start, then leave so it can replace the app.
    std::thread::sleep(std::time::Duration::from_secs(2));
    app.exit(0);
    Ok(Some(format!("installing version {}…", latest)))
}
