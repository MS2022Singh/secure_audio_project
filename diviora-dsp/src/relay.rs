//! User-mode system-wide audio relay:
//!
//! ```text
//! apps -> [virtual cable, set as Windows default output] -> capture (WASAPI)
//!      -> ring buffer -> Engine (Equalizer APO filters) -> render (WASAPI) -> real speakers
//! ```
//!
//! No driver, registry, audiodg.exe or Secure Boot changes are involved. The only
//! system component needed is an already-signed virtual audio device (for example
//! VB-CABLE, installed by the user from its vendor).
//!
//! Real-time rules: the capture and render callbacks never allocate, lock, parse or
//! do I/O. New filter settings are compiled on the control thread and handed to the
//! render callback through a lock-free SPSC queue; the replaced engine is sent back
//! and freed off the audio thread.

use crate::eqapo::{EqApoConfig, MAX_CHANNELS};
use crate::engine::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

#[derive(Debug, Clone)]
pub struct RelayConfig {
    /// Recording device to capture from, e.g. "CABLE Output". Matched exactly, then by
    /// case-insensitive substring. With `loopback`, this names a *playback* device instead.
    pub input_device: String,
    /// Physical playback device to render to, e.g. "Speakers (Realtek". Must not be the
    /// virtual cable itself.
    pub output_device: String,
    /// Capture `input_device` with WASAPI loopback. Only useful when apps play to a
    /// device you do not listen to; otherwise you hear the audio twice.
    pub loopback: bool,
    /// Target buffering between capture and render, in milliseconds.
    pub buffer_ms: u32,
}

#[derive(Debug, Default)]
pub struct RelayStats {
    pub frames_processed: AtomicU64,
    pub underruns: AtomicU64,
    pub overflow_samples: AtomicU64,
    pub drift_corrections: AtomicU64,
    pub stream_errors: AtomicU64,
    pub active_revision: AtomicU64,
    pub device_lost: AtomicBool,
}

#[derive(Debug, Clone)]
pub struct RelayInfo {
    pub input_device: String,
    pub output_device: String,
    pub sample_rate: u32,
    pub input_channels: usize,
    pub output_channels: usize,
    pub buffer_ms: u32,
}

enum Cmd {
    SetConfig(EqApoConfig, u64),
    Stop,
}

pub struct RelayHandle {
    cmd: mpsc::Sender<Cmd>,
    thread: Option<JoinHandle<()>>,
    pub stats: Arc<RelayStats>,
    pub info: RelayInfo,
}

