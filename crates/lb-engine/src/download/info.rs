//! What a link points to (`yt-dlp -J`): title, duration, author and the
//! qualities that can be downloaded, plus the yt-dlp arguments for a
//! chosen quality. Pure functions (no IO), unit-tested.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A downloadable quality offered to the operator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quality {
    /// `best`, `h1080`, `audio` … (passed back to start the download).
    pub id: String,
    /// `1080p · 60 кадр/с · ~250 МБ`.
    pub label: String,
    #[serde(default)]
    pub height: u32,
}

/// One video of a link (a playlist lists several).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub duration_ms: i64,
}

/// Summary of a link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkInfo {
    pub title: String,
    /// Site name as yt-dlp knows it (`youtube`, `TikTok`, `vk` …).
    pub site: String,
    pub uploader: String,
    pub duration_ms: i64,
    /// Preview picture (a URL on the site).
    pub thumbnail: String,
    pub is_live: bool,
    /// Videos to download: one for a single video, all of a playlist.
    pub entries: Vec<Entry>,
    pub qualities: Vec<Quality>,
}

/// Heights offered when the formats are unknown (playlists).
const STANDARD_HEIGHTS: &[u32] = &[2160, 1440, 1080, 720, 480, 360];

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

fn ms(v: &Value) -> i64 {
    v["duration"].as_f64().map(|d| (d * 1000.0).round() as i64).unwrap_or(0)
}

/// Prefer a JPEG / PNG preview (the interface cannot decode WebP).
fn thumbnail(v: &Value) -> String {
    let list = v["thumbnails"].as_array().cloned().unwrap_or_default();
    let decodable = |u: &str| {
        let path = u.split(['?', '#']).next().unwrap_or(u).to_ascii_lowercase();
        path.ends_with(".jpg") || path.ends_with(".jpeg") || path.ends_with(".png")
    };
    // the list is ordered from worst to best; take a mid-size one
    let jpgs: Vec<&Value> = list.iter().filter(|t| t["url"].as_str().is_some_and(decodable)).collect();
    let pick = jpgs.iter().rev().find(|t| t["width"].as_u64().is_none_or(|w| w <= 720)).or(jpgs.last());
    match pick {
        Some(t) => s(t, "url"),
        None => s(v, "thumbnail"),
    }
}

fn size_mb(bytes: f64) -> String {
    let mb = bytes / (1024.0 * 1024.0);
    if mb >= 1024.0 { format!("~{:.1} ГБ", mb / 1024.0) } else { format!("~{:.0} МБ", mb.max(1.0)) }
}

fn fmt_size(f: &Value) -> Option<f64> {
    f["filesize"].as_f64().or_else(|| f["filesize_approx"].as_f64()).filter(|b| *b > 0.0)
}

fn has(f: &Value, k: &str) -> bool {
    f[k].as_str().is_some_and(|c| c != "none")
}

