use std::fs;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};

use crate::playback::manager::{PlaybackManager, PlaybackStatus};
use crate::drop::scan::FileSystemScanner;
use crate::security::SecurityManager;

/// Set once the developer has unlocked the developer panel in this session.
static DEVELOPER_UNLOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The player is for activated devices (and the developer). The UI hides it too; this is the
/// check that counts.
fn require_access() -> Result<(), String> {
    if DEVELOPER_UNLOCKED.load(std::sync::atomic::Ordering::Relaxed)
        || diviora_client::state::ClientState::load().is_activated()
    {
        Ok(())
    } else {
        Err("This device is not activated. Enter your licence key or invite token first.".into())
    }
}

/// Every developer-panel command checks this in the backend; hiding the panel is not enough.
fn require_developer() -> Result<(), String> {
    if DEVELOPER_UNLOCKED.load(std::sync::atomic::Ordering::Relaxed) {
        Ok(())
    } else {
        Err("Developer access required.".into())
    }
}

#[tauri::command]
pub fn detect_audio_hardware_devices() -> Result<Vec<String>, String> {
    // Real output devices from the system (Windows audio / Android), default first.
    let devs = crate::playback::manager::output_device_names();
    if devs.is_empty() { Err("No audio output device found.".into()) } else { Ok(devs) }
}

// Master config staged in memory only; written to disk solely as the encrypted,
// ACL-locked config.dvr when the developer presses Lock (enforce_global_hardware_apo).
static STAGED_PROFILE: Mutex<Option<String>> = Mutex::new(None);
static PROFILE_LOCKED: Mutex<bool> = Mutex::new(false);
static EQ_ENABLED: Mutex<bool> = Mutex::new(true);

/// Watermark settings (developer panel), held in memory and written only inside the
/// encrypted config.dvr together with the equalizer profile.
struct WatermarkState {
    active: bool,
    spec: diviora_dsp::watermark::WatermarkSpec,
}
fn watermark_state() -> &'static Mutex<WatermarkState> {
    static S: std::sync::OnceLock<Mutex<WatermarkState>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(WatermarkState {
        active: false,
        spec: diviora_dsp::watermark::WatermarkSpec { id: "dev-key-v1".into(), interval_s: 30.0, end_s: 3600.0, ..Default::default() },
    }))
}

/// Equalizer profile (if enabled) + watermark lines (if active): what the APO receives.
fn compose_apo_payload() -> String {
    let mut payload = if *EQ_ENABLED.lock().unwrap() {
        STAGED_PROFILE.lock().unwrap().clone().unwrap_or_default()
    } else {
        String::new()
    };
    let wm = watermark_state().lock().unwrap();
    if wm.active {
        if !payload.is_empty() && !payload.ends_with('\n') { payload.push('\n'); }
        payload.push_str(&diviora_dsp::watermark::to_config_lines(&wm.spec));
    }
    payload
}

/// Re-write the locked config if Lock Config has been pressed (or a locked file exists).
fn reapply_if_locked() -> Result<bool, String> {
    if *PROFILE_LOCKED.lock().unwrap() || crate::apo_config::config_path().exists() {
        crate::apo_config::write_locked_config(&compose_apo_payload())?;
        return Ok(true);
    }
    Ok(false)
}
/// Developer configuration received from the licence server (activated devices).
static SYNCED_CONFIG: Mutex<Option<String>> = Mutex::new(None);

/// True when DivioraAPO is running inside audiodg.exe (it then processes all audio,
/// including this player, so the player must not process again).
fn system_apo_active() -> bool {
    if !cfg!(windows) { return false; }
    let fresh = std::fs::metadata(crate::apo_config::status_path()).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok()).map(|d| d.as_secs() < 5).unwrap_or(false);
    fresh && crate::apo_config::read_status()
        .and_then(|v| v.get("process").and_then(|p| p.as_str()).map(|p| p.eq_ignore_ascii_case("audiodg.exe")))
        .unwrap_or(false)
}

/// Configuration for processing inside the app: the developer's own (developer PC), else
/// the one published to this device. Some("") never returned.
fn effective_config() -> Option<String> {
    let local = compose_apo_payload();
    if !local.trim().is_empty() { return Some(local); }
    SYNCED_CONFIG.lock().unwrap().clone().filter(|t| !t.trim().is_empty())
}

fn config_cache_path() -> PathBuf { diviora_client::state::client_dir().join("app_config.txt") }

/// Fetch the developer's configuration every 5 minutes for in-app processing. On Android it is
/// cached in the app's private storage for offline use; on Windows it stays in memory only
/// (the sync service locks it for the APO).
pub fn start_background_config_sync() {
    if cfg!(target_os = "android") {
        if let Ok(t) = fs::read_to_string(config_cache_path()) { *SYNCED_CONFIG.lock().unwrap() = Some(t); }
    }
    std::thread::spawn(|| loop {
        if let Ok(Some((_v, text))) = diviora_client::sync::fetch_latest_config() {
            if cfg!(target_os = "android") {
                let _ = fs::create_dir_all(diviora_client::state::client_dir());
                let _ = fs::write(config_cache_path(), &text);
            }
            *SYNCED_CONFIG.lock().unwrap() = Some(text);
        }
        std::thread::sleep(std::time::Duration::from_secs(300));
    });
}

