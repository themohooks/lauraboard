//! Library folders: the tree beside the media list, selecting a folder
//! (filters the list), new / rename / delete, and the folder list used by
//! selects (import, edit sheet, "move to folder").

use serde_json::json;
use slint::{ComponentHandle, SharedString};

use lb_core::media::Library;
use lb_core::media::folders::normalize;

use super::{Ctx, vec_model, with};
use crate::net::Msg;
use crate::shell::{AppWindow, FolderRow, Lib};

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Lib>();
    g.on_select_folder(|path| {
        with(move |c| {
            if let Some(ui) = c.window() {
                ui.global::<Lib>().set_folder(path);
            }
            super::library::refresh(c);
        })
    });
    g.on_folder_save(|| with(save));
    g.on_folder_delete(|| with(delete));
}

/// Fetch the explicit folders (empty ones are only known to the server).
pub fn load(c: &Ctx) {
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        let list: Vec<String> = api.get(&format!("/channels/{ch}/media/folders"))?;
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            c.lib.folders = list;
            super::library::refresh(c);
        })))
    });
}

/// Every folder (explicit and used by items), sorted with parents first.
pub fn all(c: &Ctx) -> Vec<String> {
    let mut lib = Library::new(c.model.library.clone());
    lib.folders = c.lib.folders.clone();
    lib.all_folders()
}

/// Folder path behind an index of `folder-options` (0 = top level).
pub fn option_path(c: &Ctx, index: i32) -> String {
    usize::try_from(index - 1).ok().and_then(|i| all(c).get(i).cloned()).unwrap_or_default()
}

/// Index of `folder` in `folder-options`.
pub fn option_index(c: &Ctx, folder: &str) -> i32 {
    all(c).iter().position(|f| f == folder).map(|i| i as i32 + 1).unwrap_or(0)
}

/// Push the tree and the select options.
pub fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    let folders = all(c);
    let count = |f: &str| {
        c.model.library.iter().filter(|m| m.folder == f || m.folder.strip_prefix(f).is_some_and(|r| r.starts_with('/'))).count() as i32
    };
    let rows: Vec<FolderRow> = folders
        .iter()
        .map(|f| FolderRow {
            path: f.as_str().into(),
            name: f.rsplit('/').next().unwrap_or(f).into(),
            depth: f.matches('/').count() as i32,
            count: count(f),
        })
        .collect();
    g.set_folders(vec_model(rows));
    g.set_items_total(c.model.library.len() as i32);
    let mut options = vec![SharedString::from("Без папки (верхний уровень)")];
    options.extend(folders.iter().map(|f| SharedString::from(f.as_str())));
    g.set_folder_options(vec_model(options));
    // the selected folder may have been renamed or deleted elsewhere
    let cur = g.get_folder().to_string();
    if !cur.is_empty() && !folders.contains(&cur) {
        g.set_folder(SharedString::new());
    }
    // new imports go to the folder being looked at
    if !g.get_import_open() {
        g.set_import_folder(option_index(c, &g.get_folder()));
    }
}

fn save(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Lib>();
    let name = normalize(&g.get_f_name());
    if name.is_empty() {
        g.set_f_error("Введите имя".into());
        return;
    }
    let cur = g.get_folder().to_string();
    let (body, target) = if g.get_folder_dialog() == "new" {
        let path = if cur.is_empty() { name } else { format!("{cur}/{name}") };
        (json!({ "op": "create", "path": path }), path)
    } else {
        (json!({ "op": "rename", "path": cur, "to": name }), name)
    };
    g.set_f_error(SharedString::new());
    g.set_folder_dialog(SharedString::new());
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        let list: Vec<String> = serde_json::from_value(api.post(&format!("/channels/{ch}/media/folders"), &body)?).unwrap_or_default();
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            c.lib.folders = list;
            if let Some(ui) = c.window() {
                ui.global::<Lib>().set_folder(target.into());
            }
            c.load_all();
        })))
    });
}

fn delete(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let cur = ui.global::<Lib>().get_folder().to_string();
    if cur.is_empty() {
        return;
    }
    c.ask(
        &format!("Удалить папку «{cur}»?"),
        "Ролики и вложенные папки перейдут на уровень выше. Файлы и элементы медиатеки не удаляются.",
        "Удалить папку",
        move |c| {
            let ch = c.model.ch();
            let parent = cur.rsplit_once('/').map(|(a, _)| a.to_string()).unwrap_or_default();
            c.worker.spawn(move |api| {
                let body = json!({ "op": "delete", "path": cur });
                let list: Vec<String> =
                    serde_json::from_value(api.post(&format!("/channels/{ch}/media/folders"), &body)?).unwrap_or_default();
                Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
                    c.lib.folders = list;
                    if let Some(ui) = c.window() {
                        ui.global::<Lib>().set_folder(parent.into());
                    }
                    c.load_all();
                })))
            });
        },
    );
}
