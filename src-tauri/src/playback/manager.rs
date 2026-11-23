use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use rodio::{Decoder, OutputStream, Sink, Source};

/// Path value that plays the 440 Hz calibration tone instead of a file.
pub const TEST_TONE: &str = "trigger_test_tone";

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackStatus {
    Idle,
    Playing(String),
    Stopped,
    /// The track played to the end (the UI moves on to the next one).
    Finished,
    Failed(String),
}

/// Live numbers from the audio thread, readable by the UI without locking the audio.
#[derive(Default)]
pub struct Meter {
    peak_bits: AtomicU32, // f32 peak (0..1+) of the last ~50 ms
    frames: AtomicU64,    // frames handed to the output so far
}

pub struct PlaybackManager {
    pub current_track_path: Arc<Mutex<String>>,
    pub watermark_time_range: Arc<Mutex<(f64, f64)>>,
    pub watermark_signature: Arc<Mutex<Vec<u8>>>,
    pub watermark_active: Arc<Mutex<bool>>,
    pub active_amplitude: Arc<Mutex<f32>>,
    pub is_global_apo_locked: Arc<Mutex<bool>>,
    pub status: Arc<Mutex<PlaybackStatus>>,
    /// Name of the speakers/headphones the app plays to, and how the audio is processed.
    pub output_device: Arc<Mutex<String>>,
    pub mode: Arc<Mutex<String>>,
    pub meter: Arc<Meter>,
    pub sample_rate: Arc<AtomicU32>,
    paused: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}

/// Measures what actually goes to the speakers (peak level and position) and replaces
/// invalid samples (NaN/inf from a broken filter) with silence instead of noise.
struct Metered<S: Source<Item = f32>> {
    inner: S,
    meter: Arc<Meter>,
    channels: u64,
    window: u32,
    peak: f32,
    count: u32,
    in_frame: u64,
}

impl<S: Source<Item = f32>> Metered<S> {
    fn new(inner: S, meter: Arc<Meter>) -> Self {
        let channels = inner.channels().max(1) as u64;
        let window = (inner.sample_rate() / 20).max(1) * channels as u32; // ~50 ms
        meter.frames.store(0, Ordering::Relaxed);
        meter.peak_bits.store(0f32.to_bits(), Ordering::Relaxed);
        Metered { inner, meter, channels, window, peak: 0.0, count: 0, in_frame: 0 }
    }
}

impl<S: Source<Item = f32>> Iterator for Metered<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let mut s = self.inner.next()?;
        if !s.is_finite() {
            s = 0.0;
        }
        self.peak = self.peak.max(s.abs());
        self.count += 1;
        if self.count >= self.window {
            self.meter.peak_bits.store(self.peak.to_bits(), Ordering::Relaxed);
            self.peak = 0.0;
            self.count = 0;
        }
        self.in_frame += 1;
        if self.in_frame >= self.channels {
            self.in_frame = 0;
            self.meter.frames.fetch_add(1, Ordering::Relaxed);
        }
        Some(s)
    }
}

impl<S: Source<Item = f32>> Source for Metered<S> {
    fn current_frame_len(&self) -> Option<usize> { self.inner.current_frame_len() }
    fn channels(&self) -> u16 { self.inner.channels() }
    fn sample_rate(&self) -> u32 { self.inner.sample_rate() }
    fn total_duration(&self) -> Option<Duration> { self.inner.total_duration() }
}

/// Name of the default output device (what Windows/Android plays to right now).
pub fn default_output_name() -> String {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    rodio::cpal::default_host()
        .default_output_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_else(|| "no output device found".into())
}

/// All output devices, the default one marked.
pub fn output_device_names() -> Vec<String> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let host = rodio::cpal::default_host();
    let default = default_output_name();
    let mut v: Vec<String> = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    v.sort_by_key(|n| n != &default);
    v.into_iter().map(|n| if n == default { format!("{} (Default)", n) } else { n }).collect()
}

impl PlaybackManager {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            current_track_path: Arc::new(Mutex::new(String::new())),
            watermark_time_range: Arc::new(Mutex::new((0.0, 0.0))),
            watermark_signature: Arc::new(Mutex::new(Vec::new())),
            watermark_active: Arc::new(Mutex::new(false)),
            active_amplitude: Arc::new(Mutex::new(0.0)),
            is_global_apo_locked: Arc::new(Mutex::new(false)),
            status: Arc::new(Mutex::new(PlaybackStatus::Idle)),
            output_device: Arc::new(Mutex::new(String::new())),
            mode: Arc::new(Mutex::new(String::new())),
            meter: Arc::new(Meter::default()),
            sample_rate: Arc::new(AtomicU32::new(0)),
            paused: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn peak(&self) -> f32 {
        f32::from_bits(self.meter.peak_bits.load(Ordering::Relaxed))
    }