fn applied_note(applied: bool) -> &'static str {
    if applied { " Applied system-wide." } else { " Press Lock Config to apply system-wide." }
}


// ---------------------------------------------------------------- shared helpers
const MAX_DECODE_SECONDS: usize = 20 * 60;

/// Decode WAV/MP3/FLAC/OGG to interleaved f32 (at most 20 minutes).
fn decode_audio_file(path: &str) -> Result<(Vec<f32>, u32, u16), String> {
    use rodio::Source;
    let f = fs::File::open(path).map_err(|e| format!("Cannot open {}: {}", path, e))?;
    let dec = rodio::Decoder::new(std::io::BufReader::new(f))
        .map_err(|_| "Not a supported audio file (WAV, MP3, FLAC or OGG).".to_string())?;
    let ch = dec.channels().max(1);
    let sr = dec.sample_rate().max(1);
    let max = sr as usize * ch as usize * MAX_DECODE_SECONDS;
    let samples: Vec<f32> = dec.take(max).map(|x: i16| x as f32 / 32768.0).collect();
    if samples.is_empty() {
        return Err("The audio file contains no samples.".into());
    }
    Ok((samples, sr, ch))
}

fn current_track(manager: &State<'_, Arc<Mutex<PlaybackManager>>>) -> String {
    let mgr = manager.inner().lock().unwrap();
    let p = mgr.current_track_path.lock().unwrap().clone();
    p
}

/// Tone-mode watermark detection on a file, with the current Signature ID and carrier.
fn detect_watermark_in_file(path: &str) -> Result<serde_json::Value, String> {
    let (samples, sr, ch) = decode_audio_file(path)?;
    let mono: Vec<f32> = samples.chunks(ch as usize).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    let (id, freq, clip) = {
        let wm = watermark_state().lock().unwrap();
        (wm.spec.id.clone(), wm.spec.freq, wm.spec.clip.is_some())
    };
    let d = diviora_dsp::watermark::detect(&mono, sr, freq, &id);
    Ok(serde_json::json!({
        "file": path,
        "signature_id": id,
        "carrier_hz": freq,
        "detected": d.found,
        "score": d.score,
        "at_seconds": d.at_seconds,
        "note": if clip { "An uploaded watermark clip is active; detection checks the frequency signature only." } else { "" }
    }))
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn desktop_file(name: &str) -> PathBuf {
    dirs::desktop_dir().unwrap_or_else(std::env::temp_dir).join(name)
}

/// Repository root (developer machine running from source), if any.
fn find_repo_root() -> Option<PathBuf> {
    let mut starts = vec![];
    if let Ok(c) = std::env::current_dir() { starts.push(c); }
    if let Ok(e) = std::env::current_exe() { if let Some(d) = e.parent() { starts.push(d.to_path_buf()); } }
    for s in starts {
        let mut p = Some(s.as_path());
        while let Some(dir) = p {
            if dir.join(".git").exists() && dir.join("windows-installer").exists() { return Some(dir.to_path_buf()); }
            p = dir.parent();
        }
    }
    None
}

/// install_native_apo.ps1 next to the installed app (resources) or in the source tree.
fn find_installer_script() -> Option<PathBuf> {
    let mut candidates = vec![];
    if let Ok(e) = std::env::current_exe() {
        if let Some(d) = e.parent() {
            candidates.push(d.join("windows-installer").join("install_native_apo.ps1"));
            candidates.push(d.join("resources").join("windows-installer").join("install_native_apo.ps1"));
        }
    }
    if let Some(r) = find_repo_root() { candidates.push(r.join("windows-installer").join("install_native_apo.ps1")); }
    candidates.into_iter().find(|p| p.exists())
}

#[tauri::command]
pub fn upload_apo_profile(profile_data: String, is_enabled: bool, _manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    require_developer()?;
    if profile_data == "REMOVE" || !is_enabled {
        if profile_data == "REMOVE" {
            *STAGED_PROFILE.lock().unwrap() = None;
        }
        *EQ_ENABLED.lock().unwrap() = false;
        // Equalizer off = pass-through; an active watermark keeps running.
        reapply_if_locked()?;
        return Ok("Equalizer disabled (pass-through).".to_string());
    }
    let summary = diviora_dsp::eqapo::parse(&profile_data, None);
    *STAGED_PROFILE.lock().unwrap() = Some(profile_data.clone());
    *EQ_ENABLED.lock().unwrap() = true;
    let locked = reapply_if_locked()?;
    let mut msg = format!(
        "Profile staged in memory: {} processing stage(s){}.",
        summary.stages.len(),
        if locked { ", applied system-wide" } else { "; press Lock Config to apply system-wide" }
    );
    if !summary.warnings.is_empty() {
        msg.push_str(&format!(" {} line(s) not supported: {}", summary.warnings.len(),
            summary.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>().join("; ")));
    }
    Ok(msg)
}

/// Measured "Estimated properties" for the uploaded configuration.
#[tauri::command]
pub fn analyze_apo_profile(sample_rate: Option<u32>, channels: Option<u32>, device: Option<String>,
    only_channel: Option<u32>, points: Option<u32>, source: Option<String>) -> Result<serde_json::Value, String> {
    require_developer()?;
    // "applied": what the APO runs (equalizer + watermark); otherwise the uploaded file.
    let text = if source.as_deref() == Some("applied") {
        compose_apo_payload()
    } else {
        STAGED_PROFILE.lock().unwrap().clone().unwrap_or_default()
    };
    // Device names in the list carry " (Default)"; config Device: lines match the plain name.
    let dev = device.map(|d| d.trim_end_matches(" (Default)").to_string());
    let a = diviora_dsp::analysis::analyze_with(&text, sample_rate.unwrap_or(48000), channels.unwrap_or(2) as usize,
        dev.as_deref(), only_channel.map(|c| c as usize), points.unwrap_or(400) as usize);
    Ok(serde_json::json!({
        "peak_gain_db": a.peak_gain_db, "peak_freq_hz": a.peak_freq_hz, "latency_ms": a.latency_ms,
        "init_ms": a.init_ms, "cpu_percent": a.cpu_percent, "stages": a.stages, "warnings": a.warnings
    }))
}

#[tauri::command]
pub fn set_native_track_path(path: String, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    require_access()?;
    let mgr = manager.inner();
    let mutex_lock = mgr.lock().unwrap();
    if let Ok(mut track_guard) = mutex_lock.current_track_path.lock() {
        *track_guard = path.clone();
    }
    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path);
    Ok(name)
}

