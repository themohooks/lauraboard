//! Notifications from channel events and status: encoder down /
//! restored, items skipped, broadcast points unreachable, low disk space,
//! air started / stopped, new programmes, lost connection. Which ones are
//! shown is set in the preferences ([`NotifyPrefs`]); the same notice is
//! not repeated within a minute.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde_json::Value;
use slint::ComponentHandle;

use super::Ctx;
use crate::notify::{Notice, Urgency, show};
use crate::prefs::NotifyPrefs;

const REPEAT_AFTER: Duration = Duration::from_secs(60);
/// Warn when the recording disk has less free space than this.
const LOW_DISK: u64 = 5 * 1024 * 1024 * 1024;

#[derive(Default)]
pub struct NotifyState {
    sent: HashMap<String, Instant>,
    encoder_down: bool,
    problems: HashSet<String>,
    disk_low: bool,
    /// `None` until the first connection result (no notice at start-up).
    online: Option<bool>,
}

#[derive(Clone, Copy)]
enum Topic {
    Errors,
    Skipped,
    Outputs,
    Disk,
    Air,
    Programs,
}

fn wanted(p: &NotifyPrefs, t: Topic) -> bool {
    p.enabled
        && match t {
            Topic::Errors => p.errors,
            Topic::Skipped => p.skipped,
            Topic::Outputs => p.outputs,
            Topic::Disk => p.disk,
            Topic::Air => p.air,
            Topic::Programs => p.programs,
        }
}

/// Send unless filtered out, repeated too soon, or the window is in use
/// while only hidden-window notices are wanted.
/// `channel`: the channel the notice is about (its avatar is shown).
fn send(c: &mut Ctx, channel: Option<&str>, topic: Topic, key: &str, title: &str, body: &str, urgency: Urgency) {
    let p = &c.prefs.notify;
    if !wanted(p, topic) {
        return;
    }
    if p.only_hidden
        && let Some(ui) = c.window()
        && ui.window().is_visible()
        && !ui.window().is_minimized()
    {
        return;
    }
    if c.notify.sent.get(key).is_some_and(|t| t.elapsed() < REPEAT_AFTER) {
        return;
    }
    c.notify.sent.insert(key.to_string(), Instant::now());
    popup(c, channel, title, body, urgency);
}

fn popup(c: &Ctx, channel: Option<&str>, title: &str, body: &str, urgency: Urgency) {
    let avatar = channel.and_then(|id| super::avatars::get(c, id)).map(|a| a.image);
    let initial = channel.and_then(|id| super::channels::entry(c, id)).map(|e| e.initial()).unwrap_or_else(|| "L".into());
    show(Notice { title: title.into(), body: body.into(), urgency, avatar, initial });
}

fn channel_name(c: &Ctx, id: &str) -> String {
    super::channels::entry(c, id).map(|ch| ch.label().to_string()).unwrap_or_else(|| id.to_string())
}

