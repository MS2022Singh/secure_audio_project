use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use rodio::{Decoder, OutputStream, Sink};

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackStatus {
    Idle,
    Playing(String),
    Stopped,
    Failed(String),
}

pub struct PlaybackManager {
    pub current_track_path: Arc<Mutex<String>>,
    pub watermark_time_range: Arc<Mutex<(f64, f64)>>,
    pub watermark_signature: Arc<Mutex<Vec<u8>>>,
    pub watermark_active: Arc<Mutex<bool>>,
    pub active_amplitude: Arc<Mutex<f32>>,
    pub is_global_apo_locked: Arc<Mutex<bool>>,
    pub status: Arc<Mutex<PlaybackStatus>>,
    generation: Arc<AtomicU64>,
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
            generation: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn play_file(&self, path_str: String) -> Result<(), String> {
        if path_str.starts_with("http://") || path_str.starts_with("https://") {
            let err = "Network stream playback is not supported. Load a local WAV file.".to_string();
            *self.status.lock().unwrap() = PlaybackStatus::Failed(err.clone());
            return Err(err);
        }

        let path = Path::new(&path_str);
        if !path.exists() {
            let err = format!("Audio file not found: {}", path_str);
            *self.status.lock().unwrap() = PlaybackStatus::Failed(err.clone());
            return Err(err);
        }

        let file = File::open(path)
            .map_err(|e| format!("Failed to open file: {}", e))?;
        let _decoder = Decoder::new(BufReader::new(file))
            .map_err(|e| format!("Failed to decode audio file format: {}", e))?;

        let new_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let gen_ref = Arc::clone(&self.generation);
        let status_ref = Arc::clone(&self.status);
        let track_path = path_str.clone();

        *self.status.lock().unwrap() = PlaybackStatus::Playing(path_str);

        thread::spawn(move || {
            let (_stream, stream_handle) = match OutputStream::try_default() {
                Ok(val) => val,
                Err(e) => {
                    *status_ref.lock().unwrap() = PlaybackStatus::Failed(format!("Audio device init failed: {}", e));
                    return;
                }
            };

            let sink = match Sink::try_new(&stream_handle) {
                Ok(val) => val,
                Err(e) => {
                    *status_ref.lock().unwrap() = PlaybackStatus::Failed(format!("Sink init failed: {}", e));
                    return;
                }
            };

            if let Ok(file) = File::open(&track_path) {
                if let Ok(source) = Decoder::new(BufReader::new(file)) {
                    sink.append(source);
                    sink.play();

                    while !sink.empty() && gen_ref.load(Ordering::Acquire) == new_gen {
                        thread::sleep(std::time::Duration::from_millis(20));
                    }

                    sink.stop();
                }
            }

            if gen_ref.load(Ordering::Acquire) == new_gen {
                *status_ref.lock().unwrap() = PlaybackStatus::Stopped;
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.status.lock().unwrap() = PlaybackStatus::Stopped;
    }
}