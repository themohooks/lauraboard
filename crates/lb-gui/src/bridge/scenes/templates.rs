//! New scenes from templates. "Broadcast clock" and "up next" also add
//! their graphics (big clock and date, next programme) with rules limited
//! to the new scene; "music break" takes a background video and music
//! from the library folders if there are any.

use serde_json::{Value, json};

use lb_core::Dur;
use lb_core::graphics::{Element, GraphicsConfig};
use lb_core::media::scene::{Background, MusicOrder, MusicRule};
use lb_core::media::{Library, MediaItem, MediaKind, Scene};

use super::{Ctx, FOLDER, add_item, gfx};

/// Graphics of a template (each gets a rule limited to the scene).
struct Extra {
    elements: Vec<Element>,
}

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

fn unique_element_id(cfg: &GraphicsConfig, base: &str) -> String {
    let mut id = base.to_string();
    let mut n = 2;
    while cfg.elements.iter().any(|e| e.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
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
    let cfg = &c.model.graphics;
    let mut extra = Extra { elements: vec![] };
    match template {
        "clock" => {
            scene.background = Background::Color { color: "#0B1A3A".into() };
            if let Some(bg) = first_in(&lib, "Фоны", |m| m.kind == MediaKind::Image) {
                scene.background = Background::Media { media: bg.id.clone() };
            }
            let time = unique_element_id(cfg, &format!("{id}-time"));
            let date = unique_element_id(cfg, &format!("{id}-date"));
            extra.elements.extend(centred(&time, "Часы сцены", json!({ "type": "clock", "format": "%H:%M:%S" }), Look(220.0, true, "#FFFFFF"), -40.0, k));
            extra.elements.extend(centred(&date, "Дата сцены", json!({ "type": "clock", "format": "%d.%m.%Y" }), Look(56.0, false, "#C7D2FE"), 120.0, k));
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
            let t = unique_element_id(cfg, &format!("{id}-title"));
            extra.elements.extend(centred(&t, "Подпись сцены", json!({ "type": "text", "text": "{channel}" }), Look(72.0, true, "#FFFFFF"), 0.0, k));
        }
        "next" => {
            scene.background = Background::Color { color: "#111827".into() };
            let a = unique_element_id(cfg, &format!("{id}-label"));
            let b = unique_element_id(cfg, &format!("{id}-program"));
            let t = unique_element_id(cfg, &format!("{id}-time"));
            extra.elements.extend(centred(&a, "Далее в эфире", json!({ "type": "text", "text": "Далее в эфире" }), Look(48.0, false, "#FBBF24"), -130.0, k));
            extra.elements.extend(centred(&b, "Следующая передача", json!({ "type": "text", "text": "{next_program}" }), Look(96.0, true, "#FFFFFF"), 0.0, k));
            extra.elements.extend(centred(&t, "Время передачи", json!({ "type": "text", "text": "в {next_program_start}" }), Look(56.0, false, "#E5E7EB"), 120.0, k));
        }
        _ => {}
    }
    let mut item = MediaItem::scene(id.clone(), kind, dur, scene);
    item.title = title.into();
    item.folder = FOLDER.into();
    if !extra.elements.is_empty() {
        let mut cfg = c.model.graphics.clone();
        for e in extra.elements {
            let eid = e.id.clone();
            cfg.elements.push(e);
            if let Some(r) = gfx::rule_for(&cfg, &eid, &id, title) {
                cfg.rules.push(r);
            }
        }
        c.model.graphics = cfg.clone();
        c.save_doc("/graphics", cfg, "Графика сцены добавлена");
    }
    add_item(c, item, format!("Сцена «{title}» создана — она в папке «{FOLDER}» медиатеки"));
}
