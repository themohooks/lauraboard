//! Running yt-dlp: reading what a link offers, and the download job that
//! saves videos into the channel's storage and adds them to the library.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

use lb_core::project::paths;

use super::info::{self, Entry, LinkInfo};
use super::tool;
use crate::channel::ChannelHandle;
use crate::jobs::JobCtl;
use crate::library::storage::{MEDIA_DIR, remove_own_file};
use crate::library::{ImportOptions, new_item, probe_item};

const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
/// Prefixes of the lines we ask yt-dlp to print.
const PROGRESS: &str = "LBPROG";
const POST: &str = "LBPOST";
const DONE: &str = "LBDONE";

/// Programs the job needs.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ytdlp: PathBuf,
    /// ffmpeg for merging video and audio (a bare name: found in PATH).
    pub ffmpeg: PathBuf,
    pub ffprobe: String,
    /// Sign-in arguments ([`super::login::args`]).
    pub login: Vec<OsString>,
}

/// Options common to every call.
fn base_args(cmd: &mut tokio::process::Command) {
    cmd.args(["--ignore-config", "--no-colors", "--no-warnings", "--no-playlist"]);
}

fn last_error(stderr: &str) -> String {
    let line = stderr.lines().rev().find(|l| l.contains("ERROR")).or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()));
    line.map(|l| l.trim().trim_start_matches("ERROR:").trim().to_string()).unwrap_or_else(|| "yt-dlp failed".into())
}

/// What `url` offers (one video or a playlist).
pub async fn probe(ytdlp: &Path, login: &[OsString], url: &str) -> Result<LinkInfo, String> {
    if !info::is_web_link(url) {
        return Err("not a web link (http:// or https://)".into());
    }
    let mut cmd = tool::command(ytdlp);
    base_args(&mut cmd);
    cmd.args(login);
    cmd.args(["-J", "--flat-playlist", "--", url.trim()]).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = tokio::time::timeout(PROBE_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "the site did not answer in time".to_string())?
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(last_error(&String::from_utf8_lossy(&out.stderr)));
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("yt-dlp: {e}"))?;
    info::parse(&v, url)
}

/// What to download.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub entries: Vec<Entry>,
    pub quality: String,
}

