//! `playlist`: playlist management (offline, on project files).

use anyhow::Result;

use lb_core::fmt_ms;
use lb_core::playlist::{Playlist, PlaylistEntry};
use lb_core::project::m3u;

use crate::cli::PlaylistCmd;
use crate::ctx::{Ctx, confirm};
use crate::exit::{Code, bail};
use crate::table::{Table, clip};

fn not_found(id: &str) -> anyhow::Error {
    bail(Code::NotFound, format!("playlist '{id}' not found"))
}

pub fn run(ctx: &Ctx, cmd: PlaylistCmd) -> Result<()> {
    let (p, mut ch) = ctx.channel_data()?;
    match cmd {
        PlaylistCmd::List => {
            if ctx.print_json(&ch.playlists)? {
                return Ok(());
            }
            let mut t = Table::new(&["id", "name", "entries", "duration", "loop", "shuffle"]);
            for pl in &ch.playlists {
                t.row(vec![
                    pl.id.clone(),
                    pl.name.clone(),
                    pl.entries.len().to_string(),
                    fmt_ms(pl.total_duration(&ch.library)),
                    pl.looped.to_string(),
                    pl.shuffle.to_string(),
                ]);
            }
            t.print();
            return Ok(());
        }
        PlaylistCmd::Show { id } => {
            let pl = ch.playlist(&id).ok_or_else(|| not_found(&id))?;
            if ctx.print_json(pl)? {
                return Ok(());
            }
            let mut t = Table::new(&["#", "media", "duration", "title", "flags"]);
            let mut offset = 0;
            for (i, e) in pl.entries.iter().enumerate() {
                let m = ch.library.get(&e.media);
                let d = m.map(|m| m.play_duration()).unwrap_or(0);
                let mut flags = vec![format!("+{}", fmt_ms(offset))];
                if e.pinned {
                    flags.push("pinned".into());
                }
                if e.disabled {
                    flags.push("disabled".into());
                }
                if m.is_none() {
                    flags.push("MISSING".into());
                }
                t.row(vec![
                    i.to_string(),
                    e.media.clone(),
                    fmt_ms(d),
                    clip(m.map(|m| m.title.as_str()).unwrap_or(""), 40),
                    flags.join(" "),
                ]);
                if !e.disabled {
                    offset += d;
                }
            }
            t.print();
            println!("total {}", fmt_ms(pl.total_duration(&ch.library)));
            return Ok(());
        }
        PlaylistCmd::Export { id, file } => {
            let pl = ch.playlist(&id).ok_or_else(|| not_found(&id))?;
            let text = m3u::export(pl, &ch.library);
            match file {
                Some(f) => std::fs::write(&f, text)?,
                None => print!("{text}"),
            }
            return Ok(());
        }
        PlaylistCmd::Create { id, name, no_loop, shuffle } => {
            if ch.playlist(&id).is_some() {
                return Err(bail(Code::Invalid, format!("playlist '{id}' already exists")));
            }
            let mut pl = Playlist::new(&id);
            pl.name = name.unwrap_or_else(|| id.clone());
            pl.looped = !no_loop;
            pl.shuffle = shuffle;
            ch.playlists.push(pl);
            println!("created playlist '{id}'");
        }
        PlaylistCmd::Add { id, media, pinned } => {
            for m in &media {
                if !ch.library.contains(m) {
                    return Err(bail(Code::NotFound, format!("media '{m}' not in library")));
                }
            }
            let pl = ch.playlist_mut(&id).ok_or_else(|| not_found(&id))?;
            for m in media {
                let mut e = PlaylistEntry::new(m);
                e.pinned = pinned;
                pl.entries.push(e);
            }
            println!("'{id}' now has {} entries", pl.entries.len());
        }
        PlaylistCmd::RemoveEntry { id, index } => {
            let pl = ch.playlist_mut(&id).ok_or_else(|| not_found(&id))?;
            if index >= pl.entries.len() {
                return Err(bail(Code::Invalid, format!("index {index} out of range")));
            }
            let e = pl.entries.remove(index);
            println!("removed '{}' from '{id}'", e.media);
        }
        PlaylistCmd::Move { id, from, to } => {
            let pl = ch.playlist_mut(&id).ok_or_else(|| not_found(&id))?;
            if !pl.move_entry(from, to) {
                return Err(bail(Code::Invalid, "index out of range"));
            }
            println!("moved entry {from} → {to}");
        }
        PlaylistCmd::Delete { id, yes } => {
            if ch.playlist(&id).is_none() {
                return Err(not_found(&id));
            }
            confirm(&format!("Delete playlist '{id}'?"), yes)?;
            ch.playlists.retain(|p| p.id != id);
            println!("deleted '{id}'");
        }
        PlaylistCmd::Clear { id, yes } => {
            confirm(&format!("Remove all entries from '{id}'?"), yes)?;
            ch.playlist_mut(&id).ok_or_else(|| not_found(&id))?.entries.clear();
            println!("cleared '{id}'");
        }
        PlaylistCmd::Import { id, file } => {
            let text = std::fs::read_to_string(&file)?;
            let (pl, missing) = m3u::to_playlist(&id, &m3u::parse(&text), &ch.library);
            let n = pl.entries.len();
            match ch.playlist_mut(&id) {
                Some(x) => x.entries = pl.entries,
                None => ch.playlists.push(pl),
            }
            println!("imported {n} entries into '{id}'");
            for m in missing {
                eprintln!("not in library (add with `laurabrod media add`): {m}");
            }
        }
    }
    ctx.save_channel(&p, &ch)
}
