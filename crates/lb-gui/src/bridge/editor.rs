//! Video editor: hidden areas, trim, crop and sound of a library video.
//! Frames with the areas drawn come from the server; the result is a
//! render job (new item, or the channel's copy replaced).

use std::rc::Rc;

use serde_json::json;
use slint::{Color, ComponentHandle, Image, Model, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use lb_core::media::display::Crop;
use lb_core::media::edit::{EditSpec, Region, RegionMode};
use lb_core::media::{MediaItem, MediaKind};

use super::{Ctx, with};
use crate::net::Msg;
use crate::net::poller::decode_jpeg;
use crate::shell::{AppWindow, EditRegion, Ved};

const MODES: [RegionMode; 4] = [RegionMode::Blur, RegionMode::Pixelate, RegionMode::Fill, RegionMode::Delogo];

#[derive(Default)]
pub struct EditorState {
    media: Option<String>,
    duration: i64,
    regions: Vec<Region>,
    /// Rows shown on the picture (updated in place while dragging).
    rows: Option<Rc<VecModel<EditRegion>>>,
    /// The selected area alone (a new model per selection).
    current: Option<Rc<VecModel<EditRegion>>>,
    selected: Option<usize>,
    loading: bool,
    load_again: bool,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Ved>();
    g.on_select(|i| with(move |c| select(c, usize::try_from(i).ok())));
    g.on_seek(|_| with(frame));
    g.on_add_region(|x, y, w, h| with(move |c| add(c, x, y, w, h)));
    g.on_region_move(|i, x, y, w, h| {
        with(move |c| {
            edit(c, i, |r| {
                (r.x, r.y, r.w, r.h) = (x, y, w, h);
            })
        })
    });
    g.on_region_set(|i, key, v| with(move |c| set(c, i, &key, v)));
    g.on_region_color(|i, col| with(move |c| edit(c, i, |r| r.color = hex(col))));
    g.on_remove_region(|i| with(move |c| remove(c, i)));
    g.on_regions_changed(|| with(frame));
    g.on_reset(|| {
        with(|c| {
            if let Some(m) = c.ed.media.clone().and_then(|id| item(c, &id)) {
                fill(c, &m);
            }
        })
    });
    g.on_render(|| with(render));
}

fn item(c: &Ctx, id: &str) -> Option<MediaItem> {
    c.model.library.iter().find(|m| m.id == id).cloned()
}

/// Open the editor for a library item.
pub fn open(c: &mut Ctx, id: &str) {
    let Some(m) = item(c, id) else { return };
    if m.is_url() || m.is_scene() || matches!(m.kind, MediaKind::Image | MediaKind::Stream) || m.probe.video.is_none() {
        c.toast("Монтаж доступен только для видеофайлов медиатеки", true);
        return;
    }
    fill(c, &m);
    if let Some(ui) = c.window() {
        ui.global::<Ved>().set_open(true);
    }
}

/// Start from the item as it is (also "Сбросить").
fn fill(c: &mut Ctx, m: &MediaItem) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Ved>();
    let dur = m.duration.ms().max(0);
    let v = m.probe.video.clone().unwrap_or_default();
    c.ed.media = Some(m.id.clone());
    c.ed.duration = dur;
    c.ed.regions.clear();
    let rows = Rc::new(VecModel::<EditRegion>::default());
    g.set_regions(rows.clone().into());
    c.ed.rows = Some(rows);
    select(c, None);
    g.set_media_title(m.title.as_str().into());
    g.set_duration_ms(dur as f32);
    g.set_aspect(if v.width > 0 && v.height > 0 { v.width as f32 / v.height as f32 } else { 16.0 / 9.0 });
    // the item's in / out points are a good first guess for the trim
    let start = m.in_point.map(|d| d.ms()).unwrap_or(0).clamp(0, dur);
    let end = m.out_point.map(|d| d.ms()).filter(|e| *e > start).unwrap_or(dur).min(dur);
    g.set_start_ms(start as f32);
    g.set_end_ms(end as f32);
    g.set_audio_start_ms(start as f32);
    g.set_audio_end_ms(end as f32);
    g.set_has_audio(m.probe.has_audio);
    g.set_audio_track(false);
    g.set_audio_fade_in(0.0);
    g.set_audio_fade_out(0.0);
    g.set_position_ms((start + (end - start) / 10) as f32);
    g.set_crop_left(0.0);
    g.set_crop_top(0.0);
    g.set_crop_right(0.0);
    g.set_crop_bottom(0.0);
    g.set_mute(false);
    g.set_volume_db(0.0);
    g.set_fade_in(0.0);
    g.set_fade_out(0.0);
    let own = m.path.replace('\\', "/").starts_with(&format!("channels/{}/media/", c.model.ch()));
    g.set_can_replace(own);
    g.set_replace(false);
    g.set_new_title(format!("{} (ред.)", m.title).into());
    g.set_has_frame(false);
    g.set_frame_error(SharedString::new());
    frame(c);
    timeline_media(c, m.probe.has_audio);
}

