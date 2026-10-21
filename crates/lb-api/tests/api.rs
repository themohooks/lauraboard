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
    let (st, body) = call(&app, "POST", "/api/v1/channels/main/plan", None, Some(r#"{"from":"2030-01-01T10:00:00","hours":1,"rules":{}}"#)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    // a malformed draft is rejected, never stored
    let (st, _) = call(&app, "POST", "/api/v1/channels/main/plan", None, Some(r#"{"rules":{"rules":"nope"}}"#)).await;
    assert!(st.is_client_error());
}

async fn call_bytes(app: &axum::Router, method: &str, uri: &str, body: &str) -> (StatusCode, Vec<u8>) {
    let req = Request::builder().method(method).uri(uri).header("content-type", "application/json").body(Body::from(body.to_string())).unwrap();
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
    let cfg = cfg.replace(r#""width":480"#, r#""width":480,"t_ms":250"#).replace(r#""y":0}"#, r#""y":0},"animation":{"in":"fade","duration":"500ms"}"#);
    let (st, png) = call_bytes(&app, "POST", "/api/v1/channels/main/graphics/render", &cfg).await;
    assert_eq!(st, StatusCode::OK, "{}", String::from_utf8_lossy(&png));
    let a = image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(5, 5).0[3];
    assert!(a > 0 && a < 255, "alpha {a}");
}