impl RelayHandle {
    /// Replace the active filter configuration. Takes effect within one audio buffer.
    pub fn set_config(&self, cfg: EqApoConfig, revision: u64) -> Result<(), String> {
        self.cmd.send(Cmd::SetConfig(cfg, revision)).map_err(|_| "relay thread has stopped".to_string())
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.cmd.send(Cmd::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for RelayHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Names of all (recording, playback) devices on the default host (WASAPI on Windows).
pub fn list_devices() -> Result<(Vec<String>, Vec<String>), String> {
    let host = cpal::default_host();
    let ins = host.input_devices().map_err(|e| e.to_string())?.filter_map(|d| d.name().ok()).collect();
    let outs = host.output_devices().map_err(|e| e.to_string())?.filter_map(|d| d.name().ok()).collect();
    Ok((ins, outs))
}

fn find_device(
    devices: impl Iterator<Item = cpal::Device>,
    wanted: &str,
    kind: &str,
) -> Result<(cpal::Device, String), String> {
    let all: Vec<(cpal::Device, String)> = devices.filter_map(|d| d.name().ok().map(|n| (d, n))).collect();
    let w = wanted.to_lowercase();
    let pos = all.iter().position(|(_, n)| n == wanted).or_else(|| all.iter().position(|(_, n)| n.to_lowercase().contains(&w)));
    match pos {
        Some(i) => Ok(all.into_iter().nth(i).unwrap()),
        None => Err(format!(
            "{} device '{}' not found. Available: {}",
            kind,
            wanted,
            all.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(" | ")
        )),
    }
}

/// Start the relay with an initial configuration.
pub fn start(cfg: RelayConfig, initial: EqApoConfig, revision: u64) -> Result<RelayHandle, String> {
    if cfg.input_device.trim().is_empty() || cfg.output_device.trim().is_empty() {
        return Err("Both a capture (virtual cable) device and a physical output device must be chosen.".into());
    }
    let stats = Arc::new(RelayStats::default());
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<RelayInfo, String>>();
    let thread_stats = stats.clone();

    // cpal streams are not Send on every platform, so they live on this thread.
    let thread = std::thread::Builder::new()
        .name("diviora-relay".into())
        .spawn(move || {
            let built = build_streams(&cfg, thread_stats.clone());
            let (input_stream, output_stream, info, mut engine_tx, mut garbage_rx) = match built {
                Ok(b) => b,
                Err(e) => { let _ = ready_tx.send(Err(e)); return; }
            };
            let out_name = info.output_device.clone();
            let compile = |c: &EqApoConfig, rev: u64| {
                Box::new(Engine::compile(&c.stages_for_device(&out_name), info.sample_rate, info.output_channels, rev))
            };
            let _ = engine_tx.push(compile(&initial, revision));
            if let Err(e) = output_stream.play() {
                let _ = ready_tx.send(Err(format!("failed to start output stream: {}", e)));
                return;
            }
            if let Err(e) = input_stream.play() {
                let _ = ready_tx.send(Err(format!("failed to start capture stream: {}", e)));
                return;
            }
            let _ = ready_tx.send(Ok(info.clone()));

            loop {
                let msg = cmd_rx.recv_timeout(std::time::Duration::from_millis(250));
                // Free engines that the audio thread has retired.
                while let Ok(old) = garbage_rx.pop() { drop(old); }
                match msg {
                    Ok(Cmd::SetConfig(c, rev)) => {
                        let mut e = compile(&c, rev);
                        // Queue is tiny; if the audio thread is not draining (device lost), retry briefly.
                        for _ in 0..20 {
                            match engine_tx.push(e) {
                                Ok(()) => break,
                                Err(rtrb::PushError::Full(back)) => {
                                    e = back;
                                    std::thread::sleep(std::time::Duration::from_millis(10));
                                }
                            }
                        }
                    }
                    Ok(Cmd::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            drop(input_stream);
            drop(output_stream);
        })
        .map_err(|e| e.to_string())?;

    match ready_rx.recv() {
        Ok(Ok(info)) => Ok(RelayHandle { cmd: cmd_tx, thread: Some(thread), stats, info }),
        Ok(Err(e)) => { let _ = thread.join(); Err(e) }
        Err(_) => { let _ = thread.join(); Err("relay thread exited during start-up".into()) }
    }
}

type Built = (cpal::Stream, cpal::Stream, RelayInfo, rtrb::Producer<Box<Engine>>, rtrb::Consumer<Box<Engine>>);

fn build_streams(cfg: &RelayConfig, stats: Arc<RelayStats>) -> Result<Built, String> {
    let host = cpal::default_host();

    let (in_dev, in_name) = if cfg.loopback {
        find_device(host.output_devices().map_err(|e| e.to_string())?, &cfg.input_device, "Loopback source")?
    } else {
        find_device(host.input_devices().map_err(|e| e.to_string())?, &cfg.input_device, "Capture")?
    };
    let (out_dev, out_name) = find_device(host.output_devices().map_err(|e| e.to_string())?, &cfg.output_device, "Output")?;

    if in_name == out_name {
        return Err(format!(
            "Capture and output are the same device ('{}'). Output must be your real speakers/headphones, not the virtual cable, or audio will loop.",
            out_name
        ));
    }

    let in_supported = if cfg.loopback { in_dev.default_output_config() } else { in_dev.default_input_config() }
        .map_err(|e| format!("cannot read format of '{}': {}", in_name, e))?;
    let out_supported = out_dev.default_output_config().map_err(|e| format!("cannot read format of '{}': {}", out_name, e))?;

    if in_supported.sample_format() != cpal::SampleFormat::F32 || out_supported.sample_format() != cpal::SampleFormat::F32 {
        return Err("Both devices must use a 32-bit float shared-mode format (the WASAPI default).".into());
    }
    let sr = in_supported.sample_rate().0;
    if out_supported.sample_rate().0 != sr {
        return Err(format!(
            "Sample rates differ: '{}' runs at {} Hz but '{}' runs at {} Hz. Set both to the same rate (e.g. 48000 Hz) in Windows Sound settings > device Properties > Advanced.",
            in_name, sr, out_name, out_supported.sample_rate().0
        ));
    }
    let cin = (in_supported.channels() as usize).min(MAX_CHANNELS);
    let cout = (out_supported.channels() as usize).min(MAX_CHANNELS);
    let in_cfg: cpal::StreamConfig = in_supported.into();
    let out_cfg: cpal::StreamConfig = out_supported.into();
    let in_hw_channels = in_cfg.channels as usize;
    let out_hw_channels = out_cfg.channels as usize;

    let buffer_ms = cfg.buffer_ms.clamp(5, 500);
    let target = (sr as usize * buffer_ms as usize / 1000).max(64) * cin; // samples
    let (mut prod, mut cons) = rtrb::RingBuffer::<f32>::new(target * 8);
    let (engine_tx, mut engine_rx) = rtrb::RingBuffer::<Box<Engine>>::new(4);
    let (mut garbage_tx, garbage_rx) = rtrb::RingBuffer::<Box<Engine>>::new(8);

    // ---- capture callback ----
    let s_in = stats.clone();
    let err_in = stats.clone();
    let input_stream = in_dev
        .build_input_stream(
            &in_cfg,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Keep only the first `cin` channels of each hardware frame.
                for frame in data.chunks_exact(in_hw_channels) {
                    if prod.slots() < cin {
                        s_in.overflow_samples.fetch_add(cin as u64, Ordering::Relaxed);
                        continue;
                    }
                    for &s in &frame[..cin] {
                        let _ = prod.push(s);
                    }
                }
            },
            move |e| {
                err_in.stream_errors.fetch_add(1, Ordering::Relaxed);
                if matches!(e, cpal::StreamError::DeviceNotAvailable) { err_in.device_lost.store(true, Ordering::Relaxed); }
            },
            None,
        )
        .map_err(|e| format!("cannot open capture on '{}': {}", in_name, e))?;

    // ---- render callback ----
    let s_out = stats.clone();
    let err_out = stats.clone();
    let mut engine: Box<Engine> = Box::new(Engine::bypass(sr, cout));
    let mut primed = false;
    let output_stream = out_dev
        .build_output_stream(
            &out_cfg,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if let Ok(new_engine) = engine_rx.pop() {
                    s_out.active_revision.store(new_engine.revision, Ordering::Relaxed);
                    let old = std::mem::replace(&mut engine, new_engine);
                    if let Err(rtrb::PushError::Full(old)) = garbage_tx.push(old) {
                        drop(old); // rare: control thread is behind
                    }
                }
                // Clock drift between the two devices: drop excess if we fall far behind.
                let fill = cons.slots();
                if fill > target * 3 {
                    let excess = (fill - target) / cin * cin;
                    if let Ok(chunk) = cons.read_chunk(excess) { chunk.commit_all(); }
                    s_out.drift_corrections.fetch_add(1, Ordering::Relaxed);
                }
                if !primed && cons.slots() >= target {
                    primed = true;
                }
                let mut frames = 0u64;
                for frame in out.chunks_exact_mut(out_hw_channels) {
                    if !primed || cons.slots() < cin {
                        if primed {
                            primed = false;
                            s_out.underruns.fetch_add(1, Ordering::Relaxed);
                        }
                        frame.fill(0.0);
                        continue;
                    }
                    let mut inp = [0.0f32; MAX_CHANNELS];
                    for v in inp.iter_mut().take(cin) {
                        *v = cons.pop().unwrap_or(0.0);
                    }
                    let mut o = [0.0f32; MAX_CHANNELS];
                    for c in 0..cout {
                        o[c] = if cin == 1 { inp[0] } else if c < cin { inp[c] } else { 0.0 };
                    }
                    engine.process_frame(&mut o[..cout]);
                    frame[..cout].copy_from_slice(&o[..cout]);
                    for s in frame[cout..].iter_mut() { *s = 0.0; }
                    frames += 1;
                }
                s_out.frames_processed.fetch_add(frames, Ordering::Relaxed);
            },
            move |e| {
                err_out.stream_errors.fetch_add(1, Ordering::Relaxed);
                if matches!(e, cpal::StreamError::DeviceNotAvailable) { err_out.device_lost.store(true, Ordering::Relaxed); }
            },
            None,
        )
        .map_err(|e| format!("cannot open output on '{}': {}", out_name, e))?;

    let info = RelayInfo { input_device: in_name, output_device: out_name, sample_rate: sr, input_channels: cin, output_channels: cout, buffer_ms };
    Ok((input_stream, output_stream, info, engine_tx, garbage_rx))
}
