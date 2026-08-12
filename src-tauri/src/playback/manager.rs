use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

#[allow(dead_code)]
pub struct PlaybackManager {
    _stream: OutputStream,
    pub stream_handle: Option<OutputStreamHandle>,
    pub pipeline: Option<Sink>,
    pub queue: Arc<Mutex<Vec<PathBuf>>>,
    pub current_track_path: Arc<Mutex<String>>,
    pub is_global_apo_locked: Arc<Mutex<bool>>,
    pub watermark_time_range: Arc<Mutex<(f64, f64)>>,
    pub watermark_signature: Arc<Mutex<Vec<u8>>>,
    pub watermark_active: Arc<Mutex<bool>>,
    pub active_amplitude: Arc<Mutex<f32>>,
}

unsafe impl Send for PlaybackManager {}
unsafe impl Sync for PlaybackManager {}

impl PlaybackManager {
    pub fn new() -> Result<Self, String> {
        let (_stream, stream_handle) = OutputStream::try_default().map_err(|e| e.to_string())?;
        let sink = Sink::try_new(&stream_handle).map_err(|e| e.to_string())?;
        
        Ok(Self {
            _stream,
            stream_handle: Some(stream_handle),
            pipeline: Some(sink),
            queue: Arc::new(Mutex::new(Vec::new())),
            current_track_path: Arc::new(Mutex::new(String::new())),
            is_global_apo_locked: Arc::new(Mutex::new(false)),
            watermark_time_range: Arc::new(Mutex::new((0.0, 0.0))),
            watermark_signature: Arc::new(Mutex::new(Vec::new())),
            watermark_active: Arc::new(Mutex::new(false)),
            active_amplitude: Arc::new(Mutex::new(0.0)),
        })
    }

    pub fn play_local_file(&mut self, path: PathBuf) -> Result<(), String> {
        let file = File::open(&path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let source = Decoder::new(reader).map_err(|e| e.to_string())?;

        if let Some(ref sink) = self.pipeline {
            sink.stop();
            sink.append(source);
            sink.play();
        } else if let Some(ref handle) = self.stream_handle {
            let sink = Sink::try_new(handle).map_err(|e| e.to_string())?;
            sink.append(source);
            sink.play();
            self.pipeline = Some(sink);
        }

        if let Ok(mut track_lock) = self.current_track_path.lock() {
            *track_lock = path.to_string_lossy().to_string();
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn play_file(&mut self, path: PathBuf) -> Result<(), String> {
        self.play_local_file(path)
    }

    pub fn pause(&self) {
        if let Some(ref sink) = self.pipeline {
            sink.pause();
        }
    }

    #[allow(dead_code)]
    pub fn resume(&self) {
        if let Some(ref sink) = self.pipeline {
            sink.play();
        }
    }

    pub fn stop(&self) {
        if let Some(ref sink) = self.pipeline {
            sink.stop();
        }
    }

    #[allow(dead_code)]
    pub fn set_volume(&self, volume: f32) {
        if let Some(ref sink) = self.pipeline {
            sink.set_volume(volume);
        }
    }

    #[allow(dead_code)]
    pub fn is_paused(&self) -> bool {
        if let Some(ref sink) = self.pipeline {
            sink.is_paused()
        } else {
            true
        }
    }

    #[allow(dead_code)]
    pub fn play_stream_bytes(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let cursor = std::io::Cursor::new(bytes);
        let source = Decoder::new(cursor).map_err(|e| e.to_string())?;
        
        if let Some(ref sink) = self.pipeline {
            sink.stop();
            sink.append(source);
            sink.play();
        }
        Ok(())
    }
}