/// Fetch source thumbnails independently of edited preview frames.
fn timeline_media(c: &mut Ctx, has_audio: bool) {
    let Some(ui) = c.window() else { return };
    let Some(media) = c.ed.media.clone() else { return };
    let g = ui.global::<Ved>();
    g.set_thumbnails(Rc::new(VecModel::<Image>::default()).into());
    g.set_has_waveform(false);
    g.set_audio_status(if has_audio { "Загрузка звуковой волны…" } else { "Нет звуковой дорожки" }.into());
    let ch = c.model.ch();
    let duration = c.ed.duration;
    let api = c.worker.api().patient(40);
    for i in 0..8 {
        let (api, media, ch) = (api.clone(), media.clone(), ch.clone());
        c.worker.spawn(move |_| {
            let at = duration * i / 8;
            let img = api
                .post_bytes(&format!("/channels/{ch}/media/{media}/edit/frame.jpg?at={at}&w=160"), &EditSpec::default())
                .ok()
                .and_then(|b| decode_jpeg(&b));
            Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
                if c.ed.media.as_deref() != Some(media.as_str()) {
                    return;
                }
                let Some(ui) = c.window() else { return };
                let g = ui.global::<Ved>();
                let mut images: Vec<Image> = g.get_thumbnails().iter().collect();
                images.resize(8, Image::default());
                if let Some(f) = img {
                    images[i as usize] = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&f.rgba, f.width, f.height));
                }
                g.set_thumbnails(Rc::new(VecModel::from(images)).into());
            })))
        });
    }
    if !has_audio {
        return;
    }
    c.worker.spawn(move |_| {
        let img = api
            .get_bytes(&format!("/channels/{ch}/media/{media}/edit/waveform.png"))
            .ok()
            .and_then(|b| image::load_from_memory(&b).ok())
            .map(|img| {
                let rgba = img.to_rgba8();
                crate::net::Frame { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() }
            });
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if c.ed.media.as_deref() != Some(media.as_str()) {
                return;
            }
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Ved>();
            if let Some(f) = img {
                g.set_waveform(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&f.rgba, f.width, f.height)));
                g.set_has_waveform(true);
            } else {
                g.set_audio_status("Звуковая волна недоступна".into());
            }
        })))
    });
}

fn row(r: &Region, dur: i64) -> EditRegion {
    EditRegion {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        mode: MODES.iter().position(|m| *m == r.mode).unwrap_or(0) as i32,
        strength: r.strength,
        color: parse_hex(&r.color),
        timed: r.from_ms.is_some() || r.to_ms.is_some(),
        from_ms: r.from_ms.unwrap_or(0) as f32,
        to_ms: r.to_ms.unwrap_or(dur) as f32,
    }
}

fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue())
}

fn parse_hex(s: &str) -> Color {
    let h = s.trim_start_matches('#');
    let b = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok()).unwrap_or(0);
    Color::from_rgb_u8(b(0), b(2), b(4))
}

fn select(c: &mut Ctx, i: Option<usize>) {
    let i = i.filter(|i| *i < c.ed.regions.len());
    c.ed.selected = i;
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Ved>();
    g.set_selected(i.map(|i| i as i32).unwrap_or(-1));
    let cur = Rc::new(VecModel::from(i.map(|i| vec![row(&c.ed.regions[i], c.ed.duration)]).unwrap_or_default()));
    g.set_current(cur.clone().into());
    c.ed.current = Some(cur);
}

