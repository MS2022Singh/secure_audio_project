use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::fs::File;
use std::io::BufReader;

pub struct PlaybackManager {
    pub stream_handle: Option<OutputStreamHandle>,
    pub pipeline: Option<Sink>,
    pub current_track_path: Arc<Mutex<String>>,
    pub queue: Arc<Mutex<Vec<PathBuf>>>,
    pub watermark_signature: Arc<Mutex<Vec<u8>>>,
    pub watermark_active: Arc<Mutex<bool>>,
    pub watermark_time_range: Arc<Mutex<(u32, u32)>>,
    pub active_amplitude: Arc<Mutex<f32>>,
    pub is_global_apo_locked: Arc<Mutex<bool>>,
}

impl PlaybackManager {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<OutputStreamHandle>();

        // Persistent background thread keeps Windows WASAPI output stream open
        std::thread::spawn(move || {
            if let Ok((_stream, handle)) = OutputStream::try_default() {
                let _ = tx.send(handle);
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
        });

        let stream_handle = rx.recv_timeout(std::time::Duration::from_secs(2)).ok();
        let pipeline = stream_handle.as_ref().and_then(|h| {
            Sink::try_new(h).ok().map(|s| {
                s.set_volume(1.0);
                s
            })
        });

        Self {
            stream_handle,
            pipeline,
            current_track_path: Arc::new(Mutex::new(String::new())),
            queue: Arc::new(Mutex::new(Vec::new())),
            watermark_signature: Arc::new(Mutex::new(Vec::new())),
            watermark_active: Arc::new(Mutex::new(false)),
            watermark_time_range: Arc::new(Mutex::new((0, 3600))),
            active_amplitude: Arc::new(Mutex::new(0.0)),
            is_global_apo_locked: Arc::new(Mutex::new(false)),
        }
    }

    pub fn play_local_file(&mut self, path: PathBuf) -> Result<(), String> {
        let handle = self.stream_handle.as_ref().ok_or("No audio output hardware initialized.")?;
        
        let sink = Sink::try_new(handle).map_err(|e| format!("Sink creation failed: {}", e))?;
        sink.set_volume(1.0);

        let clean_path = path.to_string_lossy().replace("/", "\\");
        let abs_path = PathBuf::from(clean_path);

        let file = File::open(&abs_path)
            .or_else(|_| File::open(&path))
            .map_err(|e| format!("Cannot open file {:?}: {}", path, e))?;

        let reader = BufReader::new(file);
        let source = Decoder::new(reader).map_err(|e| format!("Audio decode error: {}", e))?;

        sink.append(source);
        sink.play();
        self.pipeline = Some(sink);
        Ok(())
    }

    pub fn pause(&self) {
        if let Some(ref pipe) = self.pipeline {
            pipe.pause();
        }
    }

    pub fn stop(&mut self) {
        if let Some(ref pipe) = self.pipeline {
            pipe.stop();
        }
        self.pipeline = None;
    }

    pub fn play_stream_bytes(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let handle = self.stream_handle.as_ref().ok_or("No audio output stream device.")?;
        let sink = Sink::try_new(handle).map_err(|e| format!("Sink creation failed: {}", e))?;
        sink.set_volume(1.0);

        let cursor = std::io::Cursor::new(bytes);
        let source = Decoder::new(cursor).map_err(|e| format!("Stream decode error: {}", e))?;
        
        sink.append(source);
        sink.play();
        self.pipeline = Some(sink);
        Ok(())
    }
}