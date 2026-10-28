//! "Download from a site": yt-dlp state on the server (and installing
//! it), reading a link, choosing the quality and starting the download job.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, SharedString};

use super::{Ctx, folders, vec_model, with};
use crate::fmt::{KINDS, hms};
use crate::net::Msg;
use crate::shell::{AppWindow, Dl, Lib};

#[derive(Default)]
pub struct DownloadState {
    /// The last probe: entries and quality ids to send back.
    entries: Value,
    quality_ids: Vec<String>,
    /// Bumped per probe: answers to an older link are dropped.
    serial: u64,
    /// The sign-in window is open: set to finish it.
    web_done: Option<Arc<AtomicBool>>,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Dl>();
    g.on_opened(|| with(opened));
    g.on_probe(|| with(probe));
    g.on_start(|| with(start));
    g.on_install_tool(|| with(install_tool));
    g.on_login_cookies_pick(|| with(login_cookies_pick));
    g.on_login_cookies_clear(|| with(|c| save_login(c, Some(String::new()))));
    g.on_login_web(|| with(login_web));
    g.on_login_web_done(|| {
        with(|c| {
            if let Some(d) = &c.download.web_done {
                d.store(true, Ordering::Relaxed);
            }
        })
    });
}

/// Sign in in our native WebView window; its cookies go to the server.
fn login_web(c: &mut Ctx) {
    if c.download.web_done.is_some() {
        return;
    }
    let Some(ui) = c.window() else { return };
    ui.global::<Dl>().set_login_web_running(true);
    let done = Arc::new(AtomicBool::new(false));
    c.download.web_done = Some(done.clone());
    std::thread::spawn(move || {
        let r = crate::weblogin::run(crate::weblogin::YOUTUBE, done);
        let _ = slint::invoke_from_event_loop(move || with(move |c| login_web_ended(c, r)));
    });
}

fn login_web_ended(c: &mut Ctx, r: Result<(String, bool), String>) {
    c.download.web_done = None;
    if let Some(ui) = c.window() {
        ui.global::<Dl>().set_login_web_running(false);
    }
    match r {
        Ok((cookies, signed_in)) => {
            if !signed_in {
                c.toast("Вход в YouTube не найден — cookies всё равно сохранены (другие сайты)", true);
            }
            save_login(c, Some(cookies));
        }
        Err(e) => c.toast(format!("Окно входа: {e}"), true),
    }
}

/// The site wants a signed-in user (YouTube: "Sign in to confirm your
/// age"; yt-dlp then suggests `--cookies`).
pub fn needs_login(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    e.contains("[youtube]")
        && ["confirm your age", "age-restricted", "inappropriate for some users", "age restricted"].iter().any(|k| e.contains(k))
}

/// Open the dialog on the sign-in panel (after a failed download).
pub fn open_login(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Dl>();
    g.set_login_needed(true);
    g.set_step("error".into());
    g.set_open(true);
    refresh_login(c);
}

fn show_login(c: &mut Ctx, v: &Value) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Dl>();
    g.set_login_cookies(v["cookies"].as_bool().unwrap_or(false));
}

fn refresh_login(c: &mut Ctx) {
    c.worker.spawn(|api| {
        let r: Result<Value, String> = api.get("/tools/ytdlp/login");
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if let Ok(v) = r {
                show_login(c, &v);
            }
        })))
    });
}

fn login_cookies_pick(c: &mut Ctx) {
    let Some(path) = rfd::FileDialog::new().set_title("Файл cookies.txt").add_filter("cookies.txt", &["txt"]).pick_file() else {
        return;
    };
    match std::fs::read_to_string(&path) {
        Ok(text) if text.trim().is_empty() => c.toast("Файл cookies.txt пуст", true),
        Ok(text) => {
            save_login(c, Some(text));
        }
        Err(e) => c.toast(format!("{}: {e}", path.display()), true),
    }
}