fn add(c: &mut Ctx, x: f32, y: f32, w: f32, h: f32) {
    if c.ed.regions.len() >= 16 {
        c.toast("Не больше 16 областей", true);
        return;
    }
    let r = Region { x, y, w, h, mode: RegionMode::Blur, strength: 0.5, color: "#000000".into(), from_ms: None, to_ms: None };
    if let Some(rows) = &c.ed.rows {
        rows.push(row(&r, c.ed.duration));
    }
    c.ed.regions.push(r);
    select(c, Some(c.ed.regions.len() - 1));
    frame(c);
}

/// Change area `i` and show it (picture and, when selected, its settings).
fn edit(c: &mut Ctx, i: i32, f: impl FnOnce(&mut Region)) {
    let Some(i) = usize::try_from(i).ok().filter(|i| *i < c.ed.regions.len()) else { return };
    f(&mut c.ed.regions[i]);
    let shown = row(&c.ed.regions[i], c.ed.duration);
    if let Some(rows) = &c.ed.rows {
        rows.set_row_data(i, shown.clone());
    }
    if c.ed.selected == Some(i)
        && let Some(cur) = &c.ed.current
        && cur.row_count() == 1
    {
        cur.set_row_data(0, shown);
    }
}

fn set(c: &mut Ctx, i: i32, key: &str, v: f32) {
    let dur = c.ed.duration;
    let pos = c.window().map(|ui| ui.global::<Ved>().get_position_ms() as i64).unwrap_or(0);
    edit(c, i, |r| match key {
        "mode" => r.mode = MODES.get(v as usize).copied().unwrap_or_default(),
        "strength" => r.strength = (v * 100.0).round() / 100.0,
        "timed" if v > 0.5 => {
            // from the current frame to the end, then adjust
            r.from_ms = Some(pos.clamp(0, dur));
            r.to_ms = Some(dur);
        }
        "timed" => (r.from_ms, r.to_ms) = (None, None),
        "from" => {
            let t = v as i64;
            r.from_ms = Some(t);
            r.to_ms = Some(r.to_ms.unwrap_or(dur).max(t));
        }
        "to" => {
            let t = v as i64;
            r.to_ms = Some(t);
            r.from_ms = Some(r.from_ms.unwrap_or(0).min(t));
        }
        _ => {}
    });
}

fn remove(c: &mut Ctx, i: i32) {
    let Some(i) = usize::try_from(i).ok().filter(|i| *i < c.ed.regions.len()) else { return };
    c.ed.regions.remove(i);
    if let Some(rows) = &c.ed.rows {
        rows.remove(i);
    }
    select(c, None);
    frame(c);
}

/// Everything set in the dialog.
fn spec(c: &Ctx) -> Option<EditSpec> {
    let ui = c.window()?;
    let g = ui.global::<Ved>();
    let ms = |v: f32| v.round() as i64;
    let end = ms(g.get_end_ms());
    Some(EditSpec {
        start_ms: ms(g.get_start_ms()),
        end_ms: (end < c.ed.duration).then_some(end),
        audio_start_ms: (ms(g.get_audio_start_ms()) != ms(g.get_start_ms())).then(|| ms(g.get_audio_start_ms())),
        audio_end_ms: (ms(g.get_audio_end_ms()) != end).then(|| ms(g.get_audio_end_ms())),
        audio_fade_in_ms: (g.get_audio_fade_in() != g.get_fade_in()).then(|| ms(g.get_audio_fade_in() * 1000.0)),
        audio_fade_out_ms: (g.get_audio_fade_out() != g.get_fade_out()).then(|| ms(g.get_audio_fade_out() * 1000.0)),
        regions: c.ed.regions.clone(),
        crop: Crop { left: g.get_crop_left(), top: g.get_crop_top(), right: g.get_crop_right(), bottom: g.get_crop_bottom() },
        mute: g.get_mute(),
        volume_db: (g.get_volume_db() * 10.0).round() / 10.0,
        fade_in_ms: ms(g.get_fade_in() * 1000.0),
        fade_out_ms: ms(g.get_fade_out() * 1000.0),
    })
}

