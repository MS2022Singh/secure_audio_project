//! Tauri commands for system-wide processing through the user-mode relay
//! (virtual cable -> Diviora DSP -> real speakers). Replaces the old
//! registry/FxProperties/audiodg.exe hook, which is removed.
//!
//! The uploaded Equalizer APO profile is kept only in process memory.

use diviora_dsp::camilla::{self, CamillaOptions};
use diviora_dsp::eqapo::{self, EqApoConfig};
use diviora_dsp::relay::{self, RelayConfig, RelayHandle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::State;

#[derive(Default)]
pub struct SystemRelayState {
    pub relay: Mutex<Option<RelayHandle>>,
    /// Active profile (None = bypass).
    pub profile: Mutex<Option<EqApoConfig>>,
    pub revision: AtomicU64,
}

impl SystemRelayState {
    fn active_profile(&self) -> EqApoConfig {
        self.profile.lock().unwrap().clone().unwrap_or_default()
    }
}

pub struct ProfileSummary {
    pub stages: usize,
    pub warnings: Vec<String>,
}

/// Parse + activate a profile (or bypass). Called by `commands::upload_apo_profile`.
pub fn apply_profile(state: &SystemRelayState, profile_text: Option<&str>) -> Result<ProfileSummary, String> {
    let parsed = profile_text.map(|t| eqapo::parse(t, None));
    let summary = ProfileSummary {
        stages: parsed.as_ref().map(|p| p.stages.len()).unwrap_or(0),
        warnings: parsed.as_ref().map(|p| p.warnings.iter().map(|w| w.to_string()).collect()).unwrap_or_default(),
    };
    let rev = state.revision.fetch_add(1, Ordering::Relaxed) + 1;
    *state.profile.lock().unwrap() = parsed.clone();
    if let Some(r) = state.relay.lock().unwrap().as_ref() {
        r.set_config(parsed.unwrap_or_default(), rev)?;
    }
    Ok(summary)
}

#[tauri::command]
pub fn list_relay_audio_devices() -> Result<serde_json::Value, String> {
    let (inputs, outputs) = relay::list_devices()?;
    Ok(serde_json::json!({ "inputs": inputs, "outputs": outputs }))
}

#[tauri::command]
pub fn start_system_relay(
    input_device: String,
    output_device: String,
    loopback: Option<bool>,
    buffer_ms: Option<u32>,
    state: State<'_, SystemRelayState>,
) -> Result<serde_json::Value, String> {
    let mut guard = state.relay.lock().unwrap();
    if let Some(old) = guard.take() {
        old.stop();
    }
    let cfg = RelayConfig { input_device, output_device, loopback: loopback.unwrap_or(false), buffer_ms: buffer_ms.unwrap_or(30) };
    let rev = state.revision.load(Ordering::Relaxed);
    let handle = relay::start(cfg, state.active_profile(), rev)?;
    let i = handle.info.clone();
    *guard = Some(handle);
    Ok(serde_json::json!({
        "input_device": i.input_device, "output_device": i.output_device,
        "sample_rate": i.sample_rate, "input_channels": i.input_channels,
        "output_channels": i.output_channels, "buffer_ms": i.buffer_ms
    }))
}

#[tauri::command]
pub fn stop_system_relay(state: State<'_, SystemRelayState>) -> Result<String, String> {
    match state.relay.lock().unwrap().take() {
        Some(h) => { h.stop(); Ok("System relay stopped.".into()) }
        None => Ok("System relay was not running.".into()),
    }
}

#[tauri::command]
pub fn get_system_relay_status(state: State<'_, SystemRelayState>) -> Result<serde_json::Value, String> {
    let guard = state.relay.lock().unwrap();
    let profile_stages = state.profile.lock().unwrap().as_ref().map(|p| p.stages.len()).unwrap_or(0);
    let Some(h) = guard.as_ref() else {
        return Ok(serde_json::json!({ "route_active": false, "profile_stages": profile_stages }));
    };
    let s = &h.stats;
    Ok(serde_json::json!({
        "route_active": !s.device_lost.load(Ordering::Relaxed),
        "device_lost": s.device_lost.load(Ordering::Relaxed),
        "virtual_input_device": h.info.input_device,
        "physical_output_device": h.info.output_device,
        "sample_rate": h.info.sample_rate,
        "channels": h.info.output_channels,
        "buffer_ms": h.info.buffer_ms,
        "frames_processed": s.frames_processed.load(Ordering::Relaxed),
        "underruns": s.underruns.load(Ordering::Relaxed),
        "dropped_samples": s.overflow_samples.load(Ordering::Relaxed),
        "drift_corrections": s.drift_corrections.load(Ordering::Relaxed),
        "stream_errors": s.stream_errors.load(Ordering::Relaxed),
        "active_revision": s.active_revision.load(Ordering::Relaxed),
        "profile_stages": profile_stages
    }))
}

/// Convert an Equalizer APO file to CamillaDSP YAML. Returns one document per
/// profile (`default` plus one per `Device:` block) and the list of lines not converted.
#[tauri::command]
pub fn export_camilladsp_config(
    profile_data: String,
    capture_device: Option<String>,
    playback_device: Option<String>,
    samplerate: Option<u32>,
) -> Result<serde_json::Value, String> {
    let cfg = eqapo::parse(&profile_data, None);
    let opt = CamillaOptions {
        samplerate: samplerate.unwrap_or(48000),
        capture_device,
        playback_device,
        ..Default::default()
    };
    let profiles: Vec<serde_json::Value> = camilla::emit_profiles(&cfg, &opt)
        .into_iter()
        .map(|(name, yaml)| serde_json::json!({ "name": name, "yaml": yaml }))
        .collect();
    let warnings: Vec<String> = cfg.warnings.iter().map(|w| w.to_string()).collect();
    Ok(serde_json::json!({ "profiles": profiles, "warnings": warnings }))
}
