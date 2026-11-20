use crate::{biquad::*, DspConfig};

#[derive(Debug, Clone)]
pub struct CompiledDspState {
    pub preamp_linear: f32,
    pub filters: Vec<BiquadCoefficients>,
    pub channel_matrix: Vec<Vec<f32>>,
}

impl CompiledDspState {
    pub fn compile(config: &DspConfig, sample_rate: f64) -> Result<Self, String> {
        config.validate(sample_rate)?;

        let preamp_linear = 10.0f64.powf(config.preamp_db / 20.0) as f32;

        let mut filters = Vec::new();
        for band in &config.bands {
            if !band.enabled {
                continue;
            }
            let coeffs = match band.filter_type.as_str() {
                "peaking" => {
                    let q = band.q.ok_or_else(|| "Peaking filter requires 'q' parameter".to_string())?;
                    peaking_coefficients(sample_rate, band.frequency_hz, band.gain_db, q)
                },
                "lowpass" => {
                    let q = band.q.ok_or_else(|| "Lowpass filter requires 'q' parameter".to_string())?;
                    lowpass_coefficients(sample_rate, band.frequency_hz, q)
                },
                "highpass" => {
                    let q = band.q.ok_or_else(|| "Highpass filter requires 'q' parameter".to_string())?;
                    highpass_coefficients(sample_rate, band.frequency_hz, q)
                },
                "lowshelf" => {
                    let slope = band.slope.ok_or_else(|| "Lowshelf filter requires 'slope' parameter".to_string())?;
                    lowshelf_coefficients(sample_rate, band.frequency_hz, band.gain_db, slope)
                },
                "highshelf" => {
                    let slope = band.slope.ok_or_else(|| "Highshelf filter requires 'slope' parameter".to_string())?;
                    highshelf_coefficients(sample_rate, band.frequency_hz, band.gain_db, slope)
                },
                _ => return Err(format!("Unsupported filter type: {}", band.filter_type)),
            };
            filters.push(coeffs);
        }

        let channel_matrix = config
            .channel_matrix
            .iter()
            .map(|row| row.iter().map(|&val| val as f32).collect())
            .collect();

        Ok(Self {
            preamp_linear,
            filters,
            channel_matrix,
        })
    }
}

pub struct AudioProcessor {
    pub state: CompiledDspState,
    pub channels: usize,
    pub biquad_states: Vec<Vec<BiquadState>>,
}

impl AudioProcessor {
    pub fn new(state: CompiledDspState, channels: usize) -> Result<Self, String> {
        if channels == 0 || channels > 8 {
            return Err("Supported channels: 1 to 8".into());
        }
        let biquad_states = vec![vec![BiquadState::default(); state.filters.len()]; channels];
        Ok(Self { state, channels, biquad_states })
    }

    #[inline]
    pub fn process_interleaved(&mut self, buffer: &mut [f32], channels: usize) -> Result<(), String> {
        if channels != self.channels {
            return Err("Channel count mismatch.".into());
        }

        if channels == 0 || channels > 8 {
            return Err("Unsupported channel count.".into());
        }

        if buffer.len() % channels != 0 {
            return Err("Incomplete interleaved audio frame.".into());
        }

        for frame in buffer.chunks_exact_mut(channels) {
            let mut input = [0.0f32; 8];
            let mut output = [0.0f32; 8];

            for ch in 0..channels {
                input[ch] = frame[ch] * self.state.preamp_linear;
            }

            for row in 0..channels {
                let mut sum = 0.0f32;
                for col in 0..channels {
                    let weight = if row < self.state.channel_matrix.len() && col < self.state.channel_matrix[row].len() {
                        self.state.channel_matrix[row][col]
                    } else if row == col { 1.0 } else { 0.0 };
                    sum += input[col] * weight;
                }
                output[row] = sum;
            }

            for ch in 0..channels {
                let mut val = output[ch];
                for (f_idx, coeffs) in self.state.filters.iter().enumerate() {
                    val = self.biquad_states[ch][f_idx].process(val, coeffs);
                }
                frame[ch] = val;
            }
        }

        Ok(())
    }
}