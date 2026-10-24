//! General settings and background of the scene draft.

use slint::ComponentHandle;

use lb_core::Dur;
use lb_core::media::scene::Background;

use super::super::graphics::color_of;
use super::super::thumbs;
use super::{Ctx, touched};
use crate::fmt::KINDS;
use crate::shell::Scenes;

/// Kinds offered for a scene (the first five of [`KINDS`]).
const SCENE_KINDS: usize = 5;

/// Draft → form fields.
pub fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Scenes>();
    let Some(m) = c.scenes.draft.clone() else { return };
    let Some(s) = &m.scene else { return };
    g.set_f_id(m.id.clone().into());
    g.set_f_title(m.title.clone().into());
    g.set_f_kind(KINDS.iter().take(SCENE_KINDS).position(|k| *k == m.kind).map_or(4, |i| i as i32));
    g.set_f_category(m.category.clone().unwrap_or_default().into());
    g.set_f_duration(m.duration.to_string().into());
    g.set_e_duration("".into());
    match &s.background {
        Background::Color { color } => {
            g.set_f_bg_type(0);
            g.set_f_bg_color(color.clone().into());
            g.set_sw_bg_color(color_of(color));
            g.set_f_bg_media("".into());
        }
        Background::Media { media } => {
            g.set_f_bg_type(1);
            let t = c.model.library.iter().find(|x| &x.id == media).map(super::title).unwrap_or_else(|| format!("{media} — нет в медиатеке"));
            g.set_f_bg_media(t.into());
        }
    }
    show_thumb(c);
}

/// Picture of the background item.
pub fn show_thumb(c: &mut Ctx) {
    let media = match c.scenes.draft.as_ref().and_then(|m| m.scene.as_ref()).map(|s| &s.background) {
        Some(Background::Media { media }) => media.clone(),
        _ => String::new(),
    };
    thumbs::request(c, &media);
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Scenes>();
    match c.thumbs.get(&media) {
        Some(img) => {
            g.set_bg_thumb(img);
            g.set_has_bg_thumb(true);
        }
        None => g.set_has_bg_thumb(false),
    }
}

/// Form fields → draft (typed text stays as typed).
pub fn changed(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Scenes>();
    let Some(m) = c.scenes.draft.as_mut() else { return };
    m.title = g.get_f_title().trim().to_string();
    if let Some(k) = usize::try_from(g.get_f_kind()).ok().filter(|i| *i < SCENE_KINDS).and_then(|i| KINDS.get(i)) {
        m.kind = *k;
    }
    let cat = g.get_f_category().trim().to_string();
    m.category = (!cat.is_empty()).then_some(cat);
    match g.get_f_duration().trim().parse::<Dur>() {
        Ok(d) if d.ms() >= 1000 => {
            m.duration = d;
            g.set_e_duration("".into());
        }
        _ => g.set_e_duration("ЧЧ:ММ:СС, не меньше секунды".into()),
    }
    let Some(s) = m.scene.as_mut() else { return };
    let was_color = matches!(s.background, Background::Color { .. });
    if g.get_f_bg_type() == 0 {
        let hex = g.get_f_bg_color().trim().to_string();
        if crate::logic::placement::valid_color(&hex) {
            s.background = Background::Color { color: hex.clone() };
            g.set_sw_bg_color(color_of(&hex));
        } else if !was_color {
            s.background = Background::Color { color: "#0B1020".into() };
        }
    } else if was_color {
        // stays a colour until a picture is picked
        g.set_f_bg_media("".into());
        g.set_has_bg_thumb(false);
    }
    touched(c);
}

pub fn background_picked(c: &mut Ctx, ids: Vec<String>) {
    let Some(id) = ids.into_iter().next() else { return };
    if c.model.library.iter().find(|m| m.id == id).is_some_and(|m| m.is_scene()) {
        c.toast("Фоном сцены не может быть другая сцена", true);
        return;
    }
    if let Some(s) = c.scenes.draft.as_mut().and_then(|m| m.scene.as_mut()) {
        s.background = Background::Media { media: id };
    }
    touched(c);
    show(c);
}
