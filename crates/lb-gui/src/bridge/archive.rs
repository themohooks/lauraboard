//! Channel export / import dialogs: parts, archive path, what an archive
//! holds; the work itself runs as a server job ([`super::jobs`]).

use serde_json::{Value, json};
use slint::{ComponentHandle, SharedString};

use super::channels::{entry, form_source, set_form_source, validate};
use super::{Ctx, with};
use crate::net::Msg;
use crate::shell::{AppWindow, Chan};

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Chan>();
    g.on_export(|id| with(move |c| open_export(c, id.as_str())));
    g.on_browse_export(|| with(browse_export));
    g.on_start_export(|| with(start_export));
    g.on_open_import(|| with(open_import));
    g.on_browse_import(|| with(browse_import));
    g.on_inspect_import(|| with(inspect));
    g.on_start_import(|| with(start_import));
}

fn default_dir() -> std::path::PathBuf {
    dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| ".".into())
}

fn open_export(c: &mut Ctx, id: &str) {
    let Some(ui) = c.window() else { return };
    let Some(e) = entry(c, id) else { return };
    let g = ui.global::<Chan>();
    set_form_source(&e.id);
    g.set_form_source(e.label().into());
    let file = format!("{}-{}.zip", e.id, chrono::Local::now().format("%Y-%m-%d"));
    g.set_x_path(default_dir().join(file).to_string_lossy().replace('\\', "/").into());
    g.set_form("export".into());
}

fn browse_export(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Chan>();
    let current = std::path::PathBuf::from(g.get_x_path().as_str());
    let mut d = crate::file_dialog::FileDialog::new().set_title("Куда сохранить архив канала").add_filter("Архив ZIP", &["zip"]);
    if let Some(dir) = current.parent().filter(|p| p.is_dir()) {
        d = d.set_directory(dir);
    }
    if let Some(name) = current.file_name() {
        d = d.set_file_name(name.to_string_lossy());
    }
    if let Some(mut p) = d.save_file() {
        if p.extension().is_none() {
            p.set_extension("zip");
        }
        g.set_x_path(p.to_string_lossy().replace('\\', "/").into());
    }
}

fn start_export(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Chan>();
    let id = form_source();
    let body = json!({
        "path": g.get_x_path().trim(),
        "parts": {
            "settings": g.get_x_settings(),
            "outputs": g.get_x_settings() && g.get_x_outputs(),
            "library": g.get_x_library(),
            "media": g.get_x_media(),
            "playlists": g.get_x_playlists(),
            "schedules": g.get_x_schedules(),
            "rules": g.get_x_rules(),
            "graphics": g.get_x_graphics(),
            "history": g.get_x_history(),
        }
    });
    g.set_form(SharedString::new());
    c.worker.spawn(move |api| {
        let r = api.post(&format!("/channels/{id}/export"), &body)?;
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if let Some(job) = r["job"].as_u64() {
                super::jobs::started(c, job);
            }
            c.toast("Экспорт начался — прогресс в шапке окна", false);
        })))
    });
}

fn open_import(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Chan>();
    g.set_i_path(SharedString::new());
    g.set_i_info(SharedString::new());
    g.set_i_valid(false);
    g.set_e_id(SharedString::new());
    g.set_e_name(SharedString::new());
    g.set_e_error(SharedString::new());
    set_form_source("");
    g.set_form("import".into());
    if c.server_is_local() {
        browse_import(c);
    }
}

fn browse_import(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let picked = crate::file_dialog::FileDialog::new()
        .set_title("Архив канала")
        .add_filter("Архив ZIP", &["zip"])
        .set_directory(default_dir())
        .pick_file();
    if let Some(p) = picked {
        ui.global::<Chan>().set_i_path(p.to_string_lossy().replace('\\', "/").into());
        inspect(c);
    }
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} ГБ", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} МБ", b as f64 / (1u64 << 20) as f64),
        b => format!("{} КБ", b.div_ceil(1024)),
    }
}

/// Human summary of an archive manifest.
fn describe(m: &Value) -> String {
    let p = &m["parts"];
    let on = |k: &str| p[k].as_bool().unwrap_or(false);
    let names = [
        ("settings", "настройки"),
        ("outputs", "выходы"),
        ("library", "медиатека"),
        ("playlists", "плейлисты"),
        ("schedules", "сетка"),
        ("rules", "правила"),
        ("graphics", "графика"),
        ("history", "журнал эфира"),
    ];
    let parts: Vec<&str> = names.iter().filter(|(k, _)| on(k)).map(|(_, n)| *n).collect();
    let media = if on("media") {
        format!("медиафайлов: {} ({})", m["media_files"].as_u64().unwrap_or(0), size_text(m["media_bytes"].as_u64().unwrap_or(0)))
    } else {
        "без медиафайлов".into()
    };
    format!(
        "Канал «{}» ({}), экспортирован {}.\nВ архиве: {}; {media}.",
        m["name"].as_str().unwrap_or_default(),
        m["channel"].as_str().unwrap_or_default(),
        m["exported_at"].as_str().unwrap_or("—"),
        parts.join(", "),
    )
}

fn inspect(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let path = ui.global::<Chan>().get_i_path().trim().to_string();
    if path.is_empty() {
        return;
    }
    c.worker.spawn(move |api| {
        let r = api.post("/archives/inspect", &json!({ "path": path }));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Chan>();
            match r {
                Ok(m) => {
                    g.set_i_info(describe(&m).into());
                    g.set_i_valid(true);
                    // keep the archive's id unless it is taken here
                    let base = m["channel"].as_str().unwrap_or("channel").to_string();
                    let mut id = base.clone();
                    let mut n = 2;
                    while c.model.channels.contains(&id) {
                        id = format!("{base}-{n}");
                        n += 1;
                    }
                    g.set_e_id(id.as_str().into());
                    g.set_e_name(m["name"].as_str().unwrap_or_default().into());
                    validate(c, &id);
                }
                Err(e) => {
                    g.set_i_info(crate::ru::server(&e).into());
                    g.set_i_valid(false);
                }
            }
        })))
    });
}

fn start_import(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Chan>();
    let id = g.get_e_id().trim().to_string();
    validate(c, &id);
    if !g.get_e_error().is_empty() {
        return;
    }
    let body = json!({ "path": g.get_i_path().trim(), "id": id, "name": g.get_e_name().trim() });
    g.set_form(SharedString::new());
    c.worker.spawn(move |api| {
        let r = api.post("/archives/import", &body)?;
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if let Some(job) = r["job"].as_u64() {
                super::jobs::started(c, job);
            }
            c.toast("Импорт начался — прогресс в шапке окна", false);
        })))
    });
}
