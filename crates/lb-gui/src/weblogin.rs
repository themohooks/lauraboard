//! Signing in to a site in a browser window of our own (Edge / Chrome /
//! Chromium with a separate LauraBoard profile): the user signs in as
//! usual, closes the window, and the cookies of that profile become the
//! `cookies.txt` yt-dlp downloads with. The user's own browser and its
//! profile are not touched.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Where the sign-in starts (YouTube, after Google's sign-in page).
pub const YOUTUBE: &str = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fwww.youtube.com%2F";

/// The profile kept between sign-ins (the next window is already signed in).
fn profile() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("lauraboard").join("web-login")
}

/// Open the window and wait until it is closed or `done` is set; returns
/// the cookies in the Netscape `cookies.txt` format and whether YouTube
/// sees a signed-in user.
pub fn run(start: &str, done: Arc<AtomicBool>) -> Result<(String, bool), String> {
    let exe = lb_core::browser::path().ok_or("не найден браузер Edge, Chrome или Chromium")?;
    let dir = profile();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let _ = std::fs::remove_file(dir.join("DevToolsActivePort"));
    let mut child = Command::new(exe)
        .arg(format!("--user-data-dir={}", dir.display()))
        .args(["--no-first-run", "--no-default-browser-check", "--disable-background-mode", "--remote-debugging-port=0"])
        .arg(format!("--app={start}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("браузер не запустился: {e}"))?;
    let result = watch(&mut child, &dir, &done);
    // "Готово" with the window still open: close it ourselves
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let cookies = result?;
    if cookies.is_empty() {
        return Err("окно закрыто до входа — cookies нет".into());
    }
    let signed_in = cookies.iter().any(|c| {
        c["domain"].as_str().is_some_and(|d| d.ends_with("youtube.com"))
            && matches!(c["name"].as_str(), Some("LOGIN_INFO" | "SAPISID" | "__Secure-3PAPISID"))
    });
    Ok((netscape(&cookies), signed_in))
}

/// Snapshot the cookies every two seconds until the browser exits (the
/// last snapshot wins: a closed browser can no longer be asked).
fn watch(child: &mut Child, dir: &std::path::Path, done: &AtomicBool) -> Result<Vec<Value>, String> {
    let started = Instant::now();
    let url = loop {
        if let Ok(text) = std::fs::read_to_string(dir.join("DevToolsActivePort")) {
            let mut lines = text.lines();
            if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                break format!("ws://127.0.0.1:{}{path}", port.trim());
            }
        }
        if child.try_wait().ok().flatten().is_some() {
            return Err("браузер закрылся сразу — возможно, окно входа уже открыто".into());
        }
        if started.elapsed() > Duration::from_secs(20) || done.load(Ordering::Relaxed) {
            return Err("браузер не ответил".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let (mut socket, _) = tungstenite::connect(url.as_str()).map_err(|e| format!("браузер: {e}"))?;
    let mut id = 0u64;
    let mut last = Vec::new();
    loop {
        id += 1;
        match call(&mut socket, id, "Storage.getCookies") {
            Ok(v) => last = v["cookies"].as_array().cloned().unwrap_or_default(),
            Err(_) => return Ok(last), // the browser is gone
        }
        if done.load(Ordering::Relaxed) {
            return Ok(last);
        }
        for _ in 0..20 {
            if done.load(Ordering::Relaxed) || child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if child.try_wait().ok().flatten().is_some() {
            return Ok(last);
        }
    }
}

type Socket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

fn call(socket: &mut Socket, id: u64, method: &str) -> Result<Value, String> {
    socket.send(tungstenite::Message::Text(json!({ "id": id, "method": method }).to_string().into())).map_err(|e| e.to_string())?;
    loop {
        if let tungstenite::Message::Text(text) = socket.read().map_err(|e| e.to_string())? {
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if v["id"] == id {
                return if v["error"].is_null() { Ok(v["result"].clone()) } else { Err(v["error"].to_string()) };
            }
        }
    }
}

/// DevTools cookies → `cookies.txt` (what yt-dlp reads with `--cookies`).
fn netscape(cookies: &[Value]) -> String {
    let mut out = String::from("# Netscape HTTP Cookie File\n# Signed in with LauraBoard\n\n");
    for c in cookies {
        let (Some(domain), Some(name)) = (c["domain"].as_str(), c["name"].as_str()) else { continue };
        let value = c["value"].as_str().unwrap_or_default();
        if domain.contains(['\t', '\n']) || name.contains(['\t', '\n']) || value.contains(['\t', '\n']) {
            continue;
        }
        let flag = |b: bool| if b { "TRUE" } else { "FALSE" };
        let http_only = if c["httpOnly"].as_bool() == Some(true) { "#HttpOnly_" } else { "" };
        // session cookies have expires -1; 0 means "session" in the file
        let expires = c["expires"].as_f64().filter(|e| *e > 0.0).map_or(0, |e| e as i64);
        out.push_str(&format!(
            "{http_only}{domain}\t{}\t{}\t{}\t{expires}\t{name}\t{value}\n",
            flag(domain.starts_with('.')),
            c["path"].as_str().unwrap_or("/"),
            flag(c["secure"].as_bool() == Some(true)),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_netscape_cookies() {
        let c = [
            json!({"domain": ".youtube.com", "name": "LOGIN_INFO", "value": "x", "path": "/", "secure": true, "httpOnly": true, "expires": 1.9e9}),
            json!({"domain": "www.youtube.com", "name": "PREF", "value": "f=1", "path": "/", "secure": false, "expires": -1}),
            json!({"domain": ".bad.com", "name": "a\tb", "value": "v"}),
        ];
        let text = netscape(&c);
        assert!(text.starts_with("# Netscape HTTP Cookie File\n"));
        assert!(text.contains("#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1900000000\tLOGIN_INFO\tx\n"));
        assert!(text.contains("www.youtube.com\tFALSE\t/\tFALSE\t0\tPREF\tf=1\n"));
        assert!(!text.contains("bad.com"));
    }

    /// Opens a real browser window for a few seconds:
    /// `cargo test -p lb-gui weblogin -- --ignored`
    #[test]
    #[ignore]
    fn reads_cookies_from_a_real_window() {
        let done = Arc::new(AtomicBool::new(false));
        let d = done.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(8));
            d.store(true, Ordering::Relaxed);
        });
        let (text, signed_in) = run("https://www.youtube.com/", done).unwrap();
        println!("{} cookie lines, signed in: {signed_in}", text.lines().filter(|l| !l.starts_with("# ") && !l.is_empty()).count());
        assert!(text.contains("youtube.com"));
    }
}
