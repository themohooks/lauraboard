//! Graphics render: a draft graphics configuration drawn by the same
//! compositor the encoder uses, so editors see the exact result.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use lb_core::Ms;
use lb_core::config::Role;
use lb_core::graphics::{GraphicsConfig, ProgramClock};
use lb_gfx::{Compositor, FrameContext};

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

/// Largest frame side accepted (guards memory on odd channel settings).
const MAX_SIDE: u32 = 4096;

struct CachedRenderer {
    root: PathBuf,
    width: u32,
    height: u32,
    interactive: bool,
    compositor: Arc<Mutex<Compositor>>,
}

fn renderer(
    root: &std::path::Path,
    width: u32,
    height: u32,
    cfg: &GraphicsConfig,
    interactive: bool,
    refresh: bool,
) -> Arc<Mutex<Compositor>> {
    static CACHE: OnceLock<Mutex<Vec<CachedRenderer>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut entries = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(index) =
        entries.iter().position(|e| e.root == root && e.width == width && e.height == height && e.interactive == interactive)
    {
        let entry = entries.remove(index);
        if !refresh {
            let comp = entry.compositor.clone();
            entries.push(entry);
            return comp;
        }
    }
    let compositor = Arc::new(Mutex::new(Compositor::new(width, height, cfg.clone(), root)));
    if entries.len() >= 4 {
        entries.remove(0);
    }
    entries.push(CachedRenderer { root: root.into(), width, height, interactive, compositor: compositor.clone() });
    compositor
}

#[derive(Deserialize)]
pub struct RenderBody {
    /// Configuration to draw (usually an unsaved draft).
    pub config: GraphicsConfig,
    /// Extra template variables, on top of the live ones.
    #[serde(default)]
    pub vars: HashMap<String, String>,
    /// Time since the elements appeared (in-animation); default: fully shown.
    #[serde(default)]
    pub t_ms: Option<Ms>,
    /// Output width (scaled down, aspect kept); default: channel frame width.
    #[serde(default)]
    pub width: Option<u32>,
    /// Elements to draw; default: every top-level element (not a group child).
    #[serde(default)]
    pub elements: Option<Vec<String>>,
    /// Interactive atlas for local move/resize; three vertically stacked planes.
    #[serde(default)]
    pub edit_layer: Option<String>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub refresh: bool,
}

/// What the preview draws besides the configuration: template variables
/// and the clocks of the air. Also served on its own
/// (`GET /graphics/live`), so an operator GUI on the same computer can draw
/// drafts itself with the same compositor.
#[derive(Serialize)]
pub struct Live {
    pub width: u32,
    pub height: u32,
    pub vars: HashMap<String, String>,
    pub remaining_ms: Ms,
    pub next_program_ms: Option<Ms>,
    pub program: ProgramClock,
}

fn live(h: &lb_engine::channel::ChannelHandle, extra: &HashMap<String, String>) -> Live {
    let st = h.status();
    let (width, height, vars) = h.read(|d| {
        let on_air = st.on_air.as_ref().map(|o| &o.item);
        let vars = lb_engine::channel::vars::build(d, on_air, st.next.as_ref(), st.next_program.as_ref(), extra);
        (d.config.video.width, d.config.video.height, vars)
    });
    let remaining = st.on_air.as_ref().map(|o| o.remaining_ms).unwrap_or(60_000);
    // the editor preview: the programme of the item on air as aired so far
    // (half a minute shown when nothing is on air)
    let program = match &st.program {
        Some(p) => p.clone(),
        None => ProgramClock { current: Some((lb_core::media::MediaKind::Program, remaining * 2)), ..Default::default() },
    };
    Live { width, height, vars, remaining_ms: remaining, next_program_ms: st.time_to_next_program_ms, program }
}

/// Variables and clocks of the air for a local preview.
pub async fn live_state(State(s): State<AppState>, c: Caller, Path(id): Path<String>) -> ApiResult<Json<Live>> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    Ok(Json(live(&h, &HashMap::new())))
}

/// Transparent PNG of the given elements at the channel frame size.
pub async fn render(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(b): Json<RenderBody>) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let root = h.shared.root.clone();
    let Live { width: w, height: hgt, vars, remaining_ms: remaining, next_program_ms: next_ms, program } = live(&h, &b.vars);
    if w == 0 || hgt == 0 || w > MAX_SIDE || hgt > MAX_SIDE {
        return Err(ApiError::bad_request(format!("unsupported frame size {w}x{hgt}")));
    }
    let (png, pending) = tokio::task::spawn_blocking(move || {
        let active = lb_gfx::preview::active_set(&b.config, b.elements.as_deref(), b.t_ms);
        let cached = renderer(&root, w, hgt, &b.config, b.interactive, b.refresh);
        let mut comp = cached.lock().unwrap_or_else(|e| e.into_inner());
        comp.set_config(b.config);
        comp.set_editing(b.interactive);
        // Editing must never wait for a browser to start or resize. The next
        // refresh can use its latest frame; exact/animation previews still wait.
        if !b.interactive {
            comp.prepare_web(std::time::Duration::from_secs(15))?;
        }
        let now = chrono::Local::now().naive_local();
        let ctx = FrameContext {
            now,
            active: &active,
            vars: &vars,
            item_remaining_ms: remaining,
            next_program_ms: next_ms,
            program: Some(&program),
        };
        if let Some(selected) = &b.edit_layer {
            if let Some(atlas) = comp.png_edit_layers(&ctx, selected, b.width.unwrap_or(w)) {
                return Ok((Some(atlas), comp.pending_web()));
            }
        }
        comp.render(&ctx);
        Ok::<_, String>((comp.png_scaled(b.width.unwrap_or(w)), comp.pending_web()))
    })
    .await
    .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| ApiError::bad_request(e))?;
    let png = png.ok_or_else(|| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "render failed"))?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "no-store"),
            (header::HeaderName::from_static("x-lb-preview-pending"), if pending { "1" } else { "0" }),
        ],
        png,
    )
        .into_response())
}
