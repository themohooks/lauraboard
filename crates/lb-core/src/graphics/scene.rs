//! Graphics of a scene on air: the scene keeps its own elements and rules
//! (stored in the scene, edited apart from the channel graphics); while the
//! scene is on air they are merged with the channel's graphics.

use super::{ElementKind, GraphicsConfig};

/// Prefix of a scene's element and rule ids in the merged configuration
/// (`<scene>/<id>`), so they never clash with the channel's own.
pub fn scene_prefix(scene: &str) -> String {
    format!("{scene}/")
}

impl GraphicsConfig {
    /// The configuration to draw while scene `scene_id` is on air: the
    /// channel graphics (`channel`: their rules too, otherwise only rules
    /// limited to this scene by its media id) plus the scene's own.
    pub fn with_scene(&self, scene_id: &str, own: &GraphicsConfig, channel: bool) -> GraphicsConfig {
        let mut out = self.clone();
        if !channel {
            out.rules.retain(|r| r.conditions.media.iter().any(|m| m == scene_id));
        }
        if own.elements.is_empty() {
            return out;
        }
        let p = scene_prefix(scene_id);
        for e in &own.elements {
            let mut e = e.clone();
            e.id = format!("{p}{}", e.id);
            if let ElementKind::Group { children } = &mut e.kind {
                for c in children.iter_mut() {
                    *c = format!("{p}{c}");
                }
            }
            out.elements.push(e);
        }
        for r in &own.rules {
            let mut r = r.clone();
            r.id = format!("{p}{}", r.id);
            r.element = format!("{p}{}", r.element);
            out.rules.push(r);
        }
        for l in &own.layers {
            if !out.layers.iter().any(|x| x.id == l.id) {
                out.layers.push(l.clone());
            }
        }
        // the scene's defaults win while it is on air
        out.vars.extend(own.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
        out
    }
}
