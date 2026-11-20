use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BiquadCoefficients {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BiquadState {
    pub x1: f32,
    pub x2: f32,
    pub y1: f32,
    pub y2: f32,
}

impl BiquadState {
    #[inline(always)]
    pub fn process(&mut self, x: f32, coeffs: &BiquadCoefficients) -> f32 {
        let y = coeffs.b0 * x + coeffs.b1 * self.x1 + coeffs.b2 * self.x2
            - coeffs.a1 * self.y1 - coeffs.a2 * self.y2;

        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;

        y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

pub fn peaking_coefficients(sample_rate: f64, frequency_hz: f64, gain_db: f64, q: f64) -> BiquadCoefficients {
    let w0 = 2.0 * std::f64::consts::PI * frequency_hz / sample_rate;
    let alpha = w0.sin() / (2.0 * q);
    let a = 10.0f64.powf(gain_db / 40.0);
    let cos_w0 = w0.cos();

    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cos_w0;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha / a;

    BiquadCoefficients {
        b0: (b0 / a0) as f32,
        b1: (b1 / a0) as f32,
        b2: (b2 / a0) as f32,
        a1: (a1 / a0) as f32,
        a2: (a2 / a0) as f32,
    }
}

pub fn lowpass_coefficients(sample_rate: f64, frequency_hz: f64, q: f64) -> BiquadCoefficients {
    let w0 = 2.0 * std::f64::consts::PI * frequency_hz / sample_rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * q);

    let b0 = (1.0 - cos_w0) / 2.0;
    let b1 = 1.0 - cos_w0;
    let b2 = (1.0 - cos_w0) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;

    BiquadCoefficients {
        b0: (b0 / a0) as f32,
        b1: (b1 / a0) as f32,
        b2: (b2 / a0) as f32,
        a1: (a1 / a0) as f32,
        a2: (a2 / a0) as f32,
    }
}

pub fn highpass_coefficients(sample_rate: f64, frequency_hz: f64, q: f64) -> BiquadCoefficients {
    let w0 = 2.0 * std::f64::consts::PI * frequency_hz / sample_rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * q);

    let b0 = (1.0 + cos_w0) / 2.0;
    let b1 = -(1.0 + cos_w0);
    let b2 = (1.0 + cos_w0) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;

    BiquadCoefficients {
        b0: (b0 / a0) as f32,
        b1: (b1 / a0) as f32,
        b2: (b2 / a0) as f32,
        a1: (a1 / a0) as f32,
        a2: (a2 / a0) as f32,
    }
}

pub fn lowshelf_coefficients(sample_rate: f64, frequency_hz: f64, gain_db: f64, slope: f64) -> BiquadCoefficients {
    let w0 = 2.0 * std::f64::consts::PI * frequency_hz / sample_rate;
    let a = 10.0f64.powf(gain_db / 40.0);
    let cos_w0 = w0.cos();
    let alpha = (w0.sin() / 2.0) * (((a + 1.0 / a) * (1.0 / slope - 1.0) + 2.0).max(0.0)).sqrt();
    let beta = 2.0 * a.sqrt() * alpha;

    let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + beta);
    let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
    let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - beta);
    let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + beta;
    let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
    let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - beta;

    BiquadCoefficients {
        b0: (b0 / a0) as f32,
        b1: (b1 / a0) as f32,
        b2: (b2 / a0) as f32,
        a1: (a1 / a0) as f32,
        a2: (a2 / a0) as f32,
    }
}

pub fn highshelf_coefficients(sample_rate: f64, frequency_hz: f64, gain_db: f64, slope: f64) -> BiquadCoefficients {
    let w0 = 2.0 * std::f64::consts::PI * frequency_hz / sample_rate;
    let a = 10.0f64.powf(gain_db / 40.0);
    let cos_w0 = w0.cos();
    let alpha = (w0.sin() / 2.0) * (((a + 1.0 / a) * (1.0 / slope - 1.0) + 2.0).max(0.0)).sqrt();
    let beta = 2.0 * a.sqrt() * alpha;

    let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + beta);
    let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
    let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - beta);
    let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + beta;
    let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
    let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - beta;

    BiquadCoefficients {
        b0: (b0 / a0) as f32,
        b1: (b1 / a0) as f32,
        b2: (b2 / a0) as f32,
        a1: (a1 / a0) as f32,
        a2: (a2 / a0) as f32,
    }
}