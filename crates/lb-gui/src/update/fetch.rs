//! Network side of updates: ask GitHub for the latest release, download a
//! file and check it against the release's SHA256SUMS. Blocking: called
//! from a background thread.

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::release::{self, Asset, Release};

fn agent(body_timeout: Duration) -> ureq::Agent {
    let cfg = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(body_timeout))
        .user_agent(concat!("LauraBoard/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(cfg)
}

type Response = ureq::http::Response<ureq::Body>;

/// `None` for 404.
fn fetch(url: &str, timeout: Duration) -> Result<Option<Response>, String> {
    let res = agent(timeout).get(url).header("Accept", "application/vnd.github+json").call().map_err(|e| net_error(&e))?;
    match res.status().as_u16() {
        200 => Ok(Some(res)),
        404 => Ok(None),
        403 | 429 => Err("GitHub временно ограничил запросы — попробуйте позже".into()),
        s => Err(format!("GitHub ответил {s}")),
    }
}

fn get(url: &str, timeout: Duration) -> Result<Response, String> {
    fetch(url, timeout)?.ok_or_else(|| "файл выпуска не найден".into())
}

fn net_error(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "GitHub не ответил вовремя".into(),
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Io(_) => "нет связи с GitHub".into(),
        e => format!("ошибка сети: {e}"),
    }
}

/// The latest published release (`None`: there are none yet).
pub fn latest() -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", release::REPO);
    let Some(mut res) = fetch(&url, Duration::from_secs(30))? else { return Ok(None) };
    let text = res.body_mut().read_to_string().map_err(|e| net_error(&e))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("непонятный ответ GitHub: {e}"))?;
    Ok(release::parse(&v))
}

/// Download `asset` to `dest`, reporting progress (0 … 1); the file only
/// appears at `dest` once its checksum matched the release's SHA256SUMS.
pub fn download(r: &Release, asset: &Asset, dest: &Path, mut progress: impl FnMut(f32)) -> Result<(), String> {
    let sums = release::sums(r).ok_or("в выпуске нет контрольных сумм (SHA256SUMS)")?;
    let listing = get(&sums.url, Duration::from_secs(30))?.body_mut().read_to_string().map_err(|e| net_error(&e))?;
    let expected = release::checksum(&listing, &asset.name).ok_or("файла нет в контрольных суммах выпуска")?;

    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("папка {}: {e}", dir.display()))?;
    }
    let part = dest.with_extension("part");
    let result = (|| {
        // the whole body: generous, the file may be large and the link slow
        let mut res = get(&asset.url, Duration::from_secs(3 * 60 * 60))?;
        let total = res.body().content_length().unwrap_or(asset.size).max(1);
        let mut body = res.body_mut().with_config().limit(u64::MAX).reader();
        let mut file = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let mut hash = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        loop {
            let n = body.read(&mut buf).map_err(|e| format!("загрузка прервалась: {e}"))?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(|e| format!("запись {}: {e}", part.display()))?;
            done += n as u64;
            progress((done as f32 / total as f32).min(1.0));
        }
        file.sync_all().map_err(|e| format!("запись {}: {e}", part.display()))?;
        let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != expected {
            return Err("файл повреждён при загрузке (контрольная сумма не совпала)".into());
        }
        drop(file);
        let _ = std::fs::remove_file(dest);
        std::fs::rename(&part, dest).map_err(|e| format!("{}: {e}", dest.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}
