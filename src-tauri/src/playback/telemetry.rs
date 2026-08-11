use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlaybackState {
    Loaded,
    Buffering,
    Playing { track_title: String },
    Paused,
    Ended,
    Error { message: String },
    QueueChanged { size: usize },
    CurrentTrackChanged { name: String },
    DropReceived,
}