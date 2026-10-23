//! Font picker of the text style: server fonts and the channel's font
//! files, search, and adding a font file (copied into the channel).

use serde_json::{Value, json};
use slint::ComponentHandle;

use super::{props_changed, s, with};
use crate::bridge::{Ctx, vec_model};
use crate::net::Msg;
use crate::shell::{AppWindow, FontRow, Gfx};

/// Fonts known for the current channel (fetched when the picker opens).
#[derive(Default)]
pub struct FontList {
    system: Vec<String>,
    files: Vec<String>,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Gfx>();
    g.on_open_fonts(|| with(open));
    g.on_font_search(|_| with(show));
    g.on_font_pick(|v| {
        with(move |c| {
            if let Some(ui) = c.window() {
                let g = ui.global::<Gfx>();
                g.set_s_font(v);
                g.set_font_open(false);
            }
            props_changed(c);
        })
    });
    g.on_font_upload(|| with(upload));
}

fn open(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    g.set_font_query(s(""));
    g.set_font_open(true);
    g.set_font_loading(c.gfx.fonts.system.is_empty());
    show(c);
    load(c);
}

fn load(c: &mut Ctx) {
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        let v: Value = api.get(&format!("/channels/{ch}/fonts"))?;
        let list = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let (system, files): (Vec<String>, Vec<String>) = (list("system"), list("files"));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            c.gfx.fonts = FontList { system, files };
            if let Some(ui) = c.window() {
                ui.global::<Gfx>().set_font_loading(false);
            }
            show(c);
        })))
    });
}

/// File name of a stored font path, for the list.
fn file_label(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    let q = g.get_font_query().trim().to_lowercase();
    let hit = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
    let mut rows: Vec<FontRow> =
        c.gfx.fonts.files.iter().filter(|p| hit(&file_label(p))).map(|p| FontRow { value: s(p.clone()), name: s(file_label(p)), file: true }).collect();
    rows.extend(c.gfx.fonts.system.iter().filter(|f| hit(f)).map(|f| FontRow { value: s(f.clone()), name: s(f.clone()), file: false }));
    g.set_font_list(vec_model(rows));
}

fn upload(c: &mut Ctx) {
    if !c.server_is_local() {
        c.toast("Файл шрифта можно добавить только на компьютере с сервером вещания", true);
        return;
    }
    let Some(path) = rfd::FileDialog::new().set_title("Файл шрифта").add_filter("Шрифты", &["ttf", "otf", "ttc"]).pick_file() else { return };
    let ch = c.model.ch();
    let body = json!({ "path": path.to_string_lossy() });
    c.worker.spawn(move |api| {
        let r = api.post(&format!("/channels/{ch}/fonts"), &body)?;
        let stored = r["path"].as_str().unwrap_or_default().to_string();
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if let Some(ui) = c.window() {
                let g = ui.global::<Gfx>();
                g.set_s_font(s(stored.clone()));
                g.set_font_open(false);
            }
            c.toast(format!("Шрифт добавлен в канал: {}", file_label(&stored)), false);
            props_changed(c);
            load(c);
        })))
    });
}
