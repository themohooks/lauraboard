//! Editor previews: which elements a draft shows, the same for the server
//! render and for a GUI drawing the draft itself.

use lb_core::Ms;
use lb_core::graphics::{ActiveGraphic, ElementKind, GraphicsConfig};

/// Elements drawn as if shown `t_ms` ago (an hour by default: animations
/// done). `only`: these elements; default: every top-level element (not a
/// group child).
pub fn active_set(cfg: &GraphicsConfig, only: Option<&[String]>, t_ms: Option<Ms>) -> Vec<ActiveGraphic> {
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

/// The draft has HTML / SWF elements: they need a browser to be drawn.
pub fn has_web(cfg: &GraphicsConfig) -> bool {
    cfg.elements.iter().any(|e| matches!(&e.kind, ElementKind::Image { path } if crate::web::is_web(path)))
}
