//! `media`: library management (offline, on project files).

use anyhow::Result;

use lb_core::fmt_ms;
use lb_core::media::PromoTarget;
use lb_engine::library::{self, ImportOptions};

use super::{block_on, parse_kind};
use crate::cli::MediaCmd;
use crate::ctx::Ctx;
use crate::exit::{Code, bail};
use crate::table::{Table, clip};

pub fn run(ctx: &Ctx, cmd: MediaCmd) -> Result<()> {
    match cmd {
        MediaCmd::List { kind } => list(ctx, kind),
        MediaCmd::Add { paths, kind, category, recursive } => add(ctx, paths, kind, category, recursive),
        MediaCmd::Remove { id } => remove(ctx, &id),
        MediaCmd::Check => check(ctx),
        MediaCmd::Set { id, title, kind, category, tags, volume_db, promo_block } => {
            let (p, mut ch) = ctx.channel_data()?;
            let m = ch.library.get_mut(&id).ok_or_else(|| bail(Code::NotFound, format!("media '{id}' not found")))?;
            if let Some(t) = title {
                m.title = t;
            }
            if let Some(k) = kind {
                m.kind = parse_kind(&k)?;
            }
            if let Some(c) = category {
                m.category = Some(c).filter(|c| !c.is_empty());
            }
            if let Some(t) = tags {
                m.tags = t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            }
            if let Some(v) = volume_db {
                m.volume_db = v;
            }
            if let Some(b) = promo_block {
                m.promo = Some(PromoTarget { block: Some(b), media: None, category: None });
            }
            ch.library.reindex();
            ctx.save_channel(&p, &ch)?;
            println!("updated '{id}'");
            Ok(())
        }
    }
}

fn list(ctx: &Ctx, kind: Option<String>) -> Result<()> {
    let (_, ch) = ctx.channel_data()?;
    let k = kind.map(|k| parse_kind(&k)).transpose()?;
    let items: Vec<_> = ch.library.items.iter().filter(|m| k.is_none_or(|k| m.kind == k)).collect();
    if ctx.print_json(&items)? {
        return Ok(());
    }
    let mut t = Table::new(&["id", "kind", "duration", "category", "title", "status"]);
    for m in &items {
        let status = match (&m.probe.error, m.enabled) {
            (Some(e), _) => clip(&format!("ERROR: {e}"), 40),
            (None, false) => "disabled".into(),
            _ => String::new(),
        };
        t.row(vec![
            m.id.clone(),
            format!("{:?}", m.kind).to_lowercase(),
            fmt_ms(m.play_duration()),
            m.category.clone().unwrap_or_default(),
            clip(&m.title, 40),
            status,
        ]);
    }
    t.print();
    println!("{} item(s), total {}", items.len(), fmt_ms(items.iter().map(|m| m.play_duration()).sum()));
    Ok(())
}

fn add(ctx: &Ctx, paths: Vec<String>, kind: Option<String>, category: Option<String>, recursive: bool) -> Result<()> {
    if paths.is_empty() {
        return Err(bail(Code::Invalid, "no paths given"));
    }
    let (p, mut ch) = ctx.channel_data()?;
    let opt = ImportOptions { kind: kind.map(|k| parse_kind(&k)).transpose()?, category, recursive, image_duration: None };
    let (added, errors) = block_on(library::import(&mut ch, &p.root, &p.app.ffmpeg.ffprobe, &paths, &opt));
    if !added.is_empty() {
        ctx.save_channel(&p, &ch)?;
    }
    for id in &added {
        let m = ch.library.get(id).unwrap();
        println!("added {id:<24} {}  {}", fmt_ms(m.play_duration()), m.path);
    }
    for e in &errors {
        eprintln!("error: {e}");
    }
    if added.is_empty() && !errors.is_empty() {
        return Err(bail(Code::Invalid, "nothing imported"));
    }
    Ok(())
}

fn remove(ctx: &Ctx, id: &str) -> Result<()> {
    let (p, mut ch) = ctx.channel_data()?;
    let used: Vec<_> = ch.playlists.iter().filter(|pl| pl.entries.iter().any(|e| e.media == id)).map(|pl| pl.id.clone()).collect();
    if !used.is_empty() {
        return Err(bail(Code::Invalid, format!("'{id}' is used in playlists: {}", used.join(", "))));
    }
    ch.library.remove(id).ok_or_else(|| bail(Code::NotFound, format!("media '{id}' not found")))?;
    ctx.save_channel(&p, &ch)?;
    println!("removed '{id}'");
    Ok(())
}

fn check(ctx: &Ctx) -> Result<()> {
    let (p, mut ch) = ctx.channel_data()?;
    let problems = block_on(library::reprobe_all(&mut ch, &p.root, &p.app.ffmpeg.ffprobe));
    ctx.save_channel(&p, &ch)?;
    if problems.is_empty() {
        println!("all {} item(s) OK", ch.library.items.len());
        return Ok(());
    }
    for pr in &problems {
        eprintln!("problem: {pr}");
    }
    Err(bail(Code::Invalid, format!("{} problem(s) found", problems.len())))
}
