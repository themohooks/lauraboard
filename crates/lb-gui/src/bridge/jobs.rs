//! Background jobs of the server: progress in the app bar and a list with
//! cancel / dismiss; finished jobs trigger their follow-up (reload the
//! library after copying media, report where an export was written).

use std::collections::HashSet;

use slint::ComponentHandle;

use super::{Ctx, vec_model, with};
use crate::model::JobView;
use crate::net::Msg;
use crate::shell::{AppWindow, JobRow, Jobs};

#[derive(Default)]
pub struct JobsState {
    /// Jobs seen running (their completion is reported once).
    running: HashSet<u64>,
    /// Jobs started from this window (only those get follow-ups / toasts).
    mine: HashSet<u64>,
}

pub fn connect(ui: &AppWindow) {
    ui.global::<Jobs>().on_cancel(|id| {
        with(move |c| {
            c.worker.spawn(move |api| {
                api.delete(&format!("/jobs/{id}"))?;
                Ok(Msg::Apply(Box::new(|_| {})))
            });
        })
    });
}

/// Remember a job started here (its completion gets a follow-up).
pub fn started(c: &mut Ctx, job: u64) {
    c.jobs.mine.insert(job);
    c.jobs.running.insert(job);
}

fn state_text(j: &JobView) -> String {
    match j.state.as_str() {
        "running" if j.message.is_empty() => format!("{:.0} %", j.progress * 100.0),
        "running" => format!("{:.0} % · {}", j.progress * 100.0, j.message),
        "done" => "Готово".into(),
        "cancelled" => "Отменено".into(),
        _ => format!("Ошибка: {}", crate::ru::server(&j.message)),
    }
}

pub fn changed(c: &mut Ctx, list: Vec<JobView>) {
    let ended: Vec<&JobView> = list.iter().filter(|j| j.state != "running" && c.jobs.running.contains(&j.id)).collect();
    for j in ended {
        c.jobs.running.remove(&j.id);
        if c.jobs.mine.remove(&j.id) {
            finished(c, j);
        }
    }
    for j in list.iter().filter(|j| j.state == "running") {
        c.jobs.running.insert(j.id);
    }
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Jobs>();
    let running: Vec<&JobView> = list.iter().filter(|j| j.state == "running").collect();
    let progress = if running.is_empty() { 1.0 } else { running.iter().map(|j| j.progress).sum::<f32>() / running.len() as f32 };
    g.set_running(running.len() as i32);
    g.set_progress(progress);
    g.set_summary(
        match running.as_slice() {
            [] => String::new(),
            [one] => format!("{:.0} % · {}", one.progress * 100.0, one.title),
            many => format!("{:.0} % · задач: {}", progress * 100.0, many.len()),
        }
        .into(),
    );
    let rows: Vec<JobRow> = list
        .iter()
        .rev()
        .map(|j| JobRow {
            id: j.id as i32,
            title: j.title.as_str().into(),
            channel: j.channel.clone().unwrap_or_default().into(),
            state: j.state.as_str().into(),
            text: state_text(j).into(),
            progress: j.progress,
        })
        .collect();
    g.set_rows(vec_model(rows));
}

/// Follow-up of a job started from this window.
fn finished(c: &mut Ctx, j: &JobView) {
    let result = j.result.clone().unwrap_or_default();
    match (j.state.as_str(), j.kind.as_str()) {
        ("done", "import_media") => {
            let added = result["added"].as_array().map(|a| a.len()).unwrap_or(0);
            let errors = result["errors"].as_array().map(|a| a.len()).unwrap_or(0);
            let text = if errors > 0 {
                format!("Скопировано в медиатеку: {added}, ошибок: {errors}")
            } else {
                format!("Скопировано в медиатеку: {added}")
            };
            c.toast(text, errors > 0);
            if j.channel.as_deref() == c.model.channel.as_deref() {
                c.load_all();
            }
        }
        ("done", "download_media") => {
            let added = result["added"].as_array().map(|a| a.len()).unwrap_or(0);
            let errors: Vec<String> = result["errors"]
                .as_array()
                .map(|a| a.iter().filter_map(|e| e.as_str().map(crate::ru::server)).collect())
                .unwrap_or_default();
            let text = match (added, errors.len()) {
                (n, 0) => format!("Скачано в медиатеку: {n}"),
                (n, e) => format!("Скачано: {n}, не получилось {e}: {}", errors.join("; ")),
            };
            c.toast(text, !errors.is_empty());
            if j.channel.as_deref() == c.model.channel.as_deref() {
                c.load_all();
            }
        }
        ("done", "edit_media") => {
            let text = if result["replaced"].as_bool() == Some(true) {
                "Монтаж готов: файл ролика заменён".to_string()
            } else {
                "Монтаж готов: новый ролик добавлен в медиатеку".to_string()
            };
            c.toast(text, false);
            if j.channel.as_deref() == c.model.channel.as_deref() {
                c.load_all();
            }
        }
        ("failed", "download_media") if super::download::needs_login(&j.message) => {
            c.toast_with("Сайт просит войти в аккаунт, чтобы скачать это видео", "Войти", super::download::open_login);
        }
        (state, "install_ytdlp") => {
            if state != "done" {
                c.toast(format!("Установка yt-dlp: {}", crate::ru::server(&j.message)), true);
            }
            super::download::tool_installed(c, state == "done");
        }
        ("done", "backgrounds") => {
            let added = result["added"].as_array().map(|a| a.len()).unwrap_or(0);
            let errors = result["errors"].as_array().map(|a| a.len()).unwrap_or(0);
            let text = match (added, errors) {
                (0, 0) => "Готовые фоны уже есть в медиатеке (папка «Фоны»)".to_string(),
                (n, 0) => format!("Готовые фоны добавлены в медиатеку: {n} (папка «Фоны»)"),
                (n, e) => format!("Готовые фоны: добавлено {n}, не получилось {e}"),
            };
            c.toast(text, errors > 0);
            if j.channel.as_deref() == c.model.channel.as_deref() {
                c.load_all();
            }
        }
        ("done", "export_channel") => {
            let path = result["path"].as_str().unwrap_or_default().to_string();
            c.toast(format!("Экспорт готов: {path}"), false);
        }
        ("done", "import_channel") => {
            let id = result["channel"].as_str().unwrap_or_default().to_string();
            c.toast(format!("Канал «{id}» импортирован"), false);
            c.model.channel = Some(id);
            c.load_channels();
        }
        ("done", _) => c.toast(format!("{}: готово", j.title), false),
        ("cancelled", _) => {
            c.toast(format!("{}: отменено", j.title), false);
            if j.kind == "import_media" || j.kind == "download_media" {
                c.load_all(); // files finished before the cancel stay
            }
        }
        _ => c.toast(format!("{}: {}", j.title, crate::ru::server(&j.message)), true),
    }
}
