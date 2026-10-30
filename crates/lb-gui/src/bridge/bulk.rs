//! Bulk actions on the checked items of the media library. Every change
//! is confirmed first; selection helpers act at once.

use serde_json::json;

use lb_core::media::MediaItem;
use lb_core::playlist::PlaylistEntry;

use super::library::{delete_ids, item, update_checks};
use super::{Ctx, folders, vec_model};
use crate::fmt::KINDS;
use crate::net::Msg;
use crate::shell::MenuEntry;

/// Names of [`KINDS`] in the same order.
const KIND_NAMES: [&str; 7] = ["Передача", "Заставка", "Реклама", "Анонс", "Заполнение", "Изображение", "Поток"];

/// Checked ids in display order.
fn checked_ids(c: &Ctx) -> Vec<String> {
    c.lib.shown.iter().map(|&i| c.model.library[i].id.clone()).filter(|id| c.lib.checked.contains(id)).collect()
}

/// "3 элемента" with the right word form.
fn items_text(n: usize) -> String {
    let word = match (n % 10, n % 100) {
        (1, r) if r != 11 => "элемент",
        (2..=4, r) if !(12..=14).contains(&r) => "элемента",
        _ => "элементов",
    };
    format!("{n} {word}")
}

/// Change the checked items with `edit` and save them, after a confirmation.
fn change_items(c: &mut Ctx, title: &str, text: String, button: &str, done: &'static str, edit: impl Fn(&mut MediaItem) + 'static) {
    let ids = checked_ids(c);
    c.confirm(title, &text, button, move |c| {
        let ch = c.model.ch();
        let items: Vec<MediaItem> = ids
            .iter()
            .filter_map(|id| item(c, id))
            .map(|mut m| {
                edit(&mut m);
                m
            })
            .collect();
        c.change(move |api| {
            for m in &items {
                api.put(&format!("/channels/{ch}/media/{}", m.id), m)?;
            }
            Ok(format!("{done}: {}", items.len()))
        });
    });
}

