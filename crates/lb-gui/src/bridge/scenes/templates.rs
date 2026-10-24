//! New scenes from templates. "Broadcast clock" and "up next" come with
//! graphics of their own (big clock and date, next programme), stored in
//! the scene — the channel graphics are not touched; "music break" takes
//! a background video and music from the library folders if there are
//! any.

use serde_json::{Value, json};

use lb_core::Dur;
use lb_core::graphics::{Element, GraphicRule};
use lb_core::media::scene::{Background, MusicOrder, MusicRule};
use lb_core::media::{Library, MediaItem, MediaKind, Scene};

use super::{Ctx, FOLDER, add_item};

/// Channel frame height (sizes are designed for 1080 lines).
fn scale(c: &Ctx) -> f32 {
    let h = c.channel_config.as_ref().and_then(|v| v["video"]["height"].as_f64()).unwrap_or(1080.0);
    (h / 1080.0) as f32
}

fn element(v: Value) -> Option<Element> {
    serde_json::from_value(v).ok()
}

/// Look of a template text: size (for 1080 lines), bold, colour.
struct Look(f32, bool, &'static str);

/// Text / clock element centred on the frame, `dy` below the centre.
fn centred(id: &str, name: &str, kind: Value, look: Look, dy: f32, k: f32) -> Option<Element> {
    let Look(size, bold, color) = look;
    let mut v = kind;
    v["id"] = json!(id);
    v["name"] = json!(name);
    v["style"] = json!({ "size": (size * k).round(), "bold": bold, "color": color, "shadow_color": "#00000080", "shadow_offset": [2.0 * k, 2.0 * k], "padding": 0.0 });
    v["placement"] = json!({ "anchor": "center", "x": 0.0, "y": (dy * k).round() });
    v["layer"] = json!(40);
    v["animation"] = json!({ "in": "fade_up", "out": "fade", "duration": "00:00:00.600" });
    element(v)
}

/// A rule showing `e` for the whole scene.
fn rule_for(e: &Element) -> Option<GraphicRule> {
    serde_json::from_value(json!({ "id": e.id, "name": e.name, "element": e.id, "priority": 10 })).ok()
}

/// First enabled item of `folder` (or below) matching `ok`.
fn first_in<'a>(lib: &'a Library, folder: &str, ok: impl Fn(&MediaItem) -> bool) -> Option<&'a MediaItem> {
    lib.items.iter().find(|m| m.enabled && !m.is_scene() && (m.folder == folder || m.folder.starts_with(&format!("{folder}/"))) && ok(m))
}

pub fn create(c: &mut Ctx, template: &str) {
    let lib = Library::new(c.model.library.clone());
    let k = scale(c);
    let (base, title, kind, dur) = match template {
        "clock" => ("clock", "Эфирные часы", MediaKind::Filler, Dur::secs(60)),
        "music" => ("music-break", "Музыкальная пауза", MediaKind::Filler, Dur::mins(5)),
        "next" => ("up-next", "Скоро в эфире", MediaKind::Promo, Dur::secs(20)),
        _ => ("scene", "Новая сцена", MediaKind::Filler, Dur::secs(60)),
    };
    let id = lib.unique_id(&format!("scene-{base}"));
    let mut scene = Scene::default();
    let mut els: Vec<Element> = vec![];
    match template {
        "clock" => {
            scene.background = Background::Color { color: "#0B1A3A".into() };
            if let Some(bg) = first_in(&lib, "Фоны", |m| m.kind == MediaKind::Image) {
                scene.background = Background::Media { media: bg.id.clone() };
            }
            els.extend(centred("time", "Часы сцены", json!({ "type": "clock", "format": "%H:%M:%S" }), Look(220.0, true, "#FFFFFF"), -40.0, k));
            els.extend(centred("date", "Дата сцены", json!({ "type": "clock", "format": "%d.%m.%Y" }), Look(56.0, false, "#C7D2FE"), 120.0, k));
        }
        "music" => {
            scene.background = match first_in(&lib, "Фоны", |m| m.kind != MediaKind::Image && m.probe.video.is_some()) {
                Some(v) => Background::Media { media: v.id.clone() },
                None => Background::Color { color: "#1A1033".into() },
            };
            let folder = ["Музыка", "Music"].into_iter().find(|f| first_in(&lib, f, |_| true).is_some());
            scene.music.push(MusicRule {
                name: "Музыка".into(),
                folder: folder.map(str::to_string),
                category: folder.is_none().then(|| "music".to_string()),
                order: MusicOrder::Shuffle,
                ..Default::default()
            });
            els.extend(centred("title", "Подпись сцены", json!({ "type": "text", "text": "{channel}" }), Look(72.0, true, "#FFFFFF"), 0.0, k));
        }
        "next" => {
            scene.background = Background::Color { color: "#111827".into() };
            els.extend(centred("label", "Далее в эфире", json!({ "type": "text", "text": "Далее в эфире" }), Look(48.0, false, "#FBBF24"), -130.0, k));
            els.extend(centred("program", "Следующая передача", json!({ "type": "text", "text": "{next_program}" }), Look(96.0, true, "#FFFFFF"), 0.0, k));
            els.extend(centred("time", "Время передачи", json!({ "type": "text", "text": "в {next_program_start}" }), Look(56.0, false, "#E5E7EB"), 120.0, k));
        }
        _ => {}
    }
    scene.graphics.rules = els.iter().filter_map(rule_for).collect();
    scene.graphics.elements = els;
    let mut item = MediaItem::scene(id, kind, dur, scene);
    item.title = title.into();
    item.folder = FOLDER.into();
    add_item(c, item, format!("Сцена «{title}» создана — она в папке «{FOLDER}» медиатеки"));
}
