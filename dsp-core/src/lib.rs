pub mod biquad;
pub mod compiler;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterBand {
    #[serde(rename = "type")]
    pub filter_type: String,

    pub frequency_hz: f64,
    pub gain_db: f64,
    pub q: Option<f64>,
    pub slope: Option<f64>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimiterConfig {
    pub enabled: bool,
    pub ceiling_db: f64,
    pub release_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DspConfig {
    pub schema: String,
    pub version: u32,
    pub revision: u64,
    pub sample_rates: Vec<u32>,
    pub channels: usize,
    pub preamp_db: f64,
    pub bands: Vec<FilterBand>,
    pub channel_matrix: Vec<Vec<f64>>,
    pub limiter: LimiterConfig,
}

impl DspConfig {
    pub fn validate(&self, target_sample_rate: f64) -> Result<(), String> {
        if self.schema != "com.diviora.dsp" {
            return Err("Unsupported DSP schema.".into());
        }

        if self.version != 1 {
            return Err(format!("Unsupported schema version: {}", self.version));
        }

        if self.revision == 0 {
            return Err("Revision must be greater than zero.".into());
        }

        if !target_sample_rate.is_finite() || target_sample_rate <= 0.0 {
            return Err("Invalid target sample rate.".into());
        }

        if self.channels == 0 || self.channels > 8 {
            return Err("Channel count must be between 1 and 8.".into());
        }

        if self.sample_rates.is_empty()
            || !self.sample_rates.contains(&(target_sample_rate as u32))
        {
            return Err("Target sample rate is not supported by this configuration.".into());
        }

        if !self.preamp_db.is_finite() || self.preamp_db < -60.0 || self.preamp_db > 24.0 {
            return Err("Preamp must be finite and between -60 dB and +24 dB.".into());
        }

        if self.channel_matrix.len() != self.channels {
            return Err("Invalid channel matrix row count.".into());
        }

        for row in &self.channel_matrix {
            if row.len() != self.channels {
                return Err("Channel matrix must be square.".into());
            }

            if row.iter().any(|v| !v.is_finite() || v.abs() > 4.0) {
                return Err("Channel matrix contains an invalid coefficient.".into());
            }
        }

        if self.bands.len() > 64 {
            return Err("Too many filter bands (maximum 64 allowed).".into());
        }

        for (index, band) in self.bands.iter().enumerate() {
            if !band.frequency_hz.is_finite()
                || band.frequency_hz <= 0.0
                || band.frequency_hz >= target_sample_rate / 2.0
            {
                return Err(format!("Band {} has an invalid frequency.", index));
            }

            if !band.gain_db.is_finite() || band.gain_db < -60.0 || band.gain_db > 24.0 {
                return Err(format!("Band {} has an invalid gain.", index));
            }

            match band.filter_type.as_str() {
                "peaking" | "lowpass" | "highpass" => {
                    let q = band.q.ok_or_else(|| format!("Band {}: 'q' required for filter type '{}'", index, band.filter_type))?;
                    if !q.is_finite() || q <= 0.01 || q > 100.0 {
                        return Err(format!("Band {} has an invalid Q value.", index));
                    }
                },
                "lowshelf" | "highshelf" => {
                    let slope = band.slope.ok_or_else(|| format!("Band {}: 'slope' required for filter type '{}'", index, band.filter_type))?;
                    if !slope.is_finite() || slope <= 0.01 || slope > 2.0 {
                        return Err(format!("Band {} has an invalid slope value.", index));
                    }
                },
                _ => return Err(format!("Band {} has an unsupported filter type.", index)),
            }
        }

        if !self.limiter.ceiling_db.is_finite()
            || self.limiter.ceiling_db < -60.0
            || self.limiter.ceiling_db > 0.0
        {
            return Err("Limiter ceiling must be between -60 dB and 0 dB.".into());
        }

        if !self.limiter.release_ms.is_finite()
            || self.limiter.release_ms < 1.0
            || self.limiter.release_ms > 5000.0
        {
            return Err("Limiter release time is invalid.".into());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::*;

    fn sample_config() -> DspConfig {
        DspConfig {
            schema: "com.diviora.dsp".into(),
            version: 1,
            revision: 1,
            sample_rates: vec![44100, 48000, 96000],
            channels: 2,
            preamp_db: 0.0,
            bands: vec![FilterBand {
                filter_type: "peaking".into(),
                frequency_hz: 1000.0,
                gain_db: -3.0,
                q: Some(1.0),
                slope: None,
                enabled: true,
            }],
            channel_matrix: vec![vec![1.0, 0.0], vec![0.0, 1.0]],
            limiter: LimiterConfig {
                enabled: true,
                ceiling_db: -1.0,
                release_ms: 100.0,
            },
        }
    }

    #[test]
    fn test_valid_configuration() {
        let config = sample_config();
        assert!(config.validate(48000.0).is_ok());
    }

    #[test]
    fn test_peaking_filter_steady_state_gain() {
        let config = sample_config();
        let compiled = CompiledDspState::compile(&config, 48000.0).unwrap();
        let mut processor = AudioProcessor::new(compiled, 2).unwrap();

        let sample_rate = 48000.0f32;
        let freq = 1000.0f32;
        let mut buffer = vec![0.0f32; 9600]; // 100 ms stereo
        for i in 0..4800 {
            let t = i as f32 / sample_rate;
            let val = (2.0 * std::f32::consts::PI * freq * t).sin();
            buffer[i * 2] = val;
            buffer[i * 2 + 1] = val;
        }

        processor.process_interleaved(&mut buffer, 2).unwrap();

        // Discard initial 3000 samples (transient phase) and measure steady-state peak
        let mut max_amp = 0.0f32;
        for i in 6000..9600 {
            max_amp = max_amp.max(buffer[i].abs());
        }

        let expected_linear = 10.0f32.powf(-3.0 / 20.0);
        assert!((max_amp - expected_linear).abs() < 0.02, "Measured peak amplitude {} differs from expected {}", max_amp, expected_linear);
    }

    #[test]
    fn test_channel_count_mismatch_error() {
        let config = sample_config();
        let compiled = CompiledDspState::compile(&config, 48000.0).unwrap();
        let mut processor = AudioProcessor::new(compiled, 2).unwrap(); // Initialized for 2 channels

        let mut buffer = vec![0.0f32; 8];
        let result = processor.process_interleaved(&mut buffer, 4); // Attempting to process as 4 channels
        assert!(result.is_err(), "Processor must return Err on channel mismatch.");
    }
}