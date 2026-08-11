use std::sync::Arc;
use rodio::{OutputStream, OutputStreamBuilder, Sink};

pub struct AudioOutputPipeline {
    pub _stream: Arc<OutputStream>,
    pub sink: Arc<Sink>,
}

impl AudioOutputPipeline {
    pub fn try_new() -> Result<Self, String> {
        let stream = OutputStreamBuilder::open_default_stream()
            .map_err(|e| format!("Failed to open default audio stream: {e}"))?;

        let sink = Sink::connect_new(stream.mixer());
        sink.set_volume(0.4);

        Ok(Self {
            _stream: Arc::new(stream),
            sink: Arc::new(sink),
        })
    }

    pub fn play_source<S>(&self, source: S)
    where
        S: rodio::Source<Item = f32> + Send + 'static,
    {
        // Smoothly clear current queue without tearing WASAPI buffer stream
        self.sink.stop();
        self.sink.append(source);
        self.sink.play();
    }

    pub fn is_empty(&self) -> bool {
        self.sink.empty()
    }

    pub fn stop(&self) {
        self.sink.stop();
    }
}