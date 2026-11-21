//! In-app processing: decoded audio passes through the Diviora engine (equalizer +
//! watermark) before it reaches the speakers. Used on Android, and on Windows when the
//! system-wide APO is not active (so audio is never processed twice).
use rodio::Source;
use std::time::Duration;

pub struct ProcessedSource<S: Source<Item = f32>> {
    inner: S,
    engine: diviora_dsp::Engine,
    channels: usize,
    frame: [f32; 8],
    pos: usize, // next sample of `frame` to hand out
    len: usize, // valid samples in `frame`
}

impl<S: Source<Item = f32>> ProcessedSource<S> {
    pub fn new(inner: S, config_text: &str) -> Self {
        let channels = (inner.channels() as usize).clamp(1, 8);
        let cfg = diviora_dsp::eqapo::parse(config_text, None);
        let engine = diviora_dsp::Engine::compile(&cfg.stages_for_all_devices(), inner.sample_rate(), channels, 0);
        ProcessedSource { inner, engine, channels, frame: [0.0; 8], pos: 0, len: 0 }
    }
}

impl<S: Source<Item = f32>> Iterator for ProcessedSource<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.len {
            // Pull one whole frame, process it, then hand the samples out one by one.
            let mut n = 0;
            while n < self.channels {
                match self.inner.next() {
                    Some(s) => { self.frame[n] = s; n += 1; }
                    None => break,
                }
            }
            if n == 0 { return None; }
            if n == self.channels { self.engine.process_frame(&mut self.frame[..n]); }
            self.len = n;
            self.pos = 0;
        }
        let s = self.frame[self.pos];
        self.pos += 1;
        Some(s)
    }
}

impl<S: Source<Item = f32>> Source for ProcessedSource<S> {
    fn current_frame_len(&self) -> Option<usize> { self.inner.current_frame_len() }
    fn channels(&self) -> u16 { self.inner.channels() }
    fn sample_rate(&self) -> u32 { self.inner.sample_rate() }
    fn total_duration(&self) -> Option<Duration> { self.inner.total_duration() }
}
