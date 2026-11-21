//! Diviora DSP core.
//!
//! * [`eqapo`]   – parse Equalizer APO 1.4.x configuration files.
//! * [`engine`]  – compile them into a real-time-safe biquad/gain/delay/mixer engine.
//! * [`camilla`] – export them as CamillaDSP YAML.
//! * [`ffi`]     – C ABI for the native Windows APO.

pub mod analysis;
pub mod camilla;
pub mod engine;
pub mod eqapo;
pub mod ffi;
pub mod watermark;

pub use engine::Engine;
pub use eqapo::{parse, EqApoConfig};
