//! Portable path handling: media paths are stored relative to the project
//! root with forward slashes, so projects move between Windows and Linux.

use std::path::{Path, PathBuf};

use crate::media::is_url;

/// Resolve a stored media path against the project root.
pub fn resolve(root: &Path, stored: &str) -> PathBuf {
    if is_url(stored) {
        return PathBuf::from(stored);
    }
    let p = PathBuf::from(stored.replace('\\', "/"));
    if p.is_absolute() || is_windows_absolute(stored) {
        return p;
    }
    let mut parts = p.components();
    if parts.next().is_some_and(|c| c.as_os_str() == super::CHANNELS_DIR)
        && let Some(std::path::Component::Normal(id)) = parts.next()
    {
        let entry = root.join(super::CHANNELS_DIR).join(id);
        let location = super::channel_location(&entry);
        if location != entry {
            return location.join(parts.as_path());
        }
    }
    root.join(p)
}

/// Resolve to a string suitable for ffmpeg (URLs untouched).
pub fn resolve_str(root: &Path, stored: &str) -> String {
    if is_url(stored) { stored.to_string() } else { resolve(root, stored).to_string_lossy().into_owned() }
}

/// Convert a path to the stored form: relative to `root` when inside it.
pub fn to_stored(root: &Path, path: &Path) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let root_abs = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if let Ok(entries) = std::fs::read_dir(root.join(super::CHANNELS_DIR)) {
        for entry in entries.flatten() {
            if !entry.path().join(super::LOCATION_FILE).is_file() {
                continue;
            }
            let location = super::channel_location(&entry.path());
            let location = std::fs::canonicalize(&location).unwrap_or(location);
            if let Ok(relative) = abs.strip_prefix(&location) {
                return format!("channels/{}/{}", entry.file_name().to_string_lossy(), relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let rel = abs.strip_prefix(&root_abs).map(Path::to_path_buf).unwrap_or(abs);
    let s = rel.to_string_lossy().replace('\\', "/");
    s.strip_prefix("//?/").map(str::to_string).unwrap_or(s)
}

fn is_windows_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_and_keeps_urls() {
        let root = Path::new("/srv/tv");
        assert_eq!(resolve(root, "media/a.mp4"), Path::new("/srv/tv/media/a.mp4"));
        assert_eq!(resolve(root, "media\\a.mp4"), Path::new("/srv/tv/media/a.mp4"));
        assert_eq!(resolve_str(root, "rtmp://x/y"), "rtmp://x/y");
        assert!(is_windows_absolute("C:\\media\\a.mp4"));
        assert_eq!(resolve(root, "C:/m/a.mp4"), PathBuf::from("C:/m/a.mp4"));
    }
}