/// Player volume, 0.0 (mute) to 1.0 (full); applies immediately and to later tracks.
#[tauri::command]
pub fn set_playback_volume(volume: f32, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<f32, String> {
    let v = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
    manager.inner().lock().unwrap().set_volume(v);
    Ok(v)
}

/// Where test/released builds publish their update manifest (set at build time).
const UPDATE_MANIFEST: &str = match option_env!("DIVIORA_UPDATE_MANIFEST") {
    Some(u) => u,
    None => "https://github.com/MS2022Singh/secure_audio_project/releases/download/test-build/latest.json",
};

fn version_tuple(v: &str) -> (u64, u64, u64) {
    let mut it = v.trim().trim_start_matches('v').split(|c: char| c == '.' || c == '-').map(|p| p.parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

/// Android cannot replace itself silently (outside an app store): the app checks for a newer
/// build and offers it; the user taps Install. Windows updates itself (see lib.rs).
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<serde_json::Value, String> {
    let current = app.package_info().version.to_string();
    let manifest = tauri::async_runtime::spawn_blocking(|| diviora_client::server::get_json(UPDATE_MANIFEST))
        .await.map_err(|e| e.to_string())??;
    let latest = manifest.get("version").and_then(|v| v.as_str()).unwrap_or("0.0.0").to_string();
    let url = manifest.pointer("/platforms/android/url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let available = version_tuple(&latest) > version_tuple(&current) && !url.is_empty();
    Ok(serde_json::json!({ "available": available, "current": current, "latest": latest, "url": url }))
}

/// Open the update download (Android: the browser downloads the APK, then the user installs it).
#[tauri::command]
pub fn open_update_download(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !url.starts_with("https://github.com/MS2022Singh/secure_audio_project/releases/") {
        return Err("Unexpected update address.".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Pause (true) or resume (false) the current track at its position.
#[tauri::command]
pub fn pause_playback(paused: bool, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<bool, String> {
    let mgr = manager.inner().lock().unwrap();
    mgr.set_paused(paused);
    Ok(paused)
}

#[tauri::command]
pub fn trigger_hardware_audio_playback(start: bool, _handle: AppHandle, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    if start {
        require_access()?;
    }
    let mgr = manager.inner().lock().unwrap();
    if start {
        let path_str = mgr.current_track_path.lock().unwrap().clone();
        if path_str.is_empty() {
            return Err("No audio file selected.".to_string());
        }
        let process = if system_apo_active() { None } else { effective_config() };
        let how = match (&process, system_apo_active()) {
            (_, true) => "system-wide engine",
            (Some(_), false) => "in-app processing",
            (None, false) => "unprocessed: no configuration yet",
        };
        mgr.play_file(path_str.clone(), process, how)?;
        Ok(format!("Playback started ({}): {}", how, path_str))
    } else {
        mgr.stop();
        Ok("Playback stopped".to_string())
    }
}

/// Remove this device's activation (licence key, session) and the local licence cache.
#[tauri::command]
pub fn reset_client_token() -> Result<String, String> {
    let _ = fs::remove_file(SecurityManager::get_sec_dir().join("license.db"));
    let mut st = diviora_client::state::ClientState::load();
    let had = !st.license_key.is_empty();
    st.license_key.clear();
    st.session.clear();
    st.session_expires_at = 0;
    st.save()?;
    Ok(if had {
        "Activation removed from this device. To move the licence to another device, the developer resets its binding on the server.".to_string()
    } else {
        "This device was not activated.".to_string()
    })
}

#[tauri::command]
pub fn verify_master_password(password: String) -> Result<bool, String> {
    // Ignore spaces/new lines picked up when the password is copied and pasted.
    let ok = SecurityManager::verify_master_pass(password.trim())?;
    if ok {
        DEVELOPER_UNLOCKED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(ok)
}

/// Developer panel closed (Ctrl+Shift+D again): drop the developer's access for this session.
#[tauri::command]
pub fn lock_developer_session() {
    DEVELOPER_UNLOCKED.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Licence state of this device, for the activation screen and the licence card.
#[tauri::command]
pub fn licence_status() -> serde_json::Value {
    let st = diviora_client::state::ClientState::load();
    let until = chrono::DateTime::from_timestamp(st.licence_expires_at, 0)
        .map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
    serde_json::json!({
        "activated": st.is_activated(),
        "licence_key": st.license_key,
        "expires": if st.licence_expires_at > 0 { until } else { String::new() },
        "offline": st.session == "offline",
        "developer": DEVELOPER_UNLOCKED.load(std::sync::atomic::Ordering::Relaxed),
        "dev_password_built_in": SecurityManager::password_is_built_in(),
    })
}

#[tauri::command]
pub fn reset_master_password(old_pass: String, new_pass: String) -> Result<String, String> {
    SecurityManager::reset_master_pass(&old_pass, &new_pass)
}

/// Developer: create a single-use invite on the licence server (needs developer credentials:
/// %LOCALAPPDATA%\\Diviora\\developer.json or DIVIORA_ADMIN_TOKEN).
#[tauri::command]
pub fn generate_invite_token(days_valid: u32) -> Result<String, String> {
    require_developer()?;
    let dev = diviora_client::state::developer_credentials().ok_or_else(|| {
        "No licence server configured for the developer. See DEVELOPER_GUIDE.md, section \"Licence server\".".to_string()
    })?;
    diviora_client::server::admin_create_invite(&dev.server_url, &dev.admin_token, days_valid.max(1))
}

#[tauri::command]
pub fn sync_project_to_github(commit_msg: String) -> Result<String, String> {
    require_developer()?;
    let root = find_repo_root().ok_or_else(|| "Not running from a project checkout: Git sync is a developer-machine feature.".to_string())?;
    let run = |args: &[&str]| -> Result<String, String> {
        let o = std::process::Command::new("git").args(args).current_dir(&root).output()
            .map_err(|e| format!("git not available: {}", e))?;
        let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        if o.status.success() { Ok(text) } else { Err(format!("git {} failed:\n{}", args.join(" "), text)) }
    };
    let msg = if commit_msg.trim().is_empty() { "Diviora update".to_string() } else { commit_msg };
    run(&["add", "-A"])?;
    let commit = match run(&["commit", "-m", &msg]) {
        Ok(t) => t,
        Err(e) if e.contains("nothing to commit") => "Nothing new to commit.".to_string(),
        Err(e) => return Err(e),
    };
    let push = run(&["push", "-u", "origin", "HEAD"])?;
    Ok(format!("{}\n{}", commit.trim(), push.trim()))
}

#[tauri::command]
pub fn export_processed_audio_file(source_path: String, _manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    require_access()?;
    let (mut samples, sr, ch) = decode_audio_file(&source_path)?;
    let payload = effective_config().unwrap_or_default();
    let stages = diviora_dsp::analysis::process_file_samples(&payload, &mut samples, sr, ch as usize);
    let stem = std::path::Path::new(&source_path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "track".into());
    let wav = diviora_dsp::analysis::wav_f32(&samples, sr, ch);
    let out = if cfg!(target_os = "android") {
        // Android: keep it in the app library (appears in the playlist).
        PathBuf::from(crate::library::add_bytes(&format!("{}_DIVIORA_PROCESSED.wav", stem), &wav, true)?.path)
    } else {
        let out = desktop_file(&format!("{}_DIVIORA_PROCESSED.wav", stem));
        fs::write(&out, &wav).map_err(|e| format!("Cannot write {}: {}", out.display(), e))?;
        out
    };
    let wm = payload.contains("DivioraWatermark:");
    Ok(format!(
        "{} ({} stage(s) applied{}, {:.1} s, {} Hz, {} ch)",
        out.display(), stages, if wm { ", watermark embedded" } else { "" },
        samples.len() as f64 / (sr as f64 * ch as f64), sr, ch
    ))
}

/// Install or repair the system-wide APO (asks for administrator rights).
#[tauri::command]
pub fn export_cpp_apo_bootstrap_script() -> Result<String, String> {
    require_developer()?;
    let script = find_installer_script()
        .ok_or_else(|| "install_native_apo.ps1 not found next to the app or in the project folder.".to_string())?;
    let arg = format!(
        "Start-Process powershell -Verb RunAs -ArgumentList '-NoExit','-ExecutionPolicy','Bypass','-File','\"{}\"'",
        script.display()
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &arg])
        .spawn()
        .map_err(|e| format!("Could not start the installer: {}", e))?;
    Ok(format!("Installer started (approve the administrator prompt): {}", script.display()))
}

#[tauri::command]
pub fn set_watermark_time_frame(start_sec: u32, end_sec: u32, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    require_developer()?;
    {
        let mgr = manager.inner();
        let mutex_lock = mgr.lock().unwrap();
        *mutex_lock.watermark_time_range.lock().unwrap() = (start_sec as f64, end_sec as f64);
    }
    {
        let mut wm = watermark_state().lock().unwrap();
        wm.spec.start_s = start_sec as f64;
        wm.spec.end_s = end_sec as f64;
    }
    let applied = reapply_if_locked()?;
    Ok(format!("Watermark window: {}s to {}s of every stream.{}", start_sec, end_sec, applied_note(applied)))
}

/// Signature ID, carrier frequency, amplitude, burst interval and auto (signal-gated) mode.
#[tauri::command]
pub fn set_watermark_params(signature_id: String, carrier_hz: f64, amplitude: f64, interval_sec: f64, auto_mode: bool) -> Result<String, String> {
    require_developer()?;
    let line = format!("Id {} Freq {} Amp {} Interval {} Auto {}", signature_id.trim(), carrier_hz, amplitude, interval_sec.max(0.0), auto_mode as u8);
    let parsed = diviora_dsp::watermark::parse_watermark_line(&line)?;
    {
        let mut wm = watermark_state().lock().unwrap();
        wm.spec.id = parsed.id;
        wm.spec.freq = parsed.freq;
        wm.spec.amp = parsed.amp;
        wm.spec.interval_s = parsed.interval_s;
        wm.spec.gate_on_signal = parsed.gate_on_signal;
    }
    let applied = reapply_if_locked()?;
    Ok(format!(
        "Watermark set: ID {}, {} Hz, amplitude {} ({:.1} dBFS), {}.{}",
        signature_id.trim(), carrier_hz, amplitude, 20.0 * amplitude.max(1e-9).log10(),
        if interval_sec > 0.0 { format!("burst every {} s", interval_sec) } else { "continuous".to_string() },
        applied_note(applied)
    ))
}

#[tauri::command]
pub fn clear_watermark_signature(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    require_developer()?;
    {
        let mgr = manager.inner();
        let mutex_lock = mgr.lock().unwrap();
        mutex_lock.watermark_signature.lock().unwrap().clear();
    }
    watermark_state().lock().unwrap().spec.clip = None;
    let applied = reapply_if_locked()?;
    Ok(format!("Watermark audio removed; using the generated frequency signature.{}", applied_note(applied)))
}

#[tauri::command]
pub fn ingest_dropped_paths(paths: Vec<String>, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<serde_json::Value, String> {
    let mgr = manager.inner();
    let mutex_lock = mgr.lock().unwrap();
    let mut flat_files = Vec::new();
    for path_str in paths {
        flat_files.extend(FileSystemScanner::flatten_directory(&PathBuf::from(&path_str)));
    }
    let count = flat_files.len();
    if !flat_files.is_empty() {
        if let Ok(mut track_guard) = mutex_lock.current_track_path.lock() {
            *track_guard = flat_files[0].to_string_lossy().to_string();
        }
    }
    Ok(serde_json::json!({ "status": "success", "items_ingested": count }))
}

/// Report of the active configuration and the APO status. Never contains the configuration itself.
#[tauri::command]
pub fn generate_apo_verification_report(config_data: String) -> Result<String, String> {
    require_developer()?;
    let _ = config_data; // ignored on purpose: the report must not leak the configuration
    let payload = compose_apo_payload();
    let analysis = diviora_dsp::analysis::analyze(&payload, 48000, 2, None);
    let status = crate::apo_config::read_status().map(|v| v.to_string()).unwrap_or_else(|| "no status (APO not running)".into());
    let report = format!(
        "DIVIORA SYSTEM AUDIO REPORT\n\
         Generated: {}\nDevice ID: {}\n\n\
         Configuration fingerprint (SHA-256): {}\nProcessing stages: {}\nLines not supported: {}\n\
         Peak gain: {:.2} dB at {:.0} Hz\nLatency: {:.1} ms\n\
         Locked config file present: {}\nWatermark active: {}\n\nAPO status (from audiodg.exe):\n{}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), SecurityManager::generate_hwid(),
        sha256_hex(payload.as_bytes()), analysis.stages, analysis.warnings.len(),
        analysis.peak_gain_db, analysis.peak_freq_hz, analysis.latency_ms,
        crate::apo_config::config_path().exists(), watermark_state().lock().unwrap().active, status
    );
    let path = desktop_file("DIVIORA_System_Audio_Report.txt");
    fs::write(&path, report).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn generate_watermark_audit_report(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    require_developer()?;
    let track = current_track(&manager);
    let detection = if track.is_empty() {
        "No track loaded: load a processed file in the player to check it for the watermark.".to_string()
    } else {
        match detect_watermark_in_file(&track) {
            Ok(v) => format!("Checked file: {}\nWatermark detected: {} (score {:.2}, at {:.1} s)",
                track, v["detected"], v["score"].as_f64().unwrap_or(0.0), v["at_seconds"].as_f64().unwrap_or(0.0)),
            Err(e) => format!("Checked file: {}\nCould not analyse: {}", track, e),
        }
    };
    let desktop_path = desktop_file("DIVIORA_Watermark_Audit_Report.txt");
    let wm = watermark_state().lock().unwrap();
    let report = format!(
        "DIVIORA WATERMARK SETTINGS REPORT\n\
         Active: {}\nSignature ID: {}\nCarrier: {} Hz\nAmplitude: {} ({:.1} dBFS)\n\
         Source: {}\nWindow: {} s to {} s of each stream\nInterval: {} s (0 = continuous)\nAuto (only under programme audio): {}\n\
         Locked config present: {}\n\
         \nDETECTION\n{}\n",
        wm.active, wm.spec.id, wm.spec.freq, wm.spec.amp, 20.0 * wm.spec.amp.max(1e-9).log10(),
        match &wm.spec.clip { Some(c) => format!("uploaded audio ({:.1} s)", c.len() as f64 / wm.spec.clip_rate as f64), None => "generated frequency signature".to_string() },
        wm.spec.start_s, wm.spec.end_s, wm.spec.interval_s, wm.spec.gate_on_signal,
        crate::apo_config::config_path().exists(), detection
    );
    fs::write(&desktop_path, report).map_err(|e| e.to_string())?;
    Ok(desktop_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn perform_real_dll_apo_audit() -> Result<serde_json::Value, String> {
    require_developer()?;
    let clsid = "{D1B100A1-8888-4444-9999-000000000001}";
    #[cfg(windows)]
    let is_com_registered = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(format!("SOFTWARE\\Classes\\CLSID\\{}", clsid))
        .is_ok();
    #[cfg(not(windows))]
    let is_com_registered = { let _ = clsid; false };
    let has_locked_config = crate::apo_config::config_path().exists();

    // Written once a second by DivioraAPO.dll itself, from inside audiodg.exe.
    let st = crate::apo_config::read_status();
    let g = |k: &str| st.as_ref().and_then(|v| v.get(k)).and_then(|v| v.as_i64()).unwrap_or(0);
    let process = st.as_ref().and_then(|v| v.get("process")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let last_error = st.as_ref().and_then(|v| v.get("last_error")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let config_loaded = st.as_ref().and_then(|v| v.get("config_loaded")).and_then(|v| v.as_bool()).unwrap_or(false);
    let tick_ms = g("tick_ms");
    let pid = g("pid");
    let (process_calls, processed_frames, filtered_frames, locked) = (g("process_calls"), g("processed_frames"), g("filtered_frames"), g("locked"));

    // Fresh = file updated in the last few seconds (the APO rewrites it every second).
    let fresh = std::fs::metadata(crate::apo_config::status_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() < 5)
        .unwrap_or(false);
    let in_audiodg = fresh && process.eq_ignore_ascii_case("audiodg.exe");

    let hook_status = if in_audiodg && config_loaded && filtered_frames > 0 {
        "ACTIVE"
    } else if in_audiodg && process_calls > 0 {
        "PASSTHROUGH_ACTIVE"
    } else if in_audiodg {
        "LOADED_IDLE"
    } else if is_com_registered {
        "BOUND_READY"
    } else {
        "UNLINKED"
    };
    let pid_str = if in_audiodg { pid.to_string() } else { "Offline".to_string() };
    let msg_str = match hook_status {
        "ACTIVE" => format!("DivioraAPO in audiodg.exe | Locked config applied | Filtered frames: {}", filtered_frames),
        "PASSTHROUGH_ACTIVE" => format!("DivioraAPO in audiodg.exe | Pass-through ({}) | Frames: {}", last_error, processed_frames),
        "LOADED_IDLE" => "DivioraAPO loaded in audiodg.exe | Waiting for audio".to_string(),
        "BOUND_READY" => "APO registered, not yet loaded by Windows audio (reboot after install, then play audio)".to_string(),
        _ => "APO not installed".to_string(),
    };
    let is_enabled = hook_status == "ACTIVE";

    Ok(serde_json::json!({
        "status": hook_status,
        "hook_status": hook_status,
        "active_status": hook_status,
        "channel_status": msg_str,
        "is_enabled": is_enabled,
        "sync_enabled": is_enabled,
        "apo_registered": is_com_registered,
        "pid": pid_str,
        "audiodg_pid": pid_str,
        "message": msg_str,
        "status_message": msg_str,
        "has_active_config": has_locked_config,
        "audio_processing_verified": in_audiodg && process_calls > 0,
        "apo_instantiated": in_audiodg,
        "lock_calls": locked,
        "process_calls": process_calls,
        "processed_frames": processed_frames,
        "filtered_frames": filtered_frames,
        "loaded_revision": g("config_revision"),
        "format_valid": g("non_float_streams") == 0,
        "last_error": last_error,
        "status_tick_ms": tick_ms
    }))
}

/// Detect the frequency-signature watermark in the currently loaded track.
#[tauri::command]
pub fn perform_real_watermark_fft_audit(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<serde_json::Value, String> {
    require_developer()?;
    let path = current_track(&manager);
    if path.is_empty() {
        return Err("Load an audio file (for example a downloaded processed file) first.".into());
    }
    detect_watermark_in_file(&path)
}

/// Watermark detection on any audio file.
#[tauri::command]
pub fn verify_watermark_in_file(path: String) -> Result<serde_json::Value, String> {
    require_developer()?;
    detect_watermark_in_file(&path)
}

#[tauri::command]
pub fn check_playback_queue_status(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<serde_json::Value, String> {
    let mgr = manager.inner().lock().unwrap();
    let status = mgr.status.lock().unwrap().clone();
    let (state, detail) = match &status {
        PlaybackStatus::Idle => ("idle", String::new()),
        PlaybackStatus::Playing(_) if mgr.is_paused() => ("paused", String::new()),
        PlaybackStatus::Playing(_) => ("playing", String::new()),
        PlaybackStatus::Stopped => ("stopped", String::new()),
        PlaybackStatus::Finished => ("finished", String::new()),
        PlaybackStatus::Failed(e) => ("failed", e.clone()),
    };
    let peak = mgr.peak();
    let level_db = if peak > 1e-6 { 20.0 * (peak as f64).log10() } else { -120.0 };
    Ok(serde_json::json!({
        "is_active": state == "playing",
        "state": state,
        "error": detail,
        "output_device": mgr.output_device.lock().unwrap().clone(),
        "mode": mgr.mode.lock().unwrap().clone(),
        "level_db": level_db,
        "position_s": mgr.position_secs(),
    }))
}

#[tauri::command]
pub fn get_live_amplitude_metrics(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<f32, String> {
    Ok(manager.inner().lock().unwrap().peak())
}

#[tauri::command]
pub fn enforce_global_hardware_apo(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    require_developer()?;
    let payload = compose_apo_payload();
    if payload.trim().is_empty() {
        return Err("Upload a configuration file (or enable the watermark) first, then press Lock Config.".to_string());
    }
    let path = crate::apo_config::write_locked_config(&payload)?;
    crate::apo_config::remove_legacy_plaintext();
    *PROFILE_LOCKED.lock().unwrap() = true;
    {
        let mgr = manager.inner();
        let mutex_lock = mgr.lock().unwrap();
        *mutex_lock.is_global_apo_locked.lock().unwrap() = true;
    }
    // Developer machine with a licence server: publish, so every activated device updates.
    let published = match diviora_client::state::developer_credentials() {
        Some(dev) => match diviora_client::server::admin_publish_config(&dev.server_url, &dev.admin_token, &payload) {
            Ok(v) => format!(" Published to all users as version {}.", v),
            Err(e) => format!(" NOT published to users: {}", e),
        },
        None => String::new(),
    };
    Ok(format!("Master configuration encrypted and locked: {} (readable only by Windows audio and Administrators).{}", path.display(), published))
}

#[tauri::command]
pub fn get_device_hwid() -> Result<String, String> {
    Ok(SecurityManager::generate_hwid())
}

/// Consumer activation: a licence key (DVL-...) or an invite token (DVI-...).
/// With a licence server the binding "one ID = one device" is enforced by the server;
/// without one, by the local cache only.
#[tauri::command]
pub fn authenticate_user_session(user_id: String, hwid: String) -> Result<String, String> {
    let key = user_id.trim().to_uppercase();
    let device = diviora_client::device::device_id();
    let _ = hwid; // the UI passes it; the device ID is always computed here
    let mut st = diviora_client::state::ClientState::load();
    let Some(url) = diviora_client::state::server_url(&st) else {
        SecurityManager::verify_session_exclusivity(&key, &device)?;
        st.license_key = key;
        st.device_id = device;
        st.session = "offline".into();
        st.save()?;
        return Ok("Activated on this device (test mode: no licence server is configured, so the key is not checked online).".to_string());
    };
    let session = if key.starts_with("DVI-") {
        diviora_client::server::redeem_invite(&url, &key, &device)?
    } else {
        diviora_client::server::activate(&url, &key, &device)?
    };
    st.server_url = url;
    st.license_key = session.license_key.clone();
    st.device_id = device;
    st.session = session.session;
    st.session_expires_at = session.session_expires_at;
    st.licence_expires_at = session.licence_expires_at;
    st.save()?;
    let until = chrono::DateTime::from_timestamp(session.licence_expires_at, 0)
        .map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
    Ok(format!("Activated: licence {} is bound to this device until {}. Updates from the developer are applied automatically.",
        session.license_key, until))
}

#[tauri::command]
pub fn upload_signature(signature: Vec<u8>, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    require_developer()?;
    use rodio::Source;
    let decoder = rodio::Decoder::new(std::io::Cursor::new(signature.clone()))
        .map_err(|_| "Not a supported audio file. Upload a WAV, MP3, FLAC or OGG clip.".to_string())?;
    let channels = decoder.channels().max(1) as usize;
    let rate = decoder.sample_rate().max(1);
    let max_samples = (diviora_dsp::watermark::MAX_CLIP_SECONDS * rate as f64) as usize * channels;
    let interleaved: Vec<i16> = decoder.take(max_samples).collect();
    let mono: Vec<f32> = interleaved
        .chunks(channels)
        .map(|f| f.iter().map(|s| *s as f32 / 32768.0).sum::<f32>() / channels as f32)
        .collect();
    if mono.len() < (rate as usize) / 10 {
        return Err("Watermark clip is shorter than 0.1 s.".to_string());
    }
    // Normalise so the configured Amplitude is the clip's peak level.
    let peak = mono.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    let clip: Vec<f32> = diviora_dsp::watermark::resample_linear(&mono, rate, 48000).iter().map(|s| s / peak).collect();
    let seconds = clip.len() as f64 / 48000.0;
    {
        let mgr = manager.inner();
        let mutex_lock = mgr.lock().unwrap();
        *mutex_lock.watermark_signature.lock().unwrap() = signature;
    }
    {
        let mut wm = watermark_state().lock().unwrap();
        wm.spec.clip = Some(clip);
        wm.spec.clip_rate = 48000;
    }
    let applied = reapply_if_locked()?;
    Ok(format!("Watermark audio loaded ({:.1} s); it replaces the generated tone.{}", seconds, applied_note(applied)))
}

#[tauri::command]
pub fn toggle_watermark(active: bool, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<bool, String> {
    require_developer()?;
    {
        let mgr = manager.inner();
        let mutex_lock = mgr.lock().unwrap();
        *mutex_lock.watermark_active.lock().unwrap() = active;
    }
    watermark_state().lock().unwrap().active = active;
    reapply_if_locked()?;
    Ok(active)
}

#[tauri::command]
pub fn verify_watermark_presence(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<bool, String> {
    require_developer()?;
    let path = current_track(&manager);
    if path.is_empty() { return Ok(false); }
    Ok(detect_watermark_in_file(&path)?.get("detected").and_then(|v| v.as_bool()).unwrap_or(false))
}

// ---------------------------------------------------------------- music library

/// Import one audio file into the library. The frontend sends the raw bytes as the request
/// body and the file name in the `x-file-name` header (URI-encoded).
#[tauri::command]
pub fn import_track(request: tauri::ipc::Request<'_>) -> Result<crate::library::Track, String> {
    require_access()?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("Expected the file contents as raw bytes.".into());
    };
    let name = request.headers().get("x-file-name").and_then(|v| v.to_str().ok())
        .map(|v| percent_decode(v)).unwrap_or_else(|| "track".into());
    // Must be playable audio.
    rodio::Decoder::new(std::io::Cursor::new(bytes.clone()))
        .map_err(|_| format!("{}: not a supported audio file (WAV, MP3, FLAC, OGG).", name))?;
    crate::library::add_bytes(&name, bytes, false)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; continue; }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[tauri::command]
pub fn list_library() -> Vec<crate::library::Track> {
    crate::library::list()
}

#[tauri::command]
pub fn delete_track(id: String) -> Result<bool, String> {
    crate::library::remove(&id)
}

/// What this build can do, for the UI.
#[tauri::command]
pub fn platform_info() -> serde_json::Value {
    serde_json::json!({
        "os": std::env::consts::OS,
        "mobile": cfg!(any(target_os = "android", target_os = "ios")),
        "system_wide_supported": cfg!(windows),
        "system_wide_active": system_apo_active(),
        "activated": diviora_client::state::ClientState::load().is_activated(),
    })
}