pub fn bulk(c: &mut Ctx, action: &str, arg: i32) {
    match action {
        "select-shown" => {
            c.lib.checked = c.lib.shown.iter().map(|&i| c.model.library[i].id.clone()).collect();
            return update_checks(c);
        }
        "invert" => {
            let shown: Vec<String> = c.lib.shown.iter().map(|&i| c.model.library[i].id.clone()).collect();
            for id in shown {
                if !c.lib.checked.remove(&id) {
                    c.lib.checked.insert(id);
                }
            }
            return update_checks(c);
        }
        _ => {}
    }
    let ids = checked_ids(c);
    if ids.is_empty() {
        return;
    }
    let n = ids.len();
    let what = items_text(n);
    let ch = c.model.ch();
    match action {
        "playlist" => {
            let Some(mut p) = c.model.playlists.get(arg as usize).cloned() else { return };
            let name = if p.name.is_empty() { p.id.clone() } else { p.name.clone() };
            c.confirm(
                "Добавить в плейлист?",
                &format!("В конец плейлиста «{name}» будет добавлено: {what}."),
                "Добавить",
                move |c| {
                    p.entries.extend(ids.into_iter().map(PlaylistEntry::new));
                    c.change(move |api| {
                        api.put(&format!("/channels/{ch}/playlists/{}", p.id), &p)?;
                        Ok(format!("Добавлено в «{name}»: {n}"))
                    });
                    c.lib.checked.clear();
                    update_checks(c);
                },
            );
        }
        "open-playlist" => {
            if !c.pl.is_open() {
                return c.toast("Сначала откройте плейлист в разделе «Плейлисты»", true);
            }
            c.confirm(
                "Добавить в открытый плейлист?",
                &format!("В конец открытого плейлиста будет добавлено: {what}. Сохраните плейлист в разделе «Плейлисты»."),
                "Добавить",
                move |c| {
                    for id in &ids {
                        super::playlists::append(c, id);
                    }
                    c.toast(format!("Добавлено в открытый плейлист: {n}"), false);
                },
            );
        }
        "queue" => c.confirm(
            "Добавить в очередь эфира?",
            &format!("В очередь эфира будет добавлено: {what}, в порядке списка."),
            "В очередь",
            move |c| {
                c.worker.spawn(move |api| {
                    for id in &ids {
                        api.post(&format!("/channels/{ch}/command"), &json!({ "cmd": "queue", "media": id }))?;
                    }
                    Ok(Msg::Done(format!("В очереди: {}", ids.len())))
                });
            },
        ),
        "enable" | "disable" => {
            let on = action == "enable";
            change_items(
                c,
                if on { "Включить выбранные?" } else { "Выключить выбранные?" },
                if on {
                    format!("Будет включено: {what}. Они снова попадут в эфир.")
                } else {
                    format!("Будет выключено: {what}. Выключенные ролики не попадают в эфир.")
                },
                if on { "Включить" } else { "Выключить" },
                if on { "Включено" } else { "Выключено" },
                move |m| m.enabled = on,
            );
        }
        "folder" => {
            let path = folders::option_path(c, arg);
            let label = if path.is_empty() { "верхний уровень".to_string() } else { format!("«{path}»") };
            c.confirm("Переместить в папку?", &format!("Будет перемещено в {label}: {what}."), "Переместить", move |c| {
                let body = json!({ "op": "move", "path": path, "ids": ids });
                c.change(move |api| {
                    api.post(&format!("/channels/{ch}/media/folders"), &body)?;
                    Ok(format!("Перемещено в {label}: {n}"))
                });
                c.lib.checked.clear();
                update_checks(c);
            });
        }
        "kind" => {
            let Some((k, name)) = KINDS.get(arg as usize).copied().zip(KIND_NAMES.get(arg as usize)) else { return };
            change_items(
                c,
                "Сменить тип?",
                format!("Тип «{name}» будет задан для: {what}. От типа зависят правила вставок и статистика."),
                "Сменить",
                "Тип изменён",
                move |m| m.kind = k,
            );
        }
        "reset-trim" => change_items(
            c,
            "Сбросить обрезку?",
            format!("Точки начала и конца будут сброшены для: {what}. Ролики пойдут в эфир целиком."),
            "Сбросить",
            "Обрезка сброшена",
            |m| {
                m.in_point = None;
                m.out_point = None;
            },
        ),
        "reset-volume" => change_items(
            c,
            "Сбросить громкость?",
            format!("Громкость 0 дБ будет задана для: {what}."),
            "Сбросить",
            "Громкость сброшена",
            |m| m.volume_db = 0.0,
        ),
        "delete" => c.ask(
            "Удалить выбранные ролики?",
            &format!("Из медиатеки будет удалено: {what}. Копии в хранилище канала удаляются, остальные файлы на диске останутся."),
            "Удалить",
            move |c| delete_ids(c, ids),
        ),
        _ => {}
    }
}

/// Bulk "Сменить категорию": `cat` is a category name, "" removes it.
pub fn set_category(c: &mut Ctx, cat: String) {
    if checked_ids(c).is_empty() {
        return;
    }
    let what = items_text(checked_ids(c).len());
    let text = if cat.is_empty() {
        format!("Категория будет убрана у: {what}.")
    } else {
        format!("Категория «{cat}» будет задана для: {what}.")
    };
    change_items(c, "Сменить категорию?", text, "Сменить", "Категория изменена", move |m| {
        m.category = Some(cat.clone()).filter(|c| !c.is_empty());
    });
}

/// Entries of the bulk category menu.
pub fn category_entries(c: &Ctx) -> slint::ModelRc<MenuEntry> {
    let mut v: Vec<MenuEntry> =
        c.lib.categories.iter().map(|k| MenuEntry { id: k.into(), text: k.into(), enabled: true, ..Default::default() }).collect();
    v.push(MenuEntry { id: "".into(), text: "Без категории".into(), enabled: true, divider: !v.is_empty(), ..Default::default() });
    vec_model(v)
}

#[cfg(test)]
mod tests {
    use super::items_text;

    #[test]
    fn word_forms() {
        assert_eq!(items_text(1), "1 элемент");
        assert_eq!(items_text(3), "3 элемента");
        assert_eq!(items_text(11), "11 элементов");
        assert_eq!(items_text(22), "22 элемента");
        assert_eq!(items_text(25), "25 элементов");
    }
}
