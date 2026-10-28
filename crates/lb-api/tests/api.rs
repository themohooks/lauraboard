//! API smoke tests against a temporary project (no FFmpeg needed: the
//! channel is not started).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use lb_api::{AppState, router};
use lb_core::config::{ApiToken, Role};
use lb_core::project::Project;
use lb_engine::Engine;

fn project(tokens: Vec<ApiToken>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::init(dir.path(), "main").unwrap();
    p.app.api.tokens = tokens;
    p.save_app().unwrap();
    let ch = p.channel_mut("main").unwrap();
    ch.config.autostart = false;
    ch.library.upsert(lb_core::media::MediaItem::new("a", "media/a.mp4", lb_core::media::MediaKind::Program, lb_core::Dur::secs(60)));
    ch.save().unwrap();
    dir
}

async fn app(dir: &tempfile::TempDir) -> axum::Router {
    let engine = Arc::new(Engine::start(dir.path()).unwrap());
    let api = Arc::new(engine.app.api.clone());
    router(AppState { engine, api })
}

async fn call(app: &axum::Router, method: &str, uri: &str, token: Option<&str>, body: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty)).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn local_access_without_tokens() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let (st, body) = call(&app, "GET", "/api/v1/channels", None, None).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.contains("\"channel\":\"main\""));
    let (st, body) = call(&app, "GET", "/api/v1/channels/main/media", None, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.contains("media/a.mp4"));
    let (st, _) = call(&app, "GET", "/api/v1/channels/nope/status", None, None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn graphic_commands_immediately_publish_the_new_state() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let graphics = r##"{"elements":[{"id":"logo","type":"rect","color":"#ffffff","width":20,"height":20}],"rules":[{"id":"always","element":"logo"}]}"##;
    let (status, body) = call(&app, "PUT", "/api/v1/channels/main/graphics", None, Some(graphics)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for (command, visible) in [("hide", false), ("show", true), ("auto", true)] {
        let body = if command == "auto" {
            r#"{"cmd":"graphic_auto","element":"logo"}"#.to_string()
        } else {
            format!(r#"{{"cmd":"graphic","element":"logo","action":"{command}"}}"#)
        };
        let (status, response) = call(&app, "POST", "/api/v1/channels/main/command", None, Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "{response}");
        let (_, response) = call(&app, "GET", "/api/v1/channels/main/status", None, None).await;
        let state: serde_json::Value = serde_json::from_str(&response).unwrap();
        let on = state["graphics"].as_array().unwrap().iter().any(|g| g["element"] == "logo");
        assert_eq!(on, visible, "{command}: {response}");
    }
}

#[tokio::test]
async fn roles_are_enforced() {
    let dir = project(vec![
        ApiToken { name: "view".into(), token: "v-token".into(), role: Role::Viewer },
        ApiToken { name: "op".into(), token: "o-token".into(), role: Role::Operator },
    ]);
    let app = app(&dir).await;
    assert_eq!(call(&app, "GET", "/api/v1/channels", None, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&app, "GET", "/api/v1/channels", Some("bad"), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&app, "GET", "/api/v1/channels", Some("v-token"), None).await.0, StatusCode::OK);
    assert_eq!(call(&app, "POST", "/api/v1/channels/main/skip", Some("v-token"), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "GET", "/api/v1/channels/main/config", Some("o-token"), None).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn old_errors_do_not_block_other_documents() {
    let dir = project(vec![]);
    // a playlist with a missing media id: an error that already exists on disk
    let mut p = Project::load(dir.path()).unwrap();
    let ch = p.channel_mut("main").unwrap();
    let mut broken = lb_core::playlist::Playlist::new("broken");
    broken.entries.push(lb_core::playlist::PlaylistEntry::new("gone"));
    ch.playlists.push(broken);
    ch.save().unwrap();
    let app = app(&dir).await;
    // saving graphics must still work …
    let (st, body) = call(&app, "PUT", "/api/v1/channels/main/graphics", None, Some(r#"{}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    // … while a new error in the saved document is still rejected and named
    let (st, body) = call(&app, "PUT", "/api/v1/channels/main/graphics", None, Some(r#"{"rules":[{"id":"r","element":"nope"}]}"#)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(body.contains("unknown element 'nope'") && !body.contains("gone"), "{body}");
}

#[tokio::test]
async fn create_channel_while_running() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let (st, body) = call(&app, "POST", "/api/v1/channels", None, Some(r#"{"id":"second","name":"Второй","copy_from":"main"}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(dir.path().join("channels/second/channel.toml").exists());
    let (_, body) = call(&app, "GET", "/api/v1/channels", None, None).await;
    assert!(body.contains("\"channel\":\"second\""), "{body}");
    let (st, body) = call(&app, "GET", "/api/v1/channels/second/config", None, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.contains("Второй") && body.contains("\"autostart\":false"), "{body}");
    // duplicates and bad ids are rejected
    assert_eq!(call(&app, "POST", "/api/v1/channels", None, Some(r#"{"id":"second"}"#)).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(call(&app, "POST", "/api/v1/channels", None, Some(r#"{"id":"../x"}"#)).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn playlist_crud_and_plan_preview() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let (st, body) = call(&app, "POST", "/api/v1/channels/main/playlists", None, Some(r#"{"id":"main","entries":[{"media":"a"}]}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let (st, _) = call(&app, "POST", "/api/v1/channels/main/playlists", None, Some(r#"{"id":"bad","entries":[{"media":"zzz"}]}"#)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, body) = call(&app, "POST", "/api/v1/channels/main/playlists/main/entries", None, Some(r#"{"media":"a","index":0}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let (_, body) = call(&app, "GET", "/api/v1/channels/main/playlists", None, None).await;
    assert!(body.contains("\"entries\":2"), "{body}");
    assert!(dir.path().join("channels/main/playlists/main.json").exists());
    let (st, body) = call(&app, "GET", "/api/v1/channels/main/plan?hours=2", None, None).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let (st, body) = call(&app, "GET", "/api/v1/channels/main/validate", None, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.contains("file not found"), "{body}");
}

#[tokio::test]
async fn plan_with_draft_documents() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let (st, body) = call(&app, "POST", "/api/v1/channels/main/plan", None, Some(r#"{"hours":2}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.contains("\"items\""), "{body}");
    let (st, body) =
        call(&app, "POST", "/api/v1/channels/main/plan", None, Some(r#"{"from":"2030-01-01T10:00:00","hours":1,"rules":{}}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    // a malformed draft is rejected, never stored
    let (st, _) = call(&app, "POST", "/api/v1/channels/main/plan", None, Some(r#"{"rules":{"rules":"nope"}}"#)).await;
    assert!(st.is_client_error());
}

async fn call_bytes(app: &axum::Router, method: &str, uri: &str, body: &str) -> (StatusCode, Vec<u8>) {
    let req =
        Request::builder().method(method).uri(uri).header("content-type", "application/json").body(Body::from(body.to_string())).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    (status, axum::body::to_bytes(res.into_body(), 16 << 20).await.unwrap().to_vec())
}

#[tokio::test]
async fn graphics_render_draws_the_draft() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let cfg = r##"{"config":{"elements":[{"id":"plate","type":"rect","color":"#FF0000","width":200,"height":100,
        "placement":{"anchor":"top_left","x":0,"y":0}}]},"width":480}"##;
    let (st, png) = call_bytes(&app, "POST", "/api/v1/channels/main/graphics/render", cfg).await;
    assert_eq!(st, StatusCode::OK, "{}", String::from_utf8_lossy(&png));
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(img.width(), 480);
    // the plate is drawn top-left and the rest of the frame stays transparent
    assert_eq!(img.get_pixel(5, 5).0, [255, 0, 0, 255]);
    assert_eq!(img.get_pixel(470, 260).0[3], 0);
    // half-way through a fade-in the plate is partly transparent
    let cfg = cfg
        .replace(r#""width":480"#, r#""width":480,"t_ms":250"#)
        .replace(r#""y":0}"#, r#""y":0},"animation":{"in":"fade","duration":"500ms"}"#);
    let (st, png) = call_bytes(&app, "POST", "/api/v1/channels/main/graphics/render", &cfg).await;
    assert_eq!(st, StatusCode::OK, "{}", String::from_utf8_lossy(&png));
    let a = image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(5, 5).0[3];
    assert!(a > 0 && a < 255, "alpha {a}");
}

#[tokio::test]
async fn media_thumbnails() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    // missing file and unknown media: no thumbnail
    assert_eq!(call(&app, "GET", "/api/v1/channels/main/media/a/thumb.jpg", None, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "GET", "/api/v1/channels/main/media/zzz/thumb.jpg", None, None).await.0, StatusCode::NOT_FOUND);
    std::fs::create_dir_all(dir.path().join("media")).unwrap();
    let ok = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=640x360:rate=25:duration=2",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(dir.path().join("media/a.mp4"))
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("ffmpeg not available: frame extraction not tested");
        return;
    }
    let req = Request::builder().uri("/api/v1/channels/main/media/a/thumb.jpg?w=160").body(Body::empty()).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "image/jpeg");
    let jpg = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let img = image::load_from_memory(&jpg).unwrap();
    assert_eq!((img.width(), img.height()), (160, 90));
    let cached = std::fs::read_dir(dir.path().join("cache/thumbs/main")).unwrap().count();
    assert_eq!(cached, 1);
}

#[tokio::test]
async fn channels_are_renamed_duplicated_and_deleted() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let (st, body) = call(&app, "PUT", "/api/v1/channels/main", None, Some(r#"{"name":"Главный","autostart":true}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.contains("\"name\":\"Главный\"") && body.contains("\"autostart\":true"), "{body}");

    let (st, body) = call(&app, "POST", "/api/v1/channels/main/duplicate", None, Some(r#"{"id":"copy"}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.contains("Главный (копия)"), "{body}");
    let (_, body) = call(&app, "GET", "/api/v1/channels/copy/media", None, None).await;
    assert!(body.contains("media/a.mp4"), "library copied: {body}");

    let (st, body) = call(&app, "PUT", "/api/v1/channels/copy", None, Some(r#"{"id":"second"}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(dir.path().join("channels/second/channel.toml").exists());
    assert!(!dir.path().join("channels/copy").exists());
    assert_eq!(call(&app, "GET", "/api/v1/channels/copy/status", None, None).await.0, StatusCode::NOT_FOUND);

    let (st, body) = call(&app, "DELETE", "/api/v1/channels/second", None, None).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.contains("deleted"), "{body}");
    assert!(!dir.path().join("channels/second").exists());
    let (_, body) = call(&app, "GET", "/api/v1/channels", None, None).await;
    assert!(!body.contains("\"second\""), "{body}");
    assert_eq!(call(&app, "POST", "/api/v1/channels/main/command", None, Some(r#"{"cmd":"shutdown"}"#)).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn media_copies_and_folders() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let src = dir.path().join("Логотип.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])).save(&src).unwrap();
    let body = serde_json::json!({ "paths": [src.to_string_lossy()], "copy": true, "folder": "Графика/Логотипы" }).to_string();
    let (st, body) = call(&app, "POST", "/api/v1/channels/main/media/import", None, Some(&body)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let job: serde_json::Value = serde_json::from_str(&body).unwrap();
    let job = job["job"].as_u64().unwrap();
    let mut state = String::new();
    for _ in 0..100 {
        let (_, b) = call(&app, "GET", &format!("/api/v1/jobs/{job}"), None, None).await;
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        state = v["state"].as_str().unwrap().to_string();
        if state != "running" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(state, "done");
    let (_, body) = call(&app, "GET", "/api/v1/channels/main/media", None, None).await;
    let items: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    let it = items.iter().find(|m| m["original_name"] == "Логотип.png").expect("copied item");
    let path = it["path"].as_str().unwrap();
    assert!(path.starts_with("channels/main/media/") && path.ends_with(".png"), "{path}");
    assert_eq!(it["folder"], "Графика/Логотипы");
    assert!(dir.path().join(path).exists());

    let (_, body) = call(&app, "GET", "/api/v1/channels/main/media/folders", None, None).await;
    assert_eq!(body, r#"["Графика","Графика/Логотипы"]"#);
    let (st, body) =
        call(&app, "POST", "/api/v1/channels/main/media/folders", None, Some(r#"{"op":"rename","path":"Графика","to":"Оформление"}"#))
            .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body, r#"["Оформление","Оформление/Логотипы"]"#);

    let id = it["id"].as_str().unwrap();
    let (st, _) = call(&app, "DELETE", &format!("/api/v1/channels/main/media/{id}"), None, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(!dir.path().join(path).exists(), "own copy removed with the item");
}

async fn wait_job(app: &axum::Router, body: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let job = v["job"].as_u64().unwrap_or_else(|| panic!("no job: {body}"));
    for _ in 0..300 {
        let (_, b) = call(app, "GET", &format!("/api/v1/jobs/{job}"), None, None).await;
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        if v["state"] != "running" {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("job {job} did not finish");
}

#[tokio::test]
async fn channel_export_and_import() {
    let dir = project(vec![]);
    // an external file referenced by the library: travels with the archive
    let ext = dir.path().join("external.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 9, 9, 255])).save(&ext).unwrap();
    let mut p = Project::load(dir.path()).unwrap();
    let ch = p.channel_mut("main").unwrap();
    ch.library.upsert(lb_core::media::MediaItem::new("ext", "external.png", lb_core::media::MediaKind::Image, lb_core::Dur::secs(5)));
    ch.config.outputs.push(lb_core::config::OutputTarget { name: "yt".into(), url: "rtmp://x/key".into(), enabled: true, format: None });
    ch.save().unwrap();
    let app = app(&dir).await;

    let zip = dir.path().join("out/main.zip");
    let body = serde_json::json!({ "path": zip.to_string_lossy(), "parts": { "history": false } }).to_string();
    let (st, body) = call(&app, "POST", "/api/v1/channels/main/export", None, Some(&body)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let job = wait_job(&app, &body).await;
    assert_eq!(job["state"], "done", "{job}");
    assert!(zip.exists());

    let body = serde_json::json!({ "path": zip.to_string_lossy() }).to_string();
    let (st, body) = call(&app, "POST", "/api/v1/archives/inspect", None, Some(&body)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.contains("\"channel\":\"main\"") && body.contains("\"media_files\":1"), "{body}");

    // same id is taken
    let same = serde_json::json!({ "path": zip.to_string_lossy() }).to_string();
    let (st, _) = call(&app, "POST", "/api/v1/archives/import", None, Some(&same)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let body = serde_json::json!({ "path": zip.to_string_lossy(), "id": "restored", "name": "Восстановленный" }).to_string();
    let (st, body) = call(&app, "POST", "/api/v1/archives/import", None, Some(&body)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let job = wait_job(&app, &body).await;
    assert_eq!(job["state"], "done", "{job}");

    let (_, body) = call(&app, "GET", "/api/v1/channels/restored/media", None, None).await;
    let items: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    let ext = items.iter().find(|m| m["id"] == "ext").unwrap();
    let path = ext["path"].as_str().unwrap();
    assert!(path.starts_with("channels/restored/media/"), "{path}");
    assert!(dir.path().join(path).exists());
    let (_, cfg) = call(&app, "GET", "/api/v1/channels/restored/config", None, None).await;
    assert!(cfg.contains("Восстановленный") && !cfg.contains("rtmp://x/key"), "outputs are not exported by default: {cfg}");
    assert!(!std::fs::read_dir(dir.path().join("channels")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with('.')));
}

#[tokio::test]
async fn downloads_reject_bad_requests() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let (st, body) = call(&app, "GET", "/api/v1/tools/ytdlp", None, None).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.contains("\"available\""), "{body}");
    // checked before yt-dlp is looked for: no program is ever started with these
    let (st, _) = call(&app, "POST", "/api/v1/channels/main/media/download", None, Some(r#"{"url":"file:///etc/passwd"}"#)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) =
        call(&app, "POST", "/api/v1/channels/main/media/download", None, Some(r#"{"url":"https://example.com/v","quality":"--exec x"}"#))
            .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&app, "POST", "/api/v1/channels/nope/media/download/probe", None, Some(r#"{"url":"https://example.com/v"}"#)).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn editor_checks_the_item() {
    let dir = project(vec![]);
    let app = app(&dir).await;
    let (st, _) = call(&app, "POST", "/api/v1/channels/main/media/nope/edit", None, Some("{}")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // "a" was never probed: no video known, nothing is started
    let (st, body) = call(&app, "POST", "/api/v1/channels/main/media/a/edit", None, Some(r#"{"spec":{"start_ms":1000}}"#)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("no video"), "{body}");
    let (st, _) = call(&app, "POST", "/api/v1/channels/main/media/a/edit/frame.jpg?at=5000", None, Some("{}")).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}
