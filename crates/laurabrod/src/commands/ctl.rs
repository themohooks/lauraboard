//! `ctl`: control a running instance through the API.

use anyhow::Result;
use serde_json::{Value, json};

use lb_core::fmt_ms;

use crate::api_client::Client;
use crate::cli::{CtlCmd, GfxAction, OnOff};
use crate::ctx::{Ctx, confirm};
use crate::table::{Table, clip};

pub fn run(ctx: &Ctx, cmd: CtlCmd) -> Result<()> {
    let p = ctx.project()?;
    let id = ctx.channel_id(&p)?;
    let c = Client::for_project(&p, ctx.token.as_deref());
    let base = format!("/channels/{id}");
    let command = |v: Value| -> Result<()> {
        let r = c.post_json(&format!("{base}/command"), &v)?;
        println!("{}", r["message"].as_str().unwrap_or("ok"));
        Ok(())
    };
    match cmd {
        CtlCmd::Status => {
            let s = c.get_json(&format!("{base}/status"))?;
            if !ctx.print_json(&s)? {
                print_status(&s);
            }
            Ok(())
        }
        CtlCmd::Watch => loop {
            let s = c.get_json(&format!("{base}/status"))?;
            print!("\x1B[2J\x1B[H");
            print_status(&s);
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
        CtlCmd::Start => command(json!({ "cmd": "start" })),
        CtlCmd::Stop { yes } => {
            confirm(&format!("Stop broadcasting channel '{id}'?"), yes)?;
            command(json!({ "cmd": "stop" }))
        }
        CtlCmd::Skip => command(json!({ "cmd": "skip" })),
        CtlCmd::Restart => command(json!({ "cmd": "restart" })),
        CtlCmd::Seek { position } => command(seek_body(&position)?),
        CtlCmd::Manual { state } => command(json!({ "cmd": "manual", "enabled": state == OnOff::On })),
        CtlCmd::Play { media } => command(json!({ "cmd": "play_now", "media": media })),
        CtlCmd::Queue { media } => command(json!({ "cmd": "queue", "media": media })),
        CtlCmd::ClearQueue => command(json!({ "cmd": "clear_queue" })),
        CtlCmd::Adbreak { id: brk, now } => command(json!({ "cmd": "ad_break", "ad_break": brk, "now": now })),
        CtlCmd::Graphic { element, action } => match action {
            GfxAction::Show => command(json!({ "cmd": "graphic", "element": element, "action": "show" })),
            GfxAction::Hide => command(json!({ "cmd": "graphic", "element": element, "action": "hide" })),
            GfxAction::Auto => command(json!({ "cmd": "graphic_auto", "element": element })),
        },
        CtlCmd::Var { key, value } => command(json!({ "cmd": "set_var", "key": key, "value": value })),
        CtlCmd::Schedule { id: s } => {
            let schedule = if s == "auto" { Value::Null } else { Value::String(s) };
            command(json!({ "cmd": "switch_schedule", "schedule": schedule }))
        }
        CtlCmd::Reload => command(json!({ "cmd": "reload" })),
        CtlCmd::Rundown { limit } => {
            let r = c.get_json(&format!("{base}/rundown?limit={limit}"))?;
            if ctx.print_json(&r)? {
                return Ok(());
            }
            let mut t = Table::new(&["start", "dur", "kind", "title", "block"]);
            for i in r["items"].as_array().cloned().unwrap_or_default() {
                t.row(vec![
                    i["start"].as_str().unwrap_or("").replace('T', " "),
                    fmt_ms(i["duration_ms"].as_i64().unwrap_or(0)),
                    i["kind"].as_str().unwrap_or("").to_string(),
                    clip(i["title"].as_str().unwrap_or(""), 40),
                    i["block_title"].as_str().unwrap_or("").to_string(),
                ]);
            }
            t.print();
            Ok(())
        }
        CtlCmd::Logs { lines } => {
            print!("{}", c.get(&format!("/logs?lines={lines}"))?);
            Ok(())
        }
    }
}

fn print_status(s: &Value) {
    let ms = |v: &Value| fmt_ms(v.as_i64().unwrap_or(0));
    println!("channel   {} ({})", s["channel"].as_str().unwrap_or(""), s["name"].as_str().unwrap_or(""));
    println!("state     {}{}", s["state"].as_str().unwrap_or("?"), if s["manual"].as_bool().unwrap_or(false) { "  [MANUAL]" } else { "" });
    if let Some(sch) = s["force_schedule"].as_str() {
        println!("schedule  {sch} (forced)");
    }
    match s["on_air"].as_object() {
        Some(o) => {
            let item = &o["item"];
            println!(
                "on air    {}  [{} / {}]  remaining {}  deviation {:+}s",
                item["title"].as_str().unwrap_or(""),
                ms(&o["elapsed_ms"]),
                ms(&item["duration_ms"]),
                ms(&o["remaining_ms"]),
                o["deviation_ms"].as_i64().unwrap_or(0) / 1000
            );
        }
        None => println!("on air    —"),
    }
    if let Some(n) = s["next"].as_object() {
        println!("next      {}  at {}", n["title"].as_str().unwrap_or(""), n["start"].as_str().unwrap_or("").replace('T', " "));
    }
    if let Some(n) = s["next_program"].as_object() {
        println!("program   {}  in {}", n["block_title"].as_str().or(n["title"].as_str()).unwrap_or(""), ms(&s["time_to_next_program_ms"]));
    }
    let o = &s["output"];
    println!(
        "encoder   {} {}  {}  {:.2}x  fps {:.1}  restarts {}",
        if o["running"].as_bool().unwrap_or(false) { "running" } else { "DOWN" },
        o["video_codec"].as_str().unwrap_or(""),
        match o["stats"]["bitrate_kbps"].as_f64().unwrap_or(0.0) {
            b if b > 0.0 => format!("{b:.0} kbit/s"),
            _ => "bitrate n/a".to_string(),
        },
        o["stats"]["speed"].as_f64().unwrap_or(0.0),
        o["stats"]["fps"].as_f64().unwrap_or(0.0),
        o["restarts"].as_u64().unwrap_or(0)
    );
    if let Some(e) = s["last_error"].as_str() {
        println!("error     {e}");
    }
}

/// `+30` / `-10` (seconds, relative) or `HH:MM:SS` (from the item start).
fn seek_body(p: &str) -> Result<Value> {
    let p = p.trim();
    if let Some(rel) = p.strip_prefix('+').or_else(|| p.strip_prefix('-').map(|_| p)) {
        let secs: f64 = rel.parse().map_err(|_| anyhow::anyhow!("seek: number of seconds expected, e.g. +30 or -10"))?;
        return Ok(json!({ "cmd": "seek", "by_ms": (secs * 1000.0) as i64 }));
    }
    let d: lb_core::Dur = p.parse().map_err(|_| anyhow::anyhow!("seek: position HH:MM:SS or +/- seconds expected"))?;
    Ok(json!({ "cmd": "seek", "to_ms": d.ms() }))
}
