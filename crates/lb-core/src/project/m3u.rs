//! M3U playlist import / export.

use crate::media::Library;
use crate::playlist::{Playlist, PlaylistEntry};

/// Parsed M3U entry.
#[derive(Debug, Clone, PartialEq)]
pub struct M3uEntry {
    pub path: String,
    pub title: Option<String>,
    pub duration_s: Option<f64>,
}

pub fn parse(text: &str) -> Vec<M3uEntry> {
    let mut out = Vec::new();
    let mut pending: Option<(Option<f64>, Option<String>)> = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            let (dur, title) = info.split_once(',').unwrap_or((info, ""));
            let d = dur.trim().parse::<f64>().ok().filter(|d| *d >= 0.0);
            let t = Some(title.trim().to_string()).filter(|t| !t.is_empty());
            pending = Some((d, t));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let (duration_s, title) = pending.take().unwrap_or((None, None));
        out.push(M3uEntry { path: line.to_string(), title, duration_s });
    }
    out
}

/// Render a playlist as extended M3U using library paths.
pub fn export(pl: &Playlist, lib: &Library) -> String {
    let mut s = String::from("#EXTM3U\n");
    for e in &pl.entries {
        if let Some(m) = lib.get(&e.media) {
            s.push_str(&format!("#EXTINF:{},{}\n{}\n", m.play_duration() / 1000, m.title, m.path));
        }
    }
    s
}

/// Build a playlist from M3U entries, matching library items by path.
/// Returns the playlist and the paths not found in the library.
pub fn to_playlist(id: &str, entries: &[M3uEntry], lib: &Library) -> (Playlist, Vec<String>) {
    let mut pl = Playlist::new(id);
    let mut missing = Vec::new();
    for e in entries {
        let norm = e.path.replace('\\', "/");
        match lib.items.iter().find(|m| m.path.replace('\\', "/") == norm) {
            Some(m) => pl.entries.push(PlaylistEntry::new(m.id.clone())),
            None => missing.push(e.path.clone()),
        }
    }
    (pl, missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extinf() {
        let e = parse("#EXTM3U\n#EXTINF:120,News\nmedia/news.mp4\n\nmedia/b.mp4\n");
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].title.as_deref(), Some("News"));
        assert_eq!(e[0].duration_s, Some(120.0));
        assert_eq!(e[1].title, None);
    }
}