/// Store the choice on the server; `cookies`: `Some("")` removes the file.
fn save_login(c: &mut Ctx, cookies: Option<String>) {
    let Some(ui) = c.window() else { return };
    ui.global::<Dl>().set_login_saving(true);
    let mut body = json!({ "browser": null });
    if let Some(text) = cookies {
        body["cookies"] = json!(text);
    }
    c.worker.spawn(move |api| {
        let r = api.put("/tools/ytdlp/login", &body);
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if let Some(ui) = c.window() {
                ui.global::<Dl>().set_login_saving(false);
            }
            match r {
                Ok(v) => {
                    show_login(c, &v);
                    if let Some(ui) = c.window()
                        && ui.global::<Dl>().get_step() == "error"
                        && v["cookies"].as_bool() == Some(true)
                    {
                        c.toast("Вход сохранён — пробуем ссылку ещё раз", false);
                        probe(c);
                    }
                }
                Err(e) => {
                    c.toast(format!("Вход в аккаунт: {}", crate::ru::server(&e)), true);
                    refresh_login(c);
                }
            }
        })))
    });
}

fn opened(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Dl>();
    // keep the folder chosen in the library
    g.set_folder(ui.global::<Lib>().get_import_folder());
    if g.get_step() == "error" {
        g.set_step(SharedString::new());
    }
    refresh_tool(c);
    refresh_login(c);
}

/// Ask the server whether yt-dlp is there.
pub fn refresh_tool(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    ui.global::<Dl>().set_tool_checking(true);
    c.worker.spawn(|api| {
        let r: Result<Value, String> = api.get("/tools/ytdlp");
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Dl>();
            g.set_tool_checking(false);
            match r {
                Ok(s) => {
                    g.set_tool_ok(s["available"].as_bool().unwrap_or(false));
                    g.set_tool_version(s["version"].as_str().unwrap_or_default().into());
                    g.set_can_install(s["can_install"].as_bool().unwrap_or(false));
                }
                Err(e) => {
                    g.set_tool_ok(false);
                    c.toast(format!("Компонент загрузки: {e}"), true);
                }
            }
        })))
    });
}

fn install_tool(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    ui.global::<Dl>().set_installing(true);
    c.worker.spawn(|api| {
        let r = api.post("/tools/ytdlp/install", &json!({}));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| match r {
            Ok(r) => super::jobs::started(c, r["job"].as_u64().unwrap_or_default()),
            Err(e) => {
                if let Some(ui) = c.window() {
                    ui.global::<Dl>().set_installing(false);
                }
                c.toast(format!("Установка компонента загрузки: {e}"), true);
            }
        })))
    });
}

/// The install job ended (see [`super::jobs`]).
pub fn tool_installed(c: &mut Ctx, ok: bool) {
    if let Some(ui) = c.window() {
        ui.global::<Dl>().set_installing(false);
    }
    if ok {
        c.toast("Компонент загрузки установлен", false);
    }
    refresh_tool(c);
}

fn details(info: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for k in ["uploader", "site"] {
        if let Some(s) = info[k].as_str().filter(|s| !s.is_empty()) {
            parts.push(s.to_string());
        }
    }
    let n = info["entries"].as_array().map(|a| a.len()).unwrap_or(0);
    if n > 1 {
        parts.push(format!("плейлист: {n} видео"));
    }
    let ms = info["duration_ms"].as_i64().unwrap_or(0);
    if ms > 0 {
        parts.push(hms(ms));
    }
    parts.join(" · ")
}

fn probe(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Dl>();
    let url = g.get_url().trim().to_string();
    if url.is_empty() {
        return;
    }
    let ch = c.model.ch();
    c.download.serial += 1;
    let serial = c.download.serial;
    g.set_step("probing".into());
    g.set_has_thumb(false);
    // the server waits for the site: allow more than the usual timeout
    let api = c.worker.api().patient(150);
    c.worker.spawn(move |_| {
        let r = api.post(&format!("/channels/{ch}/media/download/probe"), &json!({ "url": url }));
        let thumb = r.as_ref().ok().and_then(|i| i["thumbnail"].as_str().map(str::to_string)).and_then(|u| fetch_picture(&u));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if serial != c.download.serial {
                return;
            }
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Dl>();
            g.set_login_needed(r.as_ref().err().is_some_and(|e| needs_login(e)));
            match r {
                Ok(info) if info["is_live"].as_bool() == Some(true) => {
                    g.set_error(
                        "Это прямая трансляция — её нельзя скачать целиком. Добавьте её в медиатеку как поток через «Импорт».".into(),
                    );
                    g.set_step("error".into());
                }
                Ok(info) => show_info(c, &info, thumb),
                Err(e) => {
                    g.set_error(crate::ru::server(&e).into());
                    g.set_step("error".into());
                }
            }
        })))
    });
}

