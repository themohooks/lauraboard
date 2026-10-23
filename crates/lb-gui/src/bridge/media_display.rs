//! "Показ на экране" section of the media edit sheet: per-item scaling,
//! crop and picture filters ([`MediaDisplay`]), with a preview frame the
//! server renders through the on-air filter chain.

use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, SharedString};

use lb_core::config::ScaleMode;
use lb_core::media::display::{Crop, Rotate};
use lb_core::media::{MediaDisplay, MediaItem, MediaKind};

use super::{Ctx, vec_model, with};
use crate::net::Msg;
use crate::net::poller::decode_jpeg;
use crate::shell::{AppWindow, Lib};

/// Aspect presets offered first (value, label); `None` = from the file.
const ASPECTS: &[(Option<&str>, &str)] =
    &[(None, "Как в файле"), (Some("4:3"), "4:3"), (Some("16:9"), "16:9"), (Some("1.85"), "1.85:1"), (Some("2.35"), "2.35:1"), (Some("1:1"), "1:1")];

const SCALES: &[Option<ScaleMode>] =
    &[None, Some(ScaleMode::Fit), Some(ScaleMode::Fill), Some(ScaleMode::Stretch), Some(ScaleMode::Blur)];
const ROTATIONS: &[Rotate] = &[Rotate::None, Rotate::Cw90, Rotate::R180, Rotate::Ccw90];

#[derive(Default)]
pub struct DisplayState {
    /// Media being edited.
    media: Option<String>,
    /// Aspect values behind `d-aspect-options` (presets + the item's own).
    aspects: Vec<Option<String>>,
    rendering: bool,
    render_again: bool,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Lib>();
    g.on_d_changed(|| with(changed));
    g.on_d_reset(|| {
        with(|c| {
            let Some(ui) = c.window() else { return };
            put(&ui.global::<Lib>(), &c.lib.display.aspects, &MediaDisplay::default());
            changed(c);
        })
    });
}

/// Fill the section from `m` (edit sheet opened).
pub fn load(c: &mut Ctx, m: &MediaItem) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    let st = &mut c.lib.display;
    st.media = Some(m.id.clone());
    st.aspects = ASPECTS.iter().map(|(v, _)| v.map(str::to_string)).collect();
    let mut labels: Vec<SharedString> = ASPECTS.iter().map(|(_, l)| SharedString::from(*l)).collect();
    if let Some(own) = m.display.aspect.as_ref().filter(|a| !st.aspects.contains(&Some(a.to_string()))) {
        st.aspects.push(Some(own.clone()));
        labels.push(own.into());
    }
    g.set_d_aspect_options(vec_model(labels));
    put(&g, &st.aspects, &m.display);
    let can = m.kind != MediaKind::Stream && !m.is_url() && (m.probe.video.is_some() || m.kind == MediaKind::Image);
    g.set_d_can_preview(can);
    g.set_d_has_preview(false);
    g.set_d_preview_note(SharedString::new());
    if can {
        preview(c);
    }
}

/// Settings from the section's fields.
pub fn read(c: &Ctx) -> Option<MediaDisplay> {
    let ui = c.window()?;
    let g = ui.global::<Lib>();
    let at = |i: i32| i.max(0) as usize;
    Some(MediaDisplay {
        scale: SCALES.get(at(g.get_d_scale())).copied().flatten(),
        aspect: c.lib.display.aspects.get(at(g.get_d_aspect())).cloned().flatten(),
        crop: Crop { left: g.get_d_crop_left(), top: g.get_d_crop_top(), right: g.get_d_crop_right(), bottom: g.get_d_crop_bottom() },
        deinterlace: g.get_d_deinterlace(),
        rotate: ROTATIONS.get(at(g.get_d_rotate())).copied().unwrap_or_default(),
        flip_h: g.get_d_flip_h(),
        flip_v: g.get_d_flip_v(),
        brightness: round(g.get_d_brightness(), 100.0),
        contrast: round(g.get_d_contrast(), 100.0),
        saturation: round(g.get_d_saturation(), 100.0),
        gamma: round(g.get_d_gamma(), 100.0),
        sharpen: round(g.get_d_sharpen(), 100.0),
        denoise: round(g.get_d_denoise(), 10.0),
    })
}

/// Slider values carry float noise (0.30000001): keep the file tidy.
fn round(v: f32, k: f32) -> f32 {
    (v * k).round() / k
}

fn put(g: &Lib, aspects: &[Option<String>], d: &MediaDisplay) {
    g.set_d_scale(SCALES.iter().position(|s| *s == d.scale).unwrap_or(0) as i32);
    g.set_d_aspect(aspects.iter().position(|a| *a == d.aspect).unwrap_or(0) as i32);
    g.set_d_rotate(ROTATIONS.iter().position(|r| *r == d.rotate).unwrap_or(0) as i32);
    g.set_d_flip_h(d.flip_h);
    g.set_d_flip_v(d.flip_v);
    g.set_d_deinterlace(d.deinterlace);
    g.set_d_crop_left(d.crop.left);
    g.set_d_crop_top(d.crop.top);
    g.set_d_crop_right(d.crop.right);
    g.set_d_crop_bottom(d.crop.bottom);
    g.set_d_brightness(d.brightness);
    g.set_d_contrast(d.contrast);
    g.set_d_saturation(d.saturation);
    g.set_d_gamma(d.gamma);
    g.set_d_sharpen(d.sharpen);
    g.set_d_denoise(d.denoise);
    g.set_d_default(d.is_default());
}

fn changed(c: &mut Ctx) {
    let Some(d) = read(c) else { return };
    if let Some(ui) = c.window() {
        ui.global::<Lib>().set_d_default(d.is_default());
    }
    preview(c);
}

/// Render the preview frame; requests made while one is running coalesce.
fn preview(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    if !g.get_d_can_preview() {
        return;
    }
    let (Some(media), Some(display)) = (c.lib.display.media.clone(), read(c)) else { return };
    if c.lib.display.rendering {
        c.lib.display.render_again = true;
        return;
    }
    c.lib.display.rendering = true;
    g.set_d_previewing(true);
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        let r = api.post_bytes(&format!("/channels/{ch}/media/{media}/preview.jpg?w=640"), &display);
        let frame = r.as_ref().ok().and_then(|b| decode_jpeg(b));
        let err = r.err();
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let st = &mut c.lib.display;
            st.rendering = false;
            if st.media.as_deref() != Some(media.as_str()) {
                return; // another item was opened meanwhile
            }
            if std::mem::take(&mut st.render_again) {
                preview(c);
                return;
            }
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Lib>();
            g.set_d_previewing(false);
            match frame.filter(|f| f.rgba.len() == (f.width * f.height * 4) as usize) {
                Some(f) => {
                    g.set_d_preview(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&f.rgba, f.width, f.height)));
                    g.set_d_has_preview(true);
                }
                None => {
                    g.set_d_has_preview(false);
                    g.set_d_preview_note(format!("Кадр недоступен: {}", err.unwrap_or_else(|| "пустой ответ".into())).into());
                }
            }
        })))
    });
}
