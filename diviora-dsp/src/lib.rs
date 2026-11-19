//! Diviora DSP core.
//!
//! * [`eqapo`]   – parse Equalizer APO 1.4.x configuration files.
//! * [`engine`]  – compile them into a real-time-safe biquad/gain/delay/mixer engine.
//! * [`camilla`] – export them as CamillaDSP YAML.
//! * `relay`     – (feature `relay`) virtual-cable -> DSP -> speakers WASAPI relay.

pub mod camilla;
pub mod engine;
pub mod eqapo;
#[cfg(feature = "relay")]
pub mod relay;

pub use engine::Engine;
pub use eqapo::{parse, EqApoConfig};