/// Job body: download each entry, probe it and add it to the library at
/// once (a cancelled playlist keeps the videos already finished).
pub async fn run(h: ChannelHandle, tools: Tools, req: Request, opt: ImportOptions, ctl: JobCtl) -> Result<Value, String> {
    let root = h.shared.root.clone();
    let channel = h.id().to_string();
    let dir = h.read(|d| d.dir.join(MEDIA_DIR));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let n = req.entries.len().max(1);
    let (mut added, mut errors) = (Vec::new(), Vec::new());
    for (i, e) in req.entries.iter().enumerate() {
        ctl.check()?;
        let name = if e.title.is_empty() { e.url.clone() } else { e.title.clone() };
        let label = if n > 1 { format!("{}/{n}: {name}", i + 1) } else { name.clone() };
        let at = |f: f32| (i as f32 + f) / n as f32;
        ctl.progress(at(0.0), label.clone());
        let stem = uuid::Uuid::new_v4().simple().to_string();
        let fetched = fetch(&tools, &e.url, &req.quality, &dir, &stem, &ctl, |f, step| {
            ctl.progress(at(f * 0.95), if step.is_empty() { label.clone() } else { format!("{label} · {step}") })
        })
        .await;
        let done = match fetched {
            Ok(d) => d,
            Err(err) => {
                remove_partial(&dir, &stem);
                ctl.check()?;
                errors.push(format!("{name}: {err}"));
                continue;
            }
        };
        let stored = paths::to_stored(&root, &done.path);
        let title = if done.title.is_empty() { name.clone() } else { done.title.clone() };
        let mut item = h.read(|d| new_item(&d.library, &stored, &title, false, false, &opt));
        item.title = title.clone();
        item.original_name = Some(format!("{}.{}", sanitize(&title), done.ext));
        if !done.page.is_empty() {
            item.meta.insert("source_url".into(), done.page.clone());
        }
        if !done.uploader.is_empty() {
            item.meta.insert("author".into(), done.uploader.clone());
        }
        ctl.progress(at(0.97), format!("{label} · проверка файла"));
        if let Err(err) = probe_item(&mut item, &root, &tools.ffprobe).await {
            remove_own_file(&root, &channel, &stored);
            errors.push(format!("{name}: {err}"));
            continue;
        }
        let id = item.id.clone();
        h.modify(move |d| {
            if d.library.contains(&item.id) {
                item.id = d.library.unique_id(&item.id);
            }
            d.library.upsert(item);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;
        added.push(id);
    }
    if added.is_empty()
        && let [only] = errors.as_slice()
    {
        return Err(only.clone());
    }
    Ok(json!({ "added": added, "errors": errors }))
}

/// A finished download.
#[derive(Debug, Default, Deserialize)]
struct Done {
    #[serde(rename = "filepath")]
    path: PathBuf,
    #[serde(default, deserialize_with = "text")]
    title: String,
    #[serde(default, deserialize_with = "text")]
    ext: String,
    #[serde(default, rename = "webpage_url", deserialize_with = "text")]
    page: String,
    #[serde(default, deserialize_with = "text")]
    uploader: String,
}

/// yt-dlp prints `null` for fields a site does not have.
fn text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// Download one video as `<dir>/<stem>.<ext>`; `progress` gets the share
/// done (0…1) and the current step.
async fn fetch(
    tools: &Tools,
    url: &str,
    quality: &str,
    dir: &Path,
    stem: &str,
    ctl: &JobCtl,
    progress: impl Fn(f32, &str),
) -> Result<Done, String> {
    if !info::is_web_link(url) || !info::is_quality(quality) {
        return Err("bad request".into());
    }
    let mut cmd = tool::command(&tools.ytdlp);
    base_args(&mut cmd);
    cmd.args(&tools.login);
    cmd.args(info::format_args(quality));
    cmd.args(["--newline", "--progress", "--no-mtime", "--no-part"]);
    cmd.arg("--progress-template").arg(format!(
        "download:{PROGRESS} %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(info.format_id)s %(progress.speed)s"
    ));
    cmd.arg("--progress-template").arg(format!("postprocess:{POST} %(progress.postprocessor)s"));
    cmd.arg("--print").arg(format!("after_move:{DONE} %(.{{filepath,title,ext,webpage_url,uploader}})j"));
    if tools.ffmpeg.components().count() > 1 {
        cmd.arg("--ffmpeg-location").arg(&tools.ffmpeg);
    }
    cmd.arg("-o").arg(dir.join(format!("{stem}.%(ext)s")));
    cmd.args(["--", url.trim()]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("yt-dlp: {e}"))?;
    let stdout = child.stdout.take().ok_or("no output")?;
    let stderr = child.stderr.take().ok_or("no output")?;
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut tail = Vec::new();
        while let Ok(Some(l)) = lines.next_line().await {
            tail.push(l);
            if tail.len() > 30 {
                tail.remove(0);
            }
        }
        tail.join("\n")
    });
    // separate video and audio streams arrive one after the other
    let streams = if quality == "audio" { 1.0 } else { 2.0 };
    let mut phase = 0.0f32;
    let mut format = String::new();
    let mut done: Option<Done> = None;
    let mut lines = BufReader::new(stdout).lines();
    let mut tick = tokio::time::interval(Duration::from_millis(300));
    loop {
        tokio::select! {
            l = lines.next_line() => {
                let Ok(Some(l)) = l else { break };
                if let Some(rest) = l.strip_prefix(PROGRESS) {
                    let p: Vec<&str> = rest.split_whitespace().collect();
                    let num = |i: usize| p.get(i).and_then(|v| v.parse::<f64>().ok());
                    let got = num(0).unwrap_or(0.0);
                    let total = num(1).or(num(2)).unwrap_or(0.0);
                    let id = p.get(3).copied().unwrap_or_default();
                    if !format.is_empty() && id != format {
                        phase = (phase + 1.0).min(streams - 1.0);
                    }
                    format = id.to_string();
                    let f = if total > 0.0 { (got / total) as f32 } else { 0.0 };
                    let mb = got / (1024.0 * 1024.0);
                    ctl.speed(num(4));
                    progress(((phase + f.clamp(0.0, 1.0)) / streams) * 0.9, &format!("{mb:.0} МБ"));
                } else if l.starts_with(POST) {
                    ctl.speed(None);
                    progress(0.93, "сборка файла");
                } else if let Some(rest) = l.strip_prefix(DONE) {
                    done = serde_json::from_str(rest.trim()).ok();
                }
            }
            _ = tick.tick() => {
                if ctl.cancelled() {
                    let _ = child.kill().await;
                    return Err("cancelled".into());
                }
            }
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let stderr = err_task.await.unwrap_or_default();
    if !status.success() {
        return Err(last_error(&stderr));
    }
    let done = done.ok_or("yt-dlp did not report the file")?;
    // only files we asked for (the template keeps them in `dir`)
    if done.path.parent() != Some(dir) || !done.path.is_file() {
        return Err(format!("unexpected output file {}", done.path.display()));
    }
    Ok(done)
}

/// Leftovers of an interrupted download (`<stem>.*`).
fn remove_partial(dir: &Path, stem: &str) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().starts_with(stem) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// A title usable as a file name (shown as the original name).
fn sanitize(title: &str) -> String {
    let s: String = title.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    let s: String = s.chars().take(120).collect();
    if s.is_empty() { "video".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_and_names() {
        let e = "[youtube] abc: Downloading webpage\nERROR: [youtube] abc: Video unavailable\n";
        assert_eq!(last_error(e), "[youtube] abc: Video unavailable");
        assert_eq!(last_error(""), "yt-dlp failed");
        assert_eq!(sanitize("A/B: c?"), "A_B_ c_");
        assert_eq!(sanitize(" .. "), "video");
    }

    #[test]
    fn done_line_parses() {
        // a null field must not lose the whole line
        let d: Done =
            serde_json::from_str(r#"{"filepath":"/m/x.mp4","title":"T","ext":"mp4","webpage_url":"https://a","uploader":null}"#).unwrap();
        assert_eq!(d.path, PathBuf::from("/m/x.mp4"));
        assert_eq!((d.title.as_str(), d.uploader.as_str()), ("T", ""));
    }
}