/// A channel event from the server.
pub fn event(c: &mut Ctx, v: &Value) {
    let ch = v["channel"].as_str().unwrap_or_default().to_string();
    let name = channel_name(c, &ch);
    match v["type"].as_str().unwrap_or_default() {
        "encoder_stopped" => {
            c.notify.encoder_down = true;
            let err = v["error"].as_str().map(crate::ru::server).unwrap_or_else(|| "без описания".into());
            send(
                c,
                Some(&ch),
                Topic::Errors,
                &format!("enc-{ch}"),
                &format!("{name}: кодировщик остановлен"),
                &format!("{err}\nЭфир прерван, идёт перезапуск."),
                Urgency::Critical,
            );
        }
        "encoder_started" if c.notify.encoder_down => {
            c.notify.encoder_down = false;
            send(
                c,
                Some(&ch),
                Topic::Errors,
                &format!("enc-ok-{ch}"),
                &format!("{name}: эфир восстановлен"),
                "Кодировщик снова работает.",
                Urgency::Info,
            );
        }
        "item_ended" => {
            let r = &v["record"];
            let title = r["title"].as_str().unwrap_or_default();
            let note = r["note"].as_str().unwrap_or_default();
            if r["status"].as_str() == Some("skipped") {
                let body = if note.is_empty() {
                    format!("«{title}» не вышел в эфир.")
                } else {
                    format!("«{title}»: {}", crate::ru::server(note))
                };
                send(
                    c,
                    Some(&ch),
                    Topic::Skipped,
                    &format!("skip-{ch}-{title}"),
                    &format!("{name}: ролик пропущен"),
                    &body,
                    Urgency::Warning,
                );
            }
        }
        "item_started" => {
            let it = &v["item"];
            if it["kind"].as_str() == Some("program") {
                let title = it["title"].as_str().unwrap_or_default();
                send(c, Some(&ch), Topic::Programs, &format!("prog-{ch}-{title}"), &format!("{name}: в эфире"), title, Urgency::Info);
            }
        }
        "started" => send(c, Some(&ch), Topic::Air, &format!("air-{ch}"), &format!("{name}: эфир запущен"), "Канал вещает.", Urgency::Info),
        "stopped" => {
            send(c, Some(&ch), Topic::Air, &format!("air-{ch}"), &format!("{name}: эфир остановлен"), "Канал не вещает.", Urgency::Warning)
        }
        "mode_changed" => {
            let manual = v["manual"].as_bool().unwrap_or(false);
            let body = if manual {
                "Расписание на паузе, эфиром управляет оператор."
            } else {
                "Эфир снова идёт по расписанию."
            };
            send(
                c,
                Some(&ch),
                Topic::Air,
                &format!("mode-{ch}"),
                &format!("{name}: {}", if manual { "ручной режим" } else { "автоматический режим" }),
                body,
                Urgency::Info,
            );
        }
        "message" if v["level"].as_str() == Some("critical") => {
            let text = v["text"].as_str().unwrap_or_default();
            send(c, Some(&ch), Topic::Errors, &format!("msg-{ch}-{text}"), &name, &crate::ru::server(text), Urgency::Critical);
        }
        _ => {}
    }
}

/// New status of the current channel: unreachable outputs, disk space.
pub fn status(c: &mut Ctx) {
    let name = c.model.status.name.clone();
    let cur = c.model.channel.clone();
    let now: HashSet<String> = c.model.status.output.problems.iter().cloned().collect();
    let fresh: Vec<String> = now.difference(&c.notify.problems).cloned().collect();
    c.notify.problems = now;
    for p in fresh {
        send(
            c,
            cur.as_deref(),
            Topic::Outputs,
            &format!("out-{p}"),
            &format!("{name}: точка вещания недоступна"),
            &crate::ru::server(&p),
            Urgency::Warning,
        );
    }
    let free = c.model.status.output.recording.as_ref().and_then(|r| r.free_bytes);
    let low = free.is_some_and(|f| f < LOW_DISK);
    if low && !c.notify.disk_low {
        let body = format!(
            "Свободно {} — запись скоро остановится. Освободите место или ограничьте хранение записей.",
            crate::fmt::bytes(free.unwrap_or(0))
        );
        send(c, cur.as_deref(), Topic::Disk, "disk", &format!("{name}: мало места на диске"), &body, Urgency::Warning);
    }
    c.notify.disk_low = low;
}

/// Connection to the server changed.
pub fn connection(c: &mut Ctx, ok: bool) {
    let was = c.notify.online.replace(ok);
    match (was, ok) {
        (Some(true), false) => send(
            c,
            None,
            Topic::Errors,
            "conn",
            "LauraBoard: нет связи с сервером",
            "Интерфейс переподключается. Эфир на сервере продолжается, если сервер работает.",
            Urgency::Warning,
        ),
        (Some(false), true) => send(
            c,
            None,
            Topic::Errors,
            "conn-ok",
            "LauraBoard: связь восстановлена",
            "Интерфейс снова подключён к серверу.",
            Urgency::Info,
        ),
        _ => {}
    }
}

/// A test notice from the settings screen (bypasses filters).
pub fn test(c: &mut Ctx) {
    let ch = c.model.channel.clone();
    let name = ch.as_deref().map(|id| channel_name(c, id)).unwrap_or_else(|| "LauraBoard".into());
    popup(c, ch.as_deref(), &format!("{name}: проверка уведомлений"), "Так будут выглядеть уведомления об эфире.", Urgency::Info);
}