    pub fn position_secs(&self) -> f64 {
        let sr = self.sample_rate.load(Ordering::Relaxed);
        if sr == 0 { 0.0 } else { self.meter.frames.load(Ordering::Relaxed) as f64 / sr as f64 }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Pause/resume the current track at its position.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    fn fail(&self, err: String) -> Result<(), String> {
        *self.status.lock().unwrap() = PlaybackStatus::Failed(err.clone());
        Err(err)
    }

    /// `process`: configuration text to apply in the app (None = play unmodified, e.g. when the
    /// system-wide APO already processes all audio).
    pub fn play_file(&self, path_str: String, process: Option<String>, mode: &str) -> Result<(), String> {
        let tone = path_str == TEST_TONE;
        if path_str.starts_with("http://") || path_str.starts_with("https://") {
            return self.fail("Network stream playback is not supported. Add a downloaded audio file.".into());
        }
        if !tone {
            let path = Path::new(&path_str);
            if !path.exists() {
                return self.fail(format!("Audio file not found: {}", path_str));
            }
            let file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
            if let Err(e) = Decoder::new(BufReader::new(file)) {
                return self.fail(format!("This audio format cannot be played ({}). Use WAV, MP3, FLAC or OGG.", e));
            }
        }

        let new_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let gen_ref = Arc::clone(&self.generation);
        let status_ref = Arc::clone(&self.status);
        let meter = Arc::clone(&self.meter);
        let paused = Arc::clone(&self.paused);
        let rate = Arc::clone(&self.sample_rate);
        let device_ref = Arc::clone(&self.output_device);
        let track_path = path_str.clone();
        paused.store(false, Ordering::Relaxed);
        *self.mode.lock().unwrap() = mode.to_string();
        *self.status.lock().unwrap() = PlaybackStatus::Playing(path_str);

        thread::spawn(move || {
            let fail = |e: String| {
                if gen_ref.load(Ordering::Acquire) == new_gen {
                    *status_ref.lock().unwrap() = PlaybackStatus::Failed(e);
                }
            };
            *device_ref.lock().unwrap() = default_output_name();
            let (_stream, stream_handle) = match OutputStream::try_default() {
                Ok(val) => val,
                Err(e) => return fail(format!("Could not open the speakers/headphones: {}", e)),
            };
            let sink = match Sink::try_new(&stream_handle) {
                Ok(val) => val,
                Err(e) => return fail(format!("Could not start audio output: {}", e)),
            };

            let source: Box<dyn Source<Item = f32> + Send> = if tone {
                Box::new(rodio::source::SineWave::new(440.0).amplify(0.2))
            } else {
                let file = match File::open(&track_path) {
                    Ok(f) => f,
                    Err(e) => return fail(format!("Failed to open file: {}", e)),
                };
                let decoded = match Decoder::new(BufReader::new(file)) {
                    Ok(d) => d.convert_samples::<f32>(),
                    Err(e) => return fail(format!("Could not decode the file: {}", e)),
                };
                match process {
                    Some(cfg) => Box::new(crate::playback::processed::ProcessedSource::new(decoded, &cfg)),
                    None => Box::new(decoded),
                }
            };
            rate.store(source.sample_rate(), Ordering::Relaxed);
            sink.append(Metered::new(source, Arc::clone(&meter)));
            sink.play();

            while !sink.empty() && gen_ref.load(Ordering::Acquire) == new_gen {
                let want_pause = paused.load(Ordering::Relaxed);
                if want_pause && !sink.is_paused() {
                    sink.pause();
                } else if !want_pause && sink.is_paused() {
                    sink.play();
                }
                thread::sleep(Duration::from_millis(20));
            }
            let finished = sink.empty();
            sink.stop();
            meter.peak_bits.store(0f32.to_bits(), Ordering::Relaxed);

            if gen_ref.load(Ordering::Acquire) == new_gen {
                *status_ref.lock().unwrap() = if finished { PlaybackStatus::Finished } else { PlaybackStatus::Stopped };
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.paused.store(false, Ordering::Relaxed);
        *self.status.lock().unwrap() = PlaybackStatus::Stopped;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metered_measures_level_and_silences_nan() {
        let tone = rodio::source::SineWave::new(1000.0).amplify(0.5).take_duration(Duration::from_millis(200));
        let meter = Arc::new(Meter::default());
        let out: Vec<f32> = Metered::new(tone, Arc::clone(&meter)).collect();
        assert!(out.iter().all(|s| s.is_finite()));
        let peak = f32::from_bits(meter.peak_bits.load(Ordering::Relaxed));
        assert!(peak > 0.4 && peak <= 0.51, "peak {}", peak);
        assert!(meter.frames.load(Ordering::Relaxed) > 8000);
    }

    #[test]
    fn processed_playback_is_audible() {
        // The in-app chain with a typical config must produce sound, not silence.
        let tone = rodio::source::SineWave::new(1000.0).amplify(0.5).take_duration(Duration::from_millis(300));
        let cfg = "Preamp: -6 dB\nFilter: ON PK Fc 100 Hz Gain 3 dB Q 1\n";
        let out: Vec<f32> = crate::playback::processed::ProcessedSource::new(tone, cfg).collect();
        let peak = out.iter().skip(2000).fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.15, "processed peak {}", peak);
    }
}
