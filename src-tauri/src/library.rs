//! The user's music library: imported audio files are copied into the app's own storage
//! and stay there until the user deletes them (Android has no other way to keep access to
//! a picked file; on desktop the same library is used for files picked with the button).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static DIR: OnceLock<PathBuf> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

pub const MAX_IMPORT_BYTES: usize = 500 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub path: String,
    pub added: i64,
    /// true for files saved by "Download Processed Audio".
    #[serde(default)]
    pub processed: bool,
}

pub fn init(app_data: PathBuf) {
    let d = app_data.join("library");
    let _ = std::fs::create_dir_all(&d);
    let _ = DIR.set(d);
}

pub fn dir() -> Result<PathBuf, String> {
    DIR.get().cloned().ok_or_else(|| "Library not initialised".to_string())
}

fn index_path() -> Result<PathBuf, String> {
    Ok(dir()?.join("library.json"))
}

pub fn list() -> Vec<Track> {
    let Ok(p) = index_path() else { return vec![] };
    let mut v: Vec<Track> = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    v.retain(|t| std::path::Path::new(&t.path).exists());
    v
}

fn save(v: &[Track]) -> Result<(), String> {
    let p = index_path()?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, p).map_err(|e| e.to_string())
}

fn sanitize(name: &str) -> String {
    let base = std::path::Path::new(name).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "track".into());
    let clean: String = base.chars().map(|c| if c.is_alphanumeric() || " ._-()".contains(c) { c } else { '_' }).collect();
    if clean.trim().is_empty() { "track".into() } else { clean.chars().take(120).collect() }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Store bytes as a new library track.
pub fn add_bytes(name: &str, bytes: &[u8], processed: bool) -> Result<Track, String> {
    if bytes.is_empty() || bytes.len() > MAX_IMPORT_BYTES {
        return Err("File is empty or larger than 500 MB.".into());
    }
    let _g = LOCK.lock().unwrap();
    let mut v = list();
    let id = format!("{:x}{:04x}", now(), v.len() as u32 & 0xffff);
    let file = format!("{}_{}", id, sanitize(name));
    let path = dir()?.join(&file);
    std::fs::write(&path, bytes).map_err(|e| format!("Cannot save {}: {}", name, e))?;
    let t = Track { id, name: sanitize(name), path: path.to_string_lossy().to_string(), added: now(), processed };
    v.push(t.clone());
    save(&v)?;
    Ok(t)
}

/// Delete a track and its file.
pub fn remove(id: &str) -> Result<bool, String> {
    let _g = LOCK.lock().unwrap();
    let mut v = list();
    let Some(pos) = v.iter().position(|t| t.id == id) else { return Ok(false) };
    let t = v.remove(pos);
    let _ = std::fs::remove_file(&t.path);
    save(&v)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn add_list_remove() {
        let d = std::env::temp_dir().join(format!("diviora_lib_test_{}", std::process::id()));
        init(d.clone());
        let t = add_bytes("../evil/na:me?.mp3", b"abc", false).unwrap();
        assert!(t.name == "na_me_.mp3" && std::path::Path::new(&t.path).exists());
        assert_eq!(list().len(), 1);
        assert!(remove(&t.id).unwrap());
        assert!(list().is_empty() && !std::path::Path::new(&t.path).exists());
        let _ = std::fs::remove_dir_all(d);
    }
}
