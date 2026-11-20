//! C ABI used by the native Windows APO (`windows-apo/src/DivioraAPO.cpp`).
//!
//! Every entry point catches panics, so a bug here can never unwind into
//! `audiodg.exe`. A null engine pointer means "pass audio through unchanged".

use crate::eqapo;
use crate::engine::Engine;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Opaque handle owned by the caller; free with `dvr_engine_destroy`.
pub struct DvrEngine {
    engine: Engine,
}

fn utf16_to_string(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: caller passes a NUL-terminated UTF-16 string; cap the scan length.
    unsafe {
        let mut len = 0usize;
        while len < 4096 && *p.add(len) != 0 {
            len += 1;
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)))
    }
}

/// Compile an Equalizer APO configuration (UTF-8 text) for one stream format.
///
/// * `device_name` – optional NUL-terminated UTF-16 endpoint name used for `Device:` blocks.
/// * `out_stage_count` / `out_warning_count` – optional outputs.
///
/// Returns null if the text has no processing stages (pass-through) or on any error.
#[no_mangle]
pub extern "C" fn dvr_engine_create(
    text: *const u8,
    text_len: usize,
    device_name: *const u16,
    sample_rate: u32,
    channels: u32,
    out_stage_count: *mut u32,
    out_warning_count: *mut u32,
) -> *mut DvrEngine {
    let r = catch_unwind(AssertUnwindSafe(|| {
        if text.is_null() || text_len == 0 || sample_rate == 0 || channels == 0 {
            return (std::ptr::null_mut(), 0u32, 0u32);
        }
        // SAFETY: caller guarantees `text` points to `text_len` readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(text, text_len) };
        let s = String::from_utf8_lossy(bytes);
        let cfg = eqapo::parse(&s, None);
        let stages = match utf16_to_string(device_name) {
            Some(d) if !d.is_empty() => cfg.stages_for_device(&d),
            _ => cfg.stages_for_all_devices(),
        };
        let n_stages = stages.len() as u32;
        let n_warn = cfg.warnings.len() as u32;
        if stages.is_empty() {
            return (std::ptr::null_mut(), 0, n_warn);
        }
        let e = Engine::compile(&stages, sample_rate, channels as usize, 0);
        (Box::into_raw(Box::new(DvrEngine { engine: e })), n_stages, n_warn)
    }));
    let (ptr, n_stages, n_warn) = r.unwrap_or((std::ptr::null_mut(), 0, 0));
    // SAFETY: optional out-pointers, written only if non-null.
    unsafe {
        if !out_stage_count.is_null() { *out_stage_count = n_stages; }
        if !out_warning_count.is_null() { *out_warning_count = n_warn; }
    }
    ptr
}

/// Process `frames` interleaved float frames in place. Real-time safe: no allocation,
/// locking or I/O. Does nothing if `engine` is null or `channels` differs from the
/// channel count the engine was compiled for.
#[no_mangle]
pub extern "C" fn dvr_engine_process(engine: *mut DvrEngine, buffer: *mut f32, frames: u32, channels: u32) {
    if engine.is_null() || buffer.is_null() || frames == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: engine came from dvr_engine_create and is used by one thread at a time;
        // buffer holds frames*channels floats.
        let e = unsafe { &mut (*engine).engine };
        if e.channels() != channels as usize {
            return;
        }
        let buf = unsafe { std::slice::from_raw_parts_mut(buffer, frames as usize * channels as usize) };
        e.process_interleaved(buf);
    }));
}

/// Free an engine. Must not be called on the real-time thread.
#[no_mangle]
pub extern "C" fn dvr_engine_destroy(engine: *mut DvrEngine) {
    if engine.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: pointer came from Box::into_raw in dvr_engine_create.
        drop(unsafe { Box::from_raw(engine) });
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffi_roundtrip_preamp() {
        let text = b"Preamp: -6.0206 dB\n";
        let (mut st, mut wn) = (0u32, 0u32);
        let e = dvr_engine_create(text.as_ptr(), text.len(), std::ptr::null(), 48000, 2, &mut st, &mut wn);
        assert!(!e.is_null());
        assert_eq!((st, wn), (1, 0));
        let mut buf = [0.8f32, -0.8, 0.4, -0.4];
        dvr_engine_process(e, buf.as_mut_ptr(), 2, 2);
        assert!((buf[0] - 0.4).abs() < 1e-4 && (buf[3] + 0.2).abs() < 1e-4);
        // Channel mismatch -> untouched.
        let mut mono = [0.5f32];
        dvr_engine_process(e, mono.as_mut_ptr(), 1, 1);
        assert_eq!(mono[0], 0.5);
        dvr_engine_destroy(e);
    }

    #[test]
    fn ffi_empty_or_device_filtered_is_null() {
        let text = b"# nothing\nGraphicEQ: 25 0\n";
        let mut wn = 0u32;
        assert!(dvr_engine_create(text.as_ptr(), text.len(), std::ptr::null(), 48000, 2, std::ptr::null_mut(), &mut wn).is_null());
        assert_eq!(wn, 1);

        let text = b"Device: Headphones\nPreamp: -3 dB\n";
        let speakers: Vec<u16> = "Speakers (Realtek(R) Audio)\0".encode_utf16().collect();
        let headphones: Vec<u16> = "Headphones (USB)\0".encode_utf16().collect();
        let mk = |n: &Vec<u16>| dvr_engine_create(text.as_ptr(), text.len(), n.as_ptr(), 48000, 2, std::ptr::null_mut(), std::ptr::null_mut());
        assert!(mk(&speakers).is_null());
        let h = mk(&headphones);
        assert!(!h.is_null());
        dvr_engine_destroy(h);
    }

    #[test]
    fn ffi_eq_plus_watermark_end_to_end() {
        // What the APO receives after Lock Config with the watermark enabled.
        let text = b"Preamp: -3 dB\nFilter: ON PK Fc 1000 Hz Gain -6 dB Q 1\nDivioraWatermark: Id dev-key-v1 Freq 16000 Amp 0.0008 Start 0 End 0 Interval 0 Auto 1\n";
        let (mut st, mut wn) = (0u32, 0u32);
        let e = dvr_engine_create(text.as_ptr(), text.len(), std::ptr::null(), 48000, 2, &mut st, &mut wn);
        assert!(!e.is_null());
        assert_eq!((st, wn), (3, 0));
        let sr = 48000.0;
        let mut buf: Vec<f32> = (0..48000 * 3).flat_map(|i| {
            let t = i as f64 / sr;
            let v = (0.3 * (2.0 * std::f64::consts::PI * 300.0 * t).sin()) as f32;
            [v, v]
        }).collect();
        for chunk in buf.chunks_mut(480 * 2) {
            dvr_engine_process(e, chunk.as_mut_ptr(), (chunk.len() / 2) as u32, 2);
        }
        dvr_engine_destroy(e);
        let left: Vec<f32> = buf.iter().step_by(2).copied().collect();
        let d = crate::watermark::detect(&left, 48000, 16000.0, "dev-key-v1");
        assert!(d.found, "{:?}", d);
    }
}
