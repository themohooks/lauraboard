//! Graphics render: a draft graphics configuration drawn by the same
//! compositor the encoder uses, so editors see the exact result.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use lb_core::Ms;
use lb_core::config::Role;
use lb_core::graphics::{ActiveGraphic, ElementKind, GraphicsConfig};
use lb_gfx::{Compositor, FrameContext};

use super::channel;
use crate::AppState;
use crate::auth::Caller;
use crate::error::{ApiError, ApiResult};

/// Largest frame side accepted (guards memory on odd channel settings).
const MAX_SIDE: u32 = 4096;

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
}

/// Transparent PNG of the given elements at the channel frame size.
pub async fn render(State(s): State<AppState>, c: Caller, Path(id): Path<String>, Json(b): Json<RenderBody>) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let h = channel(&s, &id)?;
    let root = h.shared.root.clone();
    let st = h.status();
    let (w, hgt, vars) = h.read(|d| {
        let on_air = st.on_air.as_ref().map(|o| &o.item);
        let vars = lb_engine::channel::vars::build(d, on_air, st.next.as_ref(), st.next_program.as_ref(), &b.vars);
        (d.config.video.width, d.config.video.height, vars)
    });
    if w == 0 || hgt == 0 || w > MAX_SIDE || hgt > MAX_SIDE {
        return Err(ApiError::bad_request(format!("unsupported frame size {w}x{hgt}")));
    }
    let remaining = st.on_air.as_ref().map(|o| o.remaining_ms).unwrap_or(60_000);
    let next_ms = st.time_to_next_program_ms;
    let png = tokio::task::spawn_blocking(move || {
        let active = active_set(&b.config, b.elements.as_deref(), b.t_ms);
        let mut comp = Compositor::new(w, hgt, b.config, &root);
        let now = chrono::Local::now().naive_local();
        comp.render(&FrameContext { now, active: &active, vars: &vars, item_remaining_ms: remaining, next_program_ms: next_ms });
        let png = comp.png()?;
        match b.width {
            Some(ow) if ow > 0 && ow < w => scale_png(&png, ow),
            _ => Some(png),
        }
    })
    .await
    .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "render failed"))?;
    Ok(([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-store")], png).into_response())
}

/// Elements drawn as if shown `t_ms` ago (an hour by default: animations done).
fn active_set(cfg: &GraphicsConfig, only: Option<&[String]>, t_ms: Option<Ms>) -> Vec<ActiveGraphic> {
    let children: Vec<&str> = cfg
        .elements
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Group { children } => Some(children.iter().map(String::as_str)),
            _ => None,
        })
        .flatten()
        .collect();
    let mut v: Vec<ActiveGraphic> = cfg
        .elements
        .iter()
        .filter(|e| match only {
            Some(ids) => ids.contains(&e.id),
            None => !children.contains(&e.id.as_str()),
        })
        .map(|e| ActiveGraphic {
            element: e.id.clone(),
            layer: e.layer,
            rule: None,
            priority: 0,
            shown_ms: t_ms.unwrap_or(3_600_000).max(0),
            hide_in_ms: None,
        })
        .collect();
    v.sort_by_key(|a| a.layer);
    v
}

fn scale_png(png: &[u8], width: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let height = ((img.height() as u64 * width as u64) / img.width().max(1) as u64).max(1) as u32;
    let small = img.resize_exact(width, height, image::imageops::FilterType::Triangle);
    let mut out = std::io::Cursor::new(Vec::new());
    small.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}
