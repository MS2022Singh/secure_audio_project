use std::path::PathBuf;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrackSource {
    LocalFile(PathBuf),
    RemoteUrl(String),
    DroppedPath(PathBuf),
    Folder(PathBuf),
    BufferedStream(Vec<u8>),
}