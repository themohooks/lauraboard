//! Font picker of the text style: server fonts and the channel's font
//! files, search, adding a font file (copied into the channel) and
//! downloading a Google Fonts family (by the server, into the channel).

use serde_json::{Value, json};
use slint::ComponentHandle;

use super::{props_changed, s, with};
use crate::bridge::{Ctx, vec_model};
use crate::net::Msg;
use crate::shell::{AppWindow, FontRow, GFontRow, Gfx};

/// Fonts known for the current channel (fetched when the picker opens).
#[derive(Default)]
pub struct FontList {
    system: Vec<String>,
    files: Vec<String>,
    /// Families downloaded into the channel (Google Fonts).
    families: Vec<String>,
    /// Latest Google Fonts search (older answers are dropped).
    gf_seq: u64,
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
    g.on_gf_search(|| with(gf_search));
    g.on_gf_download(|f| with(move |c| gf_download(c, f.to_string())));
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
        let (system, files, families): (Vec<String>, Vec<String>, Vec<String>) = (list("system"), list("files"), list("families"));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let gf_seq = c.gfx.fonts.gf_seq;
            c.gfx.fonts = FontList { system, files, families, gf_seq };
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
    let row = |value: &str, name: String, file: bool, note: &str| FontRow { value: s(value), name: s(name), file, note: s(note) };
    let mut rows: Vec<FontRow> = c.gfx.fonts.families.iter().filter(|f| hit(f)).map(|f| row(f, f.clone(), false, "Google Fonts")).collect();
    rows.extend(c.gfx.fonts.files.iter().filter(|p| hit(&file_label(p))).map(|p| row(p, file_label(p), true, "файл канала")));
    rows.extend(c.gfx.fonts.system.iter().filter(|f| hit(f) && !c.gfx.fonts.families.contains(f)).map(|f| row(f, f.clone(), false, "")));
    g.set_font_list(vec_model(rows));
}

fn upload(c: &mut Ctx) {
    if !c.server_is_local() {
        c.toast("Файл шрифта можно добавить только на компьютере с сервером вещания", true);
        return;
    }
    let Some(path) = rfd::FileDialog::new().set_title("Файл шрифта").add_filter("Шрифты", &["ttf", "otf", "ttc"]).pick_file()
    else {
        return;
    };
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

/// Russian name of a Google Fonts category.
fn category(c: &str) -> &str {
    match c {
        "Sans Serif" => "Без засечек",
        "Serif" => "С засечками",
        "Display" => "Декоративный",
        "Handwriting" => "Рукописный",
        "Monospace" => "Моноширинный",
        other => other,
    }
}

fn gf_search(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    g.set_gf_loading(true);
    g.set_gf_error(s(""));
    c.gfx.fonts.gf_seq += 1;
    let seq = c.gfx.fonts.gf_seq;
    let q: String = g.get_gf_query().trim().chars().filter(|ch| ch.is_alphanumeric() || *ch == ' ').collect();
    let path = format!("/fonts/google?q={}&cyrillic={}", q.replace(' ', "%20"), g.get_gf_cyrillic());
    c.worker.spawn(move |api| {
        let r = api.patient(90).get::<Value>(&path);
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if seq != c.gfx.fonts.gf_seq {
                return;
            }
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Gfx>();
            g.set_gf_loading(false);
            match r {
                Ok(v) => {
                    let rows: Vec<GFontRow> = v["families"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|f| {
                                    let family = f["family"].as_str()?.to_string();
                                    let mut info = vec![category(f["category"].as_str().unwrap_or_default()).to_string()];
                                    if f["cyrillic"].as_bool() == Some(true) {
                                        info.push("кириллица".into());
                                    }
                                    let styles: Vec<&str> =
                                        [(f["bold"].as_bool() == Some(true), "жирный"), (f["italic"].as_bool() == Some(true), "курсив")]
                                            .into_iter()
                                            .filter_map(|(on, n)| on.then_some(n))
                                            .collect();
                                    if !styles.is_empty() {
                                        info.push(styles.join(", "));
                                    }
                                    let downloaded = c.gfx.fonts.families.iter().any(|d| d.eq_ignore_ascii_case(&family));
                                    Some(GFontRow { family: s(family), info: s(info.join(" · ")), downloaded })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if rows.is_empty() {
                        g.set_gf_error(s("Ничего не найдено"));
                    }
                    g.set_gf_list(vec_model(rows));
                }
                Err(e) => {
                    g.set_gf_list(vec_model(Vec::new()));
                    g.set_gf_error(s(format!("Не удалось получить список Google Fonts: {e}. Серверу вещания нужен доступ в интернет.")));
                }
            }
        })))
    });
}

fn gf_download(c: &mut Ctx, family: String) {
    let Some(ui) = c.window() else { return };
    ui.global::<Gfx>().set_gf_busy(s(family.clone()));
    let ch = c.model.ch();
    let body = json!({ "family": family });
    c.worker.spawn(move |api| {
        let r = api.patient(180).post(&format!("/channels/{ch}/fonts/google"), &body);
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Gfx>();
            g.set_gf_busy(s(""));
            match r {
                Ok(_) => {
                    if !c.gfx.fonts.families.contains(&family) {
                        c.gfx.fonts.families.push(family.clone());
                    }
                    g.set_s_font(s(family.clone()));
                    g.set_font_open(false);
                    c.toast(format!("Шрифт «{family}» скачан в канал и выбран"), false);
                    props_changed(c);
                    load(c);
                }
                Err(e) => c.toast(format!("Шрифт «{family}» не скачан: {}", crate::ru::server(&e)), true),
            }
        })))
    });
}
