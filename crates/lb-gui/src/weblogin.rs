//! Signing in with the system WebView and a private
//! LauraBoard profile: the user signs in as
//! usual, closes the window, and the cookies of that profile become the
//! `cookies.txt` yt-dlp downloads with. The user's own browser and its
//! profile are not touched.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
#[cfg(test)]
use serde_json::json;

/// Where the sign-in starts (YouTube, after Google's sign-in page).
pub const YOUTUBE: &str = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fwww.youtube.com%2F";

/// The profile kept between sign-ins (the next window is already signed in).
fn profile() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("lauraboard").join("web-login-runtime")
}

/// Open the window and wait until it is closed or `done` is set; returns
/// the cookies in the Netscape `cookies.txt` format and whether YouTube
/// sees a signed-in user.
#[cfg(not(target_os = "android"))]
pub fn run(start: &str, done: Arc<AtomicBool>) -> Result<(String, bool), String> {
    let exe = lb_core::browser::path().ok_or(lb_core::browser::MISSING)?;
    let dir = profile();
    let mut command = Command::new(exe);
    command.arg("login").arg(start).arg(&dir).arg("-").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|e| format!("WebView: {e}"))?;
    let output = child.stdout.take().ok_or("WebView output pipe unavailable")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(output).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let _ = tx.send(v);
            }
        }
    });
    let mut last = Vec::new();
    let mut error = None;
    let mut closing = None;
    loop {
        while let Ok(v) = rx.try_recv() {
            if let Some(cookies) = v["cookies"].as_array() {
                last = cookies.clone();
            }
            if let Some(message) = v["error"].as_str() {
                error = Some(message.to_string());
            }
        }
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if done.load(Ordering::Relaxed) && closing.is_none() {
            use std::io::Write;
            if let Some(input) = child.stdin.as_mut() {
                let _ = input.write_all(b"done\n");
            }
            closing = Some(Instant::now());
        }
        if closing.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.wait();
    // Drain the final cookie snapshot emitted while the window closes.
    while let Ok(v) = rx.recv_timeout(Duration::from_millis(100)) {
        if let Some(cookies) = v["cookies"].as_array() {
            last = cookies.clone();
        }
        if let Some(message) = v["error"].as_str() {
            error = Some(message.to_string());
        }
    }
    if last.is_empty() {
        return Err(error.unwrap_or_else(|| "окно закрыто до входа — cookies нет".into()));
    }
    let signed_in = last.iter().any(|c| {
        c["domain"].as_str().is_some_and(|d| d == "youtube.com" || d.ends_with(".youtube.com"))
            && matches!(c["name"].as_str(), Some("LOGIN_INFO" | "SAPISID" | "__Secure-3PAPISID"))
    });
    Ok((netscape(&last), signed_in))
}

/// WebView cookies → `cookies.txt` (what yt-dlp reads with `--cookies`).
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

#[cfg(target_os = "android")]
pub fn run(_: &str, _: Arc<AtomicBool>) -> Result<(String, bool), String> {
    Err("Вход через настольный WebView недоступен на Android. Используйте настольный клиент.".into())
}