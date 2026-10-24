//! Ready-made textures for the "picture background" of shapes and plates:
//! the channel's plate backgrounds (made by the server), picked from a
//! dialog with previews.

use serde_json::Value;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};

use super::{props_changed, s, with};
use crate::bridge::{Ctx, vec_model};
use crate::net::Msg;
use crate::shell::{AppWindow, Gfx, TextureRow};

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Gfx>();
    g.on_open_textures(|| with(open));
    g.on_texture_pick(|p| {
        with(move |c| {
            if let Some(ui) = c.window() {
                let g = ui.global::<Gfx>();
                g.set_sh_image(p);
                g.set_tex_open(false);
            }
            props_changed(c);
        })
    });
}

/// Small preview of a PNG.
fn preview(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
    let img = img.resize(360, 72, image::imageops::FilterType::Triangle).to_rgba8();
    Some((img.width(), img.height(), img.into_raw()))
}

fn open(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    g.set_tex_open(true);
    g.set_tex_loading(true);
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        let list: Vec<Value> = api.get(&format!("/channels/{ch}/graphics/backgrounds"))?;
        let mut out = Vec::new();
        for t in list {
            let path = t["path"].as_str().unwrap_or_default().to_string();
            let title = t["title"].as_str().unwrap_or_default().to_string();
            let file = t["file"].as_str().unwrap_or_default();
            let px = api.get_bytes(&format!("/channels/{ch}/graphics/backgrounds/{file}")).ok().and_then(|b| preview(&b));
            out.push((path, title, px));
        }
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Gfx>();
            let rows: Vec<TextureRow> = out
                .into_iter()
                .map(|(path, title, px)| {
                    let image = px.map(|(w, h, rgba)| Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h)));
                    TextureRow { path: s(path), title: s(title), has_image: image.is_some(), image: image.unwrap_or_default() }
                })
                .collect();
            g.set_tex_list(vec_model(rows));
            g.set_tex_loading(false);
        })))
    });
}
