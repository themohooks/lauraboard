//! Channel avatars: fetched from the server when a channel's avatar
//! version changes, shown in the app bar, the channel manager and the
//! notifications; chosen / removed in the channel form.

use std::collections::{HashMap, HashSet};

use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};

use super::Ctx;
use crate::net::Msg;
use crate::shell::{App, Chan};

/// A decoded avatar: version and picture.
#[derive(Clone)]
pub struct Avatar {
    pub version: u64,
    pub image: Image,
}

#[derive(Default)]
pub struct Avatars {
    map: HashMap<String, Avatar>,
    loading: HashSet<String>,
}

/// The avatar of channel `id`, when it has one.
pub fn get(c: &Ctx, id: &str) -> Option<Avatar> {
    c.avatars.map.get(id).cloned()
}

/// Fetch avatars whose version changed; forget removed ones.
pub fn sync(c: &mut Ctx) {
    let wanted: Vec<(String, Option<u64>)> = c.model.channel_info.iter().map(|e| (e.id.clone(), e.avatar)).collect();
    c.avatars.map.retain(|id, a| wanted.iter().any(|(w, v)| w == id && *v == Some(a.version)));
    for (id, v) in wanted {
        let Some(v) = v else { continue };
        if c.avatars.map.contains_key(&id) || !c.avatars.loading.insert(id.clone()) {
            continue;
        }
        c.worker.spawn(move |api| {
            let bytes = api.get_bytes(&format!("/channels/{id}/avatar.png"));
            let decoded = bytes.ok().and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgba8());
            Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
                c.avatars.loading.remove(&id);
                if let Some(img) = decoded {
                    let (w, h) = img.dimensions();
                    let raw = img.into_raw();
                    let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&raw, w, h));
                    c.avatars.map.insert(id, Avatar { version: v, image });
                }
                show(c);
            })))
        });
    }
    show(c);
}

/// Put the avatars into the window (app bar, channel list, form).
pub fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let current = c.model.channel.clone().and_then(|ch| get(c, &ch));
    let g = ui.global::<App>();
    g.set_has_channel_avatar(current.is_some());
    if let Some(a) = current {
        g.set_channel_avatar(a.image);
    }
    let form = super::channels::form_source();
    let f = get(c, &form);
    let ch = ui.global::<Chan>();
    ch.set_f_has_avatar(f.is_some());
    if let Some(a) = f {
        ch.set_f_avatar(a.image);
    }
    super::channels::refresh(c);
}

/// Choose a picture for the channel of the open form and upload it.
pub fn pick(c: &mut Ctx) {
    let id = super::channels::form_source();
    if id.is_empty() {
        return;
    }
    let Some(path) =
        crate::file_dialog::FileDialog::new().set_title("Аватар канала").add_filter("Изображения", &["png", "jpg", "jpeg", "webp"]).pick_file()
    else {
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return c.toast(format!("{}: {e}", path.display()), true),
    };
    c.worker.spawn(move |api| {
        let r = api.put_bytes(&format!("/channels/{id}/avatar"), "application/octet-stream", &bytes);
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| match r {
            Ok(_) => {
                c.toast("Аватар канала обновлён", false);
                c.load_channels();
            }
            Err(e) => c.toast(format!("Аватар не сохранён: {e}"), true),
        })))
    });
}

/// Remove the avatar of the channel of the open form.
pub fn remove(c: &mut Ctx) {
    let id = super::channels::form_source();
    if id.is_empty() {
        return;
    }
    c.worker.spawn(move |api| {
        let r = api.delete(&format!("/channels/{id}/avatar"));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| match r {
            Ok(_) => {
                c.toast("Аватар канала убран", false);
                c.load_channels();
            }
            Err(e) => c.toast(format!("Аватар не убран: {e}"), true),
        })))
    });
}