/// The frame under the cursor with the areas drawn; requests made while one
/// is running coalesce into one more.
fn frame(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let (Some(media), Some(spec)) = (c.ed.media.clone(), spec(c)) else { return };
    if c.ed.loading {
        c.ed.load_again = true;
        return;
    }
    c.ed.loading = true;
    let g = ui.global::<Ved>();
    g.set_loading_frame(true);
    let at = g.get_position_ms().round() as i64;
    let ch = c.model.ch();
    let api = c.worker.api().patient(40);
    c.worker.spawn(move |_| {
        let r = api.post_bytes(&format!("/channels/{ch}/media/{media}/edit/frame.jpg?at={at}&w=960"), &spec);
        let img = r.as_ref().ok().and_then(|b| decode_jpeg(b));
        let err = r.err();
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            c.ed.loading = false;
            if c.ed.media.as_deref() != Some(media.as_str()) {
                return;
            }
            let again = std::mem::take(&mut c.ed.load_again);
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Ved>();
            g.set_loading_frame(false);
            match img.filter(|f| f.rgba.len() == (f.width * f.height * 4) as usize) {
                Some(f) => {
                    g.set_frame(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&f.rgba, f.width, f.height)));
                    g.set_has_frame(true);
                    g.set_frame_error(SharedString::new());
                }
                None => {
                    g.set_has_frame(false);
                    let e = err.map(|e| crate::ru::server(&e)).unwrap_or_else(|| "пустой ответ".into());
                    g.set_frame_error(format!("Кадр недоступен: {e}").into());
                }
            }
            if again {
                frame(c);
            }
        })))
    });
}

fn render(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Ved>();
    let (Some(media), Some(spec)) = (c.ed.media.clone(), spec(c)) else { return };
    let (s, e) = spec.range(c.ed.duration);
    if e - s < 500 {
        c.toast("Оставьте хотя бы полсекунды ролика", true);
        return;
    }
    if spec == (EditSpec { end_ms: spec.end_ms, ..EditSpec::default() }) && spec.end_ms.is_none() {
        c.toast("Ничего не изменено", true);
        return;
    }
    let replace = g.get_replace();
    let title = g.get_new_title().trim().to_string();
    let body = json!({ "spec": spec, "replace": replace, "title": if replace || title.is_empty() { None } else { Some(title) } });
    if replace {
        c.ask(
            "Заменить исходный файл?",
            "Обработанный ролик займёт место исходного: он останется в плейлистах и расписании, а старый файл будет удалён без возможности вернуть.",
            "Заменить",
            move |c| start(c, media, body),
        );
    } else {
        start(c, media, body);
    }
}

fn start(c: &mut Ctx, media: String, body: serde_json::Value) {
    let Some(ui) = c.window() else { return };
    ui.global::<Ved>().set_starting(true);
    let ch = c.model.ch();
    let api = c.worker.api().patient(30);
    c.worker.spawn(move |_| {
        let r = api.post(&format!("/channels/{ch}/media/{media}/edit"), &body);
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Ved>();
            g.set_starting(false);
            match r {
                Ok(r) => {
                    super::jobs::started(c, r["job"].as_u64().unwrap_or_default());
                    g.set_open(false);
                    c.toast("Обработка видео началась — прогресс в шапке окна", false);
                }
                Err(e) => c.toast(format!("Монтаж: {}", crate::ru::server(&e)), true),
            }
        })))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_round_trip() {
        assert_eq!(hex(parse_hex("#1a2b3c")), "#1a2b3c");
        assert_eq!(hex(parse_hex("oops")), "#000000");
    }

    #[test]
    fn rows_show_the_time_window() {
        let r = Region {
            x: 0.1,
            y: 0.2,
            w: 0.3,
            h: 0.4,
            mode: RegionMode::Fill,
            strength: 0.5,
            color: "#ffffff".into(),
            from_ms: Some(2_000),
            to_ms: None,
        };
        let row = row(&r, 10_000);
        assert!(row.timed);
        assert_eq!((row.from_ms, row.to_ms, row.mode), (2_000.0, 10_000.0, 2));
    }
}