fn show_info(c: &mut Ctx, info: &Value, thumb: Option<(u32, u32, Vec<u8>)>) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Dl>();
    let qualities = info["qualities"].as_array().cloned().unwrap_or_default();
    c.download.quality_ids = qualities.iter().filter_map(|q| q["id"].as_str().map(str::to_string)).collect();
    c.download.entries = info["entries"].clone();
    let labels: Vec<SharedString> = qualities.iter().map(|q| q["label"].as_str().unwrap_or_default().into()).collect();
    g.set_qualities(vec_model(labels));
    // a sensible default: 1080p when there is such a choice, otherwise the best
    let default = c.download.quality_ids.iter().position(|q| q == "h1080").unwrap_or(0);
    g.set_quality(default as i32);
    let title = info["title"].as_str().unwrap_or_default();
    g.set_title(if title.is_empty() { g.get_url() } else { title.into() });
    g.set_details(details(info).into());
    match thumb {
        Some((w, h, rgba)) => {
            g.set_thumb(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h)));
            g.set_has_thumb(true);
        }
        None => g.set_has_thumb(false),
    }
    g.set_step("ready".into());
}

/// The preview picture of the link (fetched by this computer; small JPEG / PNG).
fn fetch_picture(url: &str) -> Option<(u32, u32, Vec<u8>)> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return None;
    }
    let cfg = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(10))).build();
    let mut res = ureq::Agent::new_with_config(cfg).get(url).header("User-Agent", "LauraBoard").call().ok()?;
    let bytes = res.body_mut().with_config().limit(4 * 1024 * 1024).read_to_vec().ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let img = img.thumbnail(480, 270).to_rgba8();
    Some((img.width(), img.height(), img.into_raw()))
}

fn start(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Dl>();
    let Some(quality) = c.download.quality_ids.get(g.get_quality().max(0) as usize).cloned() else { return };
    let kind = usize::try_from(g.get_kind() - 1).ok().and_then(|i| KINDS.get(i).copied());
    let cat = g.get_category().trim().to_string();
    let body = json!({
        "entries": c.download.entries,
        "quality": quality,
        "kind": kind,
        "category": if cat.is_empty() { Value::Null } else { json!(cat) },
        "folder": folders::option_path(c, g.get_folder()),
    });
    g.set_starting(true);
    let ch = c.model.ch();
    let api = c.worker.api().patient(60);
    c.worker.spawn(move |_| {
        let r = api.post(&format!("/channels/{ch}/media/download"), &body);
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Dl>();
            g.set_starting(false);
            match r {
                Ok(r) => {
                    super::jobs::started(c, r["job"].as_u64().unwrap_or_default());
                    g.set_open(false);
                    g.set_url(SharedString::new());
                    g.set_step(SharedString::new());
                    c.toast("Загрузка началась — прогресс в шапке окна", false);
                }
                Err(e) => c.toast(format!("Загрузка: {e}"), true),
            }
        })))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_line() {
        let info = json!({"uploader": "Канал", "site": "Youtube", "duration_ms": 61_000, "entries": [{}]});
        assert_eq!(details(&info), format!("Канал · Youtube · {}", hms(61_000)));
        let pl = json!({"site": "youtube:tab", "entries": [{}, {}, {}]});
        assert_eq!(details(&pl), "youtube:tab · плейлист: 3 видео");
    }

    #[test]
    fn sign_in_errors() {
        assert!(needs_login("[youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users."));
        assert!(!needs_login("[youtube] abc: Video unavailable"));
        assert!(!needs_login("[youtube] Sign in to confirm you are not a bot. Use --cookies"));
        assert!(!needs_login("[vimeo] age-restricted"));
    }
}
