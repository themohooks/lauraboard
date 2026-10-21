//! Demo logo rendered as a transparent PNG.

use std::path::Path;

use anyhow::Result;

use lb_core::graphics::{Anchor, Animation, Element, ElementKind, GraphicsConfig, Placement, TextStyle};

pub fn write(path: &Path) -> Result<()> {
    let el = Element {
        id: "logo".into(),
        name: "logo".into(),
        kind: ElementKind::Text {
            text: "LAURA TV".into(),
            style: TextStyle {
                size: 40.0,
                color: "#FFFFFF".into(),
                background: Some("#C0182BCC".into()),
                corner_radius: 10.0,
                padding: 10.0,
                shadow_color: Some("#00000080".into()),
                ..Default::default()
            },
        },
        placement: Placement { anchor: Anchor::TopLeft, ..Default::default() },
        opacity: 1.0,
        layer: 0,
        animation: Animation::default(),
        profiles: vec![],
    };
    let cfg = GraphicsConfig { layers: vec![], elements: vec![el], rules: vec![], vars: Default::default() };
    let mut c = lb_gfx::Compositor::new(240, 80, cfg, Path::new("."));
    let act = vec![lb_core::graphics::ActiveGraphic {
        element: "logo".into(),
        layer: 0,
        rule: None,
        priority: 0,
        shown_ms: 60_000,
        hide_in_ms: None,
    }];
    let vars = Default::default();
    c.render(&lb_gfx::FrameContext {
        now: chrono::Local::now().naive_local(),
        active: &act,
        vars: &vars,
        item_remaining_ms: 0,
        next_program_ms: None,
    });
    let png = c.png().ok_or_else(|| anyhow::anyhow!("logo encoding failed"))?;
    std::fs::write(path, png)?;
    Ok(())
}
