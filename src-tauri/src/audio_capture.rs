use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub struct AudioStreamBridge {
    pub shared_buffer: Arc<Mutex<Vec<f32>>>,
    pub watermark_signature: Arc<Mutex<Vec<u8>>>,
    pub watermark_active: Arc<Mutex<bool>>,
    pub is_audio_pulsing: Arc<Mutex<bool>>,
    pub is_global_apo_locked: Arc<Mutex<bool>>, 
    pub active_amplitude: Arc<Mutex<f32>>,
    capture_stream: Option<cpal::Stream>,
}

impl AudioStreamBridge {
    pub fn new() -> Self {
        Self {
            shared_buffer: Arc::new(Mutex::new(Vec::with_capacity(8192))),
            watermark_signature: Arc::new(Mutex::new(Vec::new())),
            watermark_active: Arc::new(Mutex::new(false)),
            is_audio_pulsing: Arc::new(Mutex::new(false)),
            is_global_apo_locked: Arc::new(Mutex::new(false)),
            active_amplitude: Arc::new(Mutex::new(0.0)),
            capture_stream: None,
        }
    }

    /// TRUE GLOBAL LOOPBACK CAPTURE: Grabs whatever audio Windows is mixing live (YouTube, Chrome, etc.)
    pub fn start_intercept(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        
        // Target default system output endpoint channel mixer
        let output_device = host.default_output_device()
            .ok_or_else(|| "Failed to bind default system sound device".to_string())?;
            
        // Use default output configuration explicitly mapping hardware sample rates (44.1kHz / 48kHz)
        let config: cpal::StreamConfig = output_device.default_output_config()
            .map_err(|e| format!("Driver config initialization error: {}", e))?.into();

        let buffer_ptr = Arc::clone(&self.shared_buffer);
        let sig_ptr = Arc::clone(&self.watermark_signature);
        let active_ptr = Arc::clone(&self.watermark_active);
        let pulsing_ptr = Arc::clone(&self.is_audio_pulsing);
        let apo_lock_ptr = Arc::clone(&self.is_global_apo_locked);
        let amp_ptr = Arc::clone(&self.active_amplitude);

        let err_cb = |e| { eprintln!("WASAPI stream telemetry dropout callback: {}", e); };
        
        // Shared-mode loopback intercept data frame allocation callback
        let data_cb = move |input_samples: &[f32], _: &cpal::InputCallbackInfo| {
            let mut p_packet = input_samples.to_vec();
            
            // Extract the real-time peak sound pressure amplitude level to push to the UI visualizer
            let mut current_peak: f32 = 0.0;
            for &sample in p_packet.iter() {
                let abs_s = sample.abs();
                if abs_s > current_peak {
                    current_peak = abs_s;
                }
            }
            
            if let Ok(mut amp) = amp_ptr.lock() {
                *amp = current_peak;
            }

            // Determine if active audio signals are passing through Windows channels
            let has_signal = p_packet.iter().any(|&s| s.abs() > 0.005);
            if let Ok(mut pulse) = pulsing_ptr.lock() {
                *pulse = has_signal;
            }

            // PHASE 9: If global APO parameters are locked, perform real-time sample scaling
            if let Ok(locked) = apo_lock_ptr.lock() {
                if *locked && has_signal {
                    for sample in p_packet.iter_mut() {
                        *sample *= 0.92; // Attenuate audio matching filter constraints
                    }
                }
            }

            // PHASE 10: Smoothly inject steganographic parameters without output stream noise corruption
            if let Ok(active) = active_ptr.lock() {
                if *active && has_signal {
                    if let Ok(sig) = sig_ptr.lock() {
                        if !sig.is_empty() {
                            for (i, sample) in p_packet.iter_mut().enumerate() {
                                let sig_val = sig[i % sig.len()] as f32 / 255.0;
                                *sample = (*sample + (sig_val * 0.0001)).clamp(-1.0, 1.0);
                            }
                        }
                    }
                }
            }

            if let Ok(mut buf) = buffer_ptr.lock() {
                buf.clear();
                buf.extend_from_slice(&p_packet);
            }
        };

        // Build the shared loopback stream channel target mapping
        let stream = output_device.build_input_stream(&config, data_cb, err_cb, None)
            .map_err(|e| format!("Failed to configure WASAPI system shared loopback: {}", e))?;

        stream.play().map_err(|e| format!("Failed to initiate system intercept worker thread: {}", e))?;
        self.capture_stream = Some(stream);
        
        Ok(())
    }
}