/// Qualities from the formats of one video: every distinct height (best
/// first), "best" and "audio only".
pub fn qualities(formats: &[Value]) -> Vec<Quality> {
    let video: Vec<&Value> = formats.iter().filter(|f| has(f, "vcodec") && f["height"].as_u64().is_some()).collect();
    let audio_size = formats
        .iter()
        .filter(|f| has(f, "acodec") && !has(f, "vcodec"))
        .filter_map(|f| fmt_size(f).map(|b| (f["abr"].as_f64().unwrap_or(0.0), b)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|x| x.1);
    let mut heights: Vec<u32> = video.iter().filter_map(|f| f["height"].as_u64().map(|h| h as u32)).filter(|h| *h >= 144).collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    let mut out =
        vec![Quality { id: "best".into(), label: "Лучшее доступное".into(), height: heights.first().copied().unwrap_or(0) }];
    for h in heights {
        let at: Vec<&&Value> = video.iter().filter(|f| f["height"].as_u64() == Some(h as u64)).collect();
        let fps = at.iter().filter_map(|f| f["fps"].as_f64()).fold(0.0, f64::max);
        let size = at.iter().filter_map(|f| fmt_size(f)).fold(0.0, f64::max);
        let mut label = format!("{h}p");
        if fps > 31.0 {
            label.push_str(&format!(" · {fps:.0} кадр/с"));
        }
        if size > 0.0 {
            let with_audio = at.iter().any(|f| has(f, "acodec"));
            label.push_str(&format!(" · {}", size_mb(size + if with_audio { 0.0 } else { audio_size.unwrap_or(0.0) })));
        }
        out.push(Quality { id: format!("h{h}"), label, height: h });
    }
    if formats.iter().any(|f| has(f, "acodec")) {
        let label = match audio_size {
            Some(b) => format!("Только звук · {}", size_mb(b)),
            None => "Только звук".into(),
        };
        out.push(Quality { id: "audio".into(), label, height: 0 });
    }
    out
}

/// Qualities offered when the formats are not known in advance.
pub fn standard_qualities() -> Vec<Quality> {
    let mut out = vec![Quality { id: "best".into(), label: "Лучшее доступное".into(), height: 0 }];
    out.extend(STANDARD_HEIGHTS.iter().map(|h| Quality { id: format!("h{h}"), label: format!("до {h}p"), height: *h }));
    out.push(Quality { id: "audio".into(), label: "Только звук".into(), height: 0 });
    out
}

/// Summary of `yt-dlp -J --flat-playlist` output.
pub fn parse(v: &Value, url: &str) -> Result<LinkInfo, String> {
    let is_playlist = v["_type"].as_str() == Some("playlist") || v["entries"].is_array();
    let site =
        [s(v, "extractor_key"), s(v, "extractor"), s(v, "webpage_url_domain")].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
    let uploader = [s(v, "uploader"), s(v, "channel"), s(v, "creator")].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
    if is_playlist {
        let entries: Vec<Entry> = v["entries"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let u = [s(e, "webpage_url"), s(e, "url")].into_iter().find(|x| !x.is_empty())?;
                        Some(Entry { url: u, title: s(e, "title"), duration_ms: ms(e) })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if entries.is_empty() {
            return Err("the playlist has no videos".into());
        }
        return Ok(LinkInfo {
            title: s(v, "title"),
            site,
            uploader,
            duration_ms: entries.iter().map(|e| e.duration_ms).sum(),
            thumbnail: thumbnail(v),
            is_live: false,
            entries,
            qualities: standard_qualities(),
        });
    }
    let formats = v["formats"].as_array().cloned().unwrap_or_default();
    let qualities = if formats.is_empty() { standard_qualities() } else { qualities(&formats) };
    let page = [s(v, "webpage_url"), s(v, "original_url"), url.to_string()].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
    let title = s(v, "title");
    Ok(LinkInfo {
        title: title.clone(),
        site,
        uploader,
        duration_ms: ms(v),
        thumbnail: thumbnail(v),
        is_live: v["is_live"].as_bool().unwrap_or(false) || v["live_status"].as_str() == Some("is_live"),
        entries: vec![Entry { url: page, title, duration_ms: ms(v) }],
        qualities,
    })
}

/// yt-dlp arguments selecting `quality`. Video prefers H.264 / AAC (what
/// every decoder handles) at the same resolution, and is merged into MP4
/// when the streams fit, otherwise into MKV.
pub fn format_args(quality: &str) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if quality == "audio" {
        return v(&["-f", "ba/b/bv*", "-x", "--audio-format", "m4a"]);
    }
    let height = quality.strip_prefix('h').and_then(|h| h.parse::<u32>().ok());
    let sort = match height {
        Some(h) => format!("res:{h},fps,vcodec:h264,acodec:aac"),
        None => "res,fps,vcodec:h264,acodec:aac".into(),
    };
    // a video-only or single-stream result is better than "format not available"
    v(&["-f", "bv*+ba/b/bv*/ba", "-S", &sort, "--merge-output-format", "mp4/mkv"])
}

/// Arguments for the second attempt after "Requested format is not
/// available": no format selection (yt-dlp's own default) and the other
/// YouTube clients, which often expose formats the default one hides.
pub fn fallback_args(quality: &str) -> Vec<String> {
    let mut a = vec!["--extractor-args".to_string(), "youtube:player_client=default,tv,web_safari,mweb".to_string()];
    if quality == "audio" {
        a.extend(["-x", "--audio-format", "m4a"].map(String::from));
    } else {
        a.extend(["--merge-output-format", "mp4/mkv"].map(String::from));
    }
    a
}

/// Whether yt-dlp failed because no format matched.
pub fn is_format_error(message: &str) -> bool {
    message.contains("Requested format is not available")
}

/// A valid quality id (the rest of the request goes to yt-dlp as is).
pub fn is_quality(id: &str) -> bool {
    id == "best" || id == "audio" || id.strip_prefix('h').and_then(|h| h.parse::<u32>().ok()).is_some_and(|h| (100..=8640).contains(&h))
}

/// Links yt-dlp may open: http(s) only (no local files or other schemes).
pub fn is_web_link(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    (l.starts_with("https://") || l.starts_with("http://")) && !l.contains(char::is_whitespace) && l.len() > 10
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn video() -> Value {
        json!({
            "title": "Клип", "extractor_key": "Youtube", "uploader": "Канал", "duration": 212.4,
            "webpage_url": "https://www.youtube.com/watch?v=abc",
            "thumbnails": [
                {"url": "https://i.ytimg.com/vi/abc/default.jpg", "width": 120},
                {"url": "https://i.ytimg.com/vi/abc/hqdefault.jpg", "width": 480},
                {"url": "https://i.ytimg.com/vi_webp/abc/maxres.webp", "width": 1280}
            ],
            "formats": [
                {"format_id": "140", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 128.0, "filesize": 3_400_000},
                {"format_id": "18", "vcodec": "avc1", "acodec": "mp4a", "height": 360, "fps": 30, "filesize": 10_000_000},
                {"format_id": "137", "vcodec": "avc1", "acodec": "none", "height": 1080, "fps": 30, "filesize": 80_000_000},
                {"format_id": "299", "vcodec": "avc1", "acodec": "none", "height": 1080, "fps": 60, "filesize": 120_000_000},
                {"format_id": "sb0", "vcodec": "none", "acodec": "none", "format_note": "storyboard"}
            ]
        })
    }

    #[test]
    fn single_video() {
        let i = parse(&video(), "https://youtu.be/abc").unwrap();
        assert_eq!(i.title, "Клип");
        assert_eq!(i.site, "Youtube");
        assert_eq!(i.duration_ms, 212_400);
        assert_eq!(i.thumbnail, "https://i.ytimg.com/vi/abc/hqdefault.jpg");
        assert_eq!(i.entries.len(), 1);
        assert_eq!(i.entries[0].url, "https://www.youtube.com/watch?v=abc");
        let ids: Vec<&str> = i.qualities.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, ["best", "h1080", "h360", "audio"]);
        assert!(i.qualities[1].label.starts_with("1080p · 60 кадр/с · ~"), "{}", i.qualities[1].label);
        assert_eq!(i.qualities[2].label, "360p · ~10 МБ");
    }

    #[test]
    fn playlist_entries() {
        let v = json!({"_type": "playlist", "title": "Сборник", "extractor": "youtube:tab", "entries": [
            {"url": "https://www.youtube.com/watch?v=a", "title": "A", "duration": 10},
            {"url": "https://www.youtube.com/watch?v=b", "title": "B", "duration": 20},
            {"title": "без ссылки"}
        ]});
        let i = parse(&v, "https://www.youtube.com/playlist?list=x").unwrap();
        assert_eq!(i.entries.len(), 2);
        assert_eq!(i.duration_ms, 30_000);
        assert_eq!(i.qualities, standard_qualities());
        assert!(parse(&json!({"_type": "playlist", "entries": []}), "").is_err());
    }

    #[test]
    fn quality_arguments() {
        assert_eq!(format_args("audio")[..2], ["-f", "ba/b/bv*"]);
        assert!(is_format_error("ERROR: Requested format is not available. Use --list-formats"));
        let a = format_args("h720");
        assert!(a.contains(&"res:720,fps,vcodec:h264,acodec:aac".to_string()));
        assert!(is_quality("h1080") && is_quality("best") && is_quality("audio"));
        assert!(!is_quality("h") && !is_quality("--exec") && !is_quality("h99999"));
    }

    #[test]
    fn only_web_links() {
        assert!(is_web_link("https://www.tiktok.com/@a/video/1"));
        assert!(!is_web_link("file:///etc/passwd"));
        assert!(!is_web_link("--exec=calc"));
        assert!(!is_web_link("https://a b"));
    }
}
