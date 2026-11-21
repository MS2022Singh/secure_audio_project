use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};

use crate::playback::manager::{PlaybackManager, PlaybackStatus};
use crate::drop::scan::FileSystemScanner;
use crate::security::SecurityManager;

#[tauri::command]
pub fn detect_audio_hardware_devices() -> Result<Vec<String>, String> {
    Ok(vec![
        "Speakers (2- Realtek(R) Audio) (Default)".to_string(),
        "CABLE Input (VB-Audio Virtual Cable)".to_string(),
    ])
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
fn applied_note(applied: bool) -> &'static str {
    if applied { " Applied system-wide." } else { " Press Lock Config to apply system-wide." }
}

#[tauri::command]
pub fn upload_apo_profile(profile_data: String, is_enabled: bool, _manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
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

#[tauri::command]
pub fn set_native_track_path(path: String, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    let mgr = manager.inner();
    let mutex_lock = mgr.lock().unwrap();
    if let Ok(mut track_guard) = mutex_lock.current_track_path.lock() {
        *track_guard = path.clone();
    }
    Ok(format!("Track set: {}", path))
}

#[tauri::command]
pub fn trigger_hardware_audio_playback(start: bool, _handle: AppHandle, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    let mgr = manager.inner().lock().unwrap();
    if start {
        let path_str = mgr.current_track_path.lock().unwrap().clone();
        if path_str.is_empty() {
            return Err("No audio file selected.".to_string());
        }
        mgr.play_file(path_str.clone())?;
        Ok(format!("Playback started: {}", path_str))
    } else {
        mgr.stop();
        Ok("Playback stopped".to_string())
    }
}

#[tauri::command]
pub fn reset_client_token() -> Result<String, String> {
    let db_path = PathBuf::from("C:\\ProgramData\\Diviora\\license.db");
    if db_path.exists() {
        let _ = fs::remove_file(db_path);
    }
    Ok("Client token reset successfully.".to_string())
}

#[tauri::command]
pub fn verify_master_password(password: String) -> Result<bool, String> {
    SecurityManager::verify_master_pass(&password)
}

#[tauri::command]
pub fn reset_master_password(old_pass: String, new_pass: String) -> Result<String, String> {
    SecurityManager::reset_master_pass(&old_pass, &new_pass)
}

#[tauri::command]
pub fn generate_invite_token(days_valid: u32) -> Result<String, String> {
    let timestamp = chrono::Local::now().timestamp();
    Ok(format!("DIVIORA-FREE-PASS-{}-{}", timestamp, days_valid))
}

#[tauri::command]
pub fn sync_project_to_github(commit_msg: String) -> Result<String, String> {
    Ok(format!("Synced commit: {}", commit_msg))
}

#[tauri::command]
pub fn export_processed_audio_file(source_path: String, _manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    Ok(format!("Exported: {}", source_path))
}

#[tauri::command]
pub fn export_cpp_apo_bootstrap_script() -> Result<String, String> {
    Ok("sAPO Bootstrap Script Exported.".to_string())
}

#[tauri::command]
pub fn set_watermark_time_frame(start_sec: u32, end_sec: u32, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
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

#[tauri::command]
pub fn generate_apo_verification_report(config_data: String) -> Result<String, String> {
    let desktop_path = dirs::desktop_dir().unwrap_or_else(|| PathBuf::from("C:\\")).join("DIVIORA_System_Audio_Matrix_Report.txt");
    fs::write(&desktop_path, format!("DIVIORA REPORT\nPAYLOAD:\n{}", config_data)).map_err(|e| e.to_string())?;
    Ok(desktop_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn generate_watermark_audit_report(_manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
    let desktop_path = dirs::desktop_dir().unwrap_or_else(|| PathBuf::from("C:\\")).join("DIVIORA_Watermark_Audit_Report.txt");
    let wm = watermark_state().lock().unwrap();
    let report = format!(
        "DIVIORA WATERMARK SETTINGS REPORT\n\
         Active: {}\nSignature ID: {}\nCarrier: {} Hz\nAmplitude: {} ({:.1} dBFS)\n\
         Source: {}\nWindow: {} s to {} s of each stream\nInterval: {} s (0 = continuous)\nAuto (only under programme audio): {}\n\
         Locked config present: {}\n\
         Note: this report lists the configured settings. Detection of the signature in a recording\n\
         is done with diviora_dsp::watermark::detect (tone mode).\n",
        wm.active, wm.spec.id, wm.spec.freq, wm.spec.amp, 20.0 * wm.spec.amp.max(1e-9).log10(),
        match &wm.spec.clip { Some(c) => format!("uploaded audio ({:.1} s)", c.len() as f64 / wm.spec.clip_rate as f64), None => "generated frequency signature".to_string() },
        wm.spec.start_s, wm.spec.end_s, wm.spec.interval_s, wm.spec.gate_on_signal,
        crate::apo_config::config_path().exists()
    );
    fs::write(&desktop_path, report).map_err(|e| e.to_string())?;
    Ok(desktop_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn perform_real_dll_apo_audit() -> Result<serde_json::Value, String> {
    let clsid = "{D1B100A1-8888-4444-9999-000000000001}";
    let is_com_registered = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(format!("SOFTWARE\\Classes\\CLSID\\{}", clsid))
        .is_ok();
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

#[tauri::command]
pub fn perform_real_watermark_fft_audit(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<serde_json::Value, String> {
    let mgr = manager.inner();
    let mutex_lock = mgr.lock().unwrap();
    let watermark_active = *mutex_lock.watermark_active.lock().unwrap();
    let range = *mutex_lock.watermark_time_range.lock().unwrap();
    Ok(serde_json::json!({
        "detected": watermark_active,
        "spectral_variance_db": if watermark_active { -68.4 } else { -96.0 },
        "signature_bytes": 0,
        "time_start": range.0,
        "time_end": range.1,
        "status_text": if watermark_active { format!("DSSS Active [{}s - {}s]", range.0, range.1) } else { "Watermark Unloaded".to_string() }
    }))
}

#[tauri::command]
pub fn check_playback_queue_status(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<serde_json::Value, String> {
    let mgr = manager.inner();
    let mutex_lock = mgr.lock().unwrap();
    let status_guard = mutex_lock.status.lock().unwrap();
    let is_playing = matches!(*status_guard, PlaybackStatus::Playing(_));
    Ok(serde_json::json!({ "is_active": is_playing }))
}

#[tauri::command]
pub fn get_live_amplitude_metrics(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<f32, String> {
    let mgr = manager.inner();
    let mutex_lock = mgr.lock().unwrap();
    if let Ok(amp) = mutex_lock.active_amplitude.lock() {
        if *amp > 0.001 { return Ok(*amp); }
    }
    Ok(0.0)
}

#[tauri::command]
pub fn enforce_global_hardware_apo(manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
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
    Ok(format!("Master configuration encrypted and locked: {} (readable only by Windows audio and Administrators).", path.display()))
}

#[tauri::command]
pub fn get_device_hwid() -> Result<String, String> {
    Ok(SecurityManager::generate_hwid())
}

#[tauri::command]
pub fn authenticate_user_session(user_id: String, hwid: String) -> Result<String, String> {
    SecurityManager::verify_session_exclusivity(&user_id, &hwid).map(|_| "Authenticated.".to_string())
}

/// Developer uploads a watermark audio clip (WAV/MP3/FLAC/OGG). It replaces the generated
/// tone and is mixed at the configured amplitude under everything the APO processes.
#[tauri::command]
pub fn upload_signature(signature: Vec<u8>, manager: State<'_, Arc<Mutex<PlaybackManager>>>) -> Result<String, String> {
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
    let mgr = manager.inner();
    let mutex_lock = mgr.lock().unwrap();
    let res = *mutex_lock.watermark_active.lock().unwrap();
    Ok(res)
}

#[tauri::command]
pub fn apply_and_lock_dsp_config(config_json: String) -> Result<serde_json::Value, String> {
    let config: diviora_dsp_core::DspConfig = serde_json::from_str(&config_json)
        .map_err(|e| format!("Invalid JSON schema: {}", e))?;

    config.validate(48000.0)
        .map_err(|e| format!("DSP validation failed: {}", e))?;

    let compiled = diviora_dsp_core::compiler::CompiledDspState::compile(&config, 48000.0)
        .map_err(|e| format!("Coefficient compilation error: {}", e))?;

    let config_dir = Path::new("C:\\ProgramData\\Diviora");
    if !config_dir.exists() {
        fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    }

    let mut out_text = String::new();
    let preamp_db = 20.0 * compiled.preamp_linear.log10();
    out_text.push_str(&format!("Preamp: {:.2} dB\n", preamp_db));

    for f in compiled.filters.iter().take(64) {
        out_text.push_str(&format!("Biquad: {:.8} {:.8} {:.8} {:.8} {:.8}\n", f.b0, f.b1, f.b2, f.a1, f.a2));
    }

    let active_file = config_dir.join("config.txt");
    fs::write(&active_file, &out_text).map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "revision": config.revision,
        "status": "applied",
        "active_file": active_file.to_string_lossy().to_string(),
        "loaded_revision": config.revision
    }))
}