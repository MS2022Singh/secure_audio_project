use std::path::PathBuf;
use walkdir::WalkDir;

pub struct FileSystemScanner;
impl FileSystemScanner {
    pub fn flatten_directory(path: &PathBuf) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if path.is_file() {
            files.push(path.clone());
        } else if path.is_dir() {
            for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                if entry.path().is_file() {
                    if let Some(ext) = entry.path().extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        if ["mp3", "wav", "flac", "ogg", "m4a", "aac"].contains(&ext_str.as_str()) {
                            files.push(entry.path().to_path_buf());
                        }
                    }
                }
            }
        }
        files
    }
}
