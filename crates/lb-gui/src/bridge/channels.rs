//! Channel manager: list with state and autostart, switch to a channel,
//! rename / change id, duplicate, delete.

use serde_json::{Value, json};
use slint::ComponentHandle;

use super::{Ctx, vec_model, with};
use crate::model::ChannelEntry;
use crate::net::Msg;
use crate::shell::{AppWindow, Chan, ChannelRow};

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Chan>();
    g.on_opened(|| with(reload_list));
    g.on_select(|id| {
        with(move |c| {
            let id = id.to_string();
            if c.model.channel.as_deref() != Some(&id) {
                c.model.channel = Some(id.clone());
                c.channel_selected(&id);
                super::shell::channels_changed(c);
                c.load_all();
            }
            refresh(c);
        })
    });
    g.on_rename(|id| with(move |c| open_form(c, "rename", id.as_str())));
    g.on_duplicate(|id| with(move |c| open_form(c, "duplicate", id.as_str())));
    g.on_validate_id(|id| with(move |c| validate(c, id.as_str())));
    g.on_save_form(|| with(save_form));
    g.on_autostart(|id, on| with(move |c| set_autostart(c, id.to_string(), on)));
    g.on_remove(|id| with(move |c| remove(c, id.to_string())));
    g.on_avatar_pick(|| with(super::avatars::pick));
    g.on_avatar_remove(|| with(super::avatars::remove));
    super::archive::connect(ui);
}

/// Fetch the channel list without reloading the documents of the channel.
fn reload_list(c: &mut Ctx) {
    c.worker.spawn(|api| {
        let list: Vec<Value> = api.get("/channels")?;
        let info: Vec<ChannelEntry> = list.iter().filter_map(ChannelEntry::from_json).collect();
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if info.iter().map(|e| &e.id).eq(c.model.channels.iter()) {
                c.model.channel_info = info;
                super::shell::channels_changed(c);
                refresh(c);
            } else {
                c.load_channels(); // channels added or removed elsewhere
            }
        })))
    });
}

fn state_text(state: &str) -> &'static str {
    match state {
        "on_air" => "В эфире",
        "starting" => "Запуск",
        "reconnecting" => "Переподключение",
        "error" => "Ошибка",
        _ => "Остановлен",
    }
}

/// Push the list to the window.
pub fn refresh(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let rows: Vec<ChannelRow> = c
        .model
        .channel_info
        .iter()
        .map(|e| ChannelRow {
            id: e.id.as_str().into(),
            name: e.label().into(),
            initial: e.initial().into(),
            avatar: super::avatars::get(c, &e.id).map(|a| a.image).unwrap_or_default(),
            has_avatar: super::avatars::get(c, &e.id).is_some(),
            state_text: state_text(&e.state).into(),
            on_air: e.state != "stopped",
            autostart: e.autostart,
            current: c.model.channel.as_deref() == Some(e.id.as_str()),
        })
        .collect();
    let g = ui.global::<Chan>();
    g.set_rows(vec_model(rows));
    g.set_can_browse(c.server_is_local());
}

pub fn entry(c: &Ctx, id: &str) -> Option<ChannelEntry> {
    c.model.channel_info.iter().find(|e| e.id == id).cloned()
}

fn open_form(c: &mut Ctx, form: &str, id: &str) {
    let Some(ui) = c.window() else { return };
    let Some(e) = entry(c, id) else { return };
    let g = ui.global::<Chan>();
    g.set_form_source(e.label().into());
    g.set_form_on_air(e.state != "stopped");
    if form == "duplicate" {
        let mut n = 2;
        while c.model.channels.iter().any(|x| *x == format!("{}-{n}", e.id)) {
            n += 1;
        }
        g.set_e_id(format!("{}-{n}", e.id).into());
        g.set_e_name(format!("{} (копия)", e.label()).into());
    } else {
        g.set_e_id(e.id.as_str().into());
        g.set_e_name(e.name.as_str().into());
    }
    g.set_e_error("".into());
    set_form_source(&e.id);
    g.set_f_initial(e.initial().into());
    g.set_form(form.into());
    super::avatars::show(c);
}

/// Channel the open form acts on.
pub fn form_source() -> String {
    FORM_SOURCE.with(|s| s.borrow().clone())
}

pub fn set_form_source(id: &str) {
    FORM_SOURCE.with(|s| *s.borrow_mut() = id.to_string());
}

thread_local! {
    /// Id of the channel the open form acts on.
    static FORM_SOURCE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

pub fn validate(c: &mut Ctx, id: &str) {
    let Some(ui) = c.window() else { return };
    let id = id.trim();
    let source = FORM_SOURCE.with(|s| s.borrow().clone());
    let renaming_self = ui.global::<Chan>().get_form() == "rename" && id == source;
    let err = if id.is_empty() {
        "Укажите идентификатор"
    } else if !id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
        "Допустимы латиница, цифры, «-» и «_»"
    } else if !renaming_self && c.model.channels.iter().any(|x| x == id) {
        "Канал с таким идентификатором уже есть"
    } else {
        ""
    };
    ui.global::<Chan>().set_e_error(err.into());
}

fn save_form(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Chan>();
    let id = g.get_e_id().trim().to_string();
    validate(c, &id);
    if !g.get_e_error().is_empty() {
        return;
    }
    let name = g.get_e_name().trim().to_string();
    let source = FORM_SOURCE.with(|s| s.borrow().clone());
    let dup = g.get_form() == "duplicate";
    g.set_form("".into());
    c.toast(if dup { "Канал копируется…" } else { "Сохранение…" }, false);
    c.worker.spawn(move |api| {
        let target = if dup {
            api.post(&format!("/channels/{source}/duplicate"), &json!({ "id": id, "name": name }))?;
            id.clone()
        } else {
            let new_id = if id == source { String::new() } else { id.clone() };
            api.put(&format!("/channels/{source}"), &json!({ "name": name, "id": new_id }))?;
            id.clone()
        };
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            c.toast(if dup { format!("Копия «{target}» создана") } else { "Канал сохранён".to_string() }, false);
            // follow a renamed current channel to its new id
            if !dup && c.model.channel.as_deref() == Some(source.as_str()) {
                c.model.channel = Some(target);
            }
            c.load_channels();
        })))
    });
}

fn set_autostart(c: &mut Ctx, id: String, on: bool) {
    if let Some(e) = c.model.channel_info.iter_mut().find(|e| e.id == id) {
        e.autostart = on;
    }
    c.worker.spawn(move |api| {
        api.put(&format!("/channels/{id}"), &json!({ "autostart": on }))?;
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            c.toast(
                if on {
                    "Канал будет выходить в эфир при запуске программы"
                } else {
                    "Автозапуск выключен"
                },
                false,
            );
        })))
    });
}

fn remove(c: &mut Ctx, id: String) {
    let Some(e) = entry(c, &id) else { return };
    let warn_air = if e.state != "stopped" {
        " Канал сейчас в эфире — вещание будет остановлено."
    } else {
        ""
    };
    let text = format!(
        "Канал исчезнет из программы.{warn_air} Его папка со всеми настройками и собственными файлами будет перенесена в папку «deleted» проекта — оттуда её можно вернуть вручную."
    );
    c.ask_with_option(
        &format!("Удалить канал «{}»?", e.label()),
        &text,
        "Удалить",
        "Стереть файлы безвозвратно",
        move |c, purge| {
            c.worker.spawn(move |api| {
                api.delete(&format!("/channels/{id}?purge={purge}"))?;
                Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
                    c.toast(format!("Канал «{id}» удалён"), false);
                    c.load_channels();
                })))
            });
        },
    );
}
