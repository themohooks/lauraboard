//! Event log screen: live channel events (newest first) or the tail of the
//! server log file, filtered by level and text; pause, clear, export.

use chrono::{DateTime, Local, Utc};
use slint::{ComponentHandle, SharedString};

use super::{Ctx, vec_model, with};
use crate::model::LogLine;
use crate::net::Msg;
use crate::shell::{AppWindow, Log, LogRow};

const SERVER_LINES: usize = 1000;

#[derive(Default)]
pub struct LogState {
    /// Server log file lines, newest first.
    server: Vec<LogLine>,
    /// Lines currently shown (for export).
    shown: Vec<LogLine>,
    /// Events that arrived while paused.
    unseen: usize,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Log>();
    g.on_refresh_list(|| with(refresh));
    g.on_pause(|on| {
        with(move |c| {
            c.log.unseen = 0;
            if let Some(ui) = c.window() {
                ui.global::<Log>().set_paused(on);
            }
            refresh(c);
        })
    });
    g.on_clear(|| {
        with(|c| {
            c.model.log.clear();
            c.log.unseen = 0;
            refresh(c);
            super::air::log_changed(c);
        })
    });
    g.on_reload(|| with(reload));
    g.on_export(|| with(export));
}

pub fn opened(c: &mut Ctx) {
    reload(c);
}

/// Server log on its tab; the live list just redraws.
fn reload(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    if ui.global::<Log>().get_tab() != 1 {
        refresh(c);
        return;
    }
    ui.global::<Log>().set_loading(true);
    c.worker.spawn(|api| {
        let text = api.get_bytes(&format!("/logs?lines={SERVER_LINES}")).map(|b| String::from_utf8_lossy(&b).into_owned());
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            match text {
                Ok(t) => c.log.server = t.lines().rev().filter(|l| !l.trim().is_empty()).map(parse_server_line).collect(),
                Err(e) => {
                    c.log.server.clear();
                    c.toast(format!("Журнал сервера недоступен: {e}"), true);
                }
            }
            if let Some(ui) = c.window() {
                ui.global::<Log>().set_loading(false);
            }
            refresh(c);
        })))
    });
}

/// A live event arrived.
pub fn changed(c: &mut Ctx) {
    if c.section() != "log" {
        return;
    }
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Log>();
    if g.get_tab() != 0 {
        return;
    }
    if g.get_paused() {
        c.log.unseen += 1;
        g.set_unseen(c.log.unseen as i32);
        g.set_total(c.model.log.len() as i32);
        return;
    }
    refresh(c);
}

fn level_class(level: &str) -> u8 {
    match level {
        "critical" | "error" => 2,
        "warning" => 1,
        _ => 0,
    }
}

pub fn refresh(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Log>();
    let show = [g.get_show_info(), g.get_show_warning(), g.get_show_critical()];
    let q = g.get_query().trim().to_lowercase();
    let source: Vec<LogLine> = if g.get_tab() == 1 { c.log.server.clone() } else { c.model.log.iter().cloned().collect() };
    let total = source.len();
    c.log.shown = source
        .into_iter()
        .filter(|l| show[level_class(&l.level) as usize])
        .filter(|l| q.is_empty() || l.text.to_lowercase().contains(&q))
        .collect();
    let rows: Vec<LogRow> = c
        .log
        .shown
        .iter()
        .map(|l| LogRow { at: SharedString::from(l.at.as_str()), level: SharedString::from(l.level.as_str()), text: SharedString::from(l.text.as_str()) })
        .collect();
    g.set_rows(vec_model(rows));
    g.set_total(total as i32);
    g.set_unseen(c.log.unseen as i32);
}

/// `2026-10-21T18:18:41.384324Z  INFO target: message fields` → local time,
/// level, `target: message`.
fn parse_server_line(line: &str) -> LogLine {
    let mut parts = line.splitn(2, ' ');
    let stamp = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim_start();
    let Ok(t) = stamp.parse::<DateTime<Utc>>() else {
        return LogLine { at: String::new(), level: "info".into(), text: line.to_string() };
    };
    let (lvl, text) = rest.split_once(' ').unwrap_or((rest, ""));
    let level = match lvl {
        "ERROR" => "critical",
        "WARN" => "warning",
        _ => "info",
    };
    LogLine { at: t.with_timezone(&Local).format("%H:%M:%S").to_string(), level: level.into(), text: text.trim_start().to_string() }
}

fn export(c: &mut Ctx) {
    if c.log.shown.is_empty() {
        return;
    }
    let server = c.window().is_some_and(|ui| ui.global::<Log>().get_tab() == 1);
    let name = format!("{}-{}.txt", if server { "server-log" } else { "events" }, Local::now().format("%Y%m%d-%H%M"));
    let Some(path) = rfd::FileDialog::new().set_file_name(name).add_filter("Текст", &["txt", "log"]).save_file() else { return };
    // oldest first in the file, like a regular log
    let text: String = c.log.shown.iter().rev().map(|l| format!("{}\t{}\t{}\n", l.at, l.level, l.text)).collect();
    match std::fs::write(&path, text) {
        Ok(()) => c.toast(format!("Сохранено: {}", path.display()), false),
        Err(e) => c.toast(format!("{}: {e}", path.display()), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_lines_are_split_into_time_level_text() {
        let l = parse_server_line("2026-10-21T18:17:12.061856Z  WARN ffmpeg: packet corrupt process=encoder");
        assert_eq!(l.level, "warning");
        assert_eq!(l.text, "ffmpeg: packet corrupt process=encoder");
        assert_eq!(l.at.len(), 8);
        let l = parse_server_line("2026-10-21T18:18:41.384324Z  INFO lb_engine: on air");
        assert_eq!((l.level.as_str(), l.text.as_str()), ("info", "lb_engine: on air"));
        let l = parse_server_line("garbage line");
        assert_eq!((l.at.as_str(), l.text.as_str()), ("", "garbage line"));
    }
}
