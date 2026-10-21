//! `plan`: offline schedule preview with estimated start times.

use anyhow::Result;

use lb_core::planner::{self, PlanInput};
use lb_core::rundown::{Origin, Rundown};
use lb_core::{add_ms, fmt_ms};
use lb_engine::persist::{asrun, state};

use crate::cli::{Format, PlanArgs};
use crate::ctx::Ctx;
use crate::table::{Table, clip};

pub fn compute(ctx: &Ctx, a: &PlanArgs) -> Result<Rundown> {
    let (_, ch) = ctx.channel_data()?;
    let from = a.from.unwrap_or_else(|| chrono::Local::now().naive_local());
    let saved = state::load(&ch.state_dir());
    let hist = asrun::load_history(&ch.asrun_dir(), from.date(), (ch.config.planning.history.ms() / 86_400_000).max(1));
    let input = PlanInput {
        library: &ch.library,
        playlists: &ch.playlists,
        schedules: &ch.schedules,
        rules: &ch.rules,
        history: &hist,
        mode: &ch.config.planning.mode,
        seed: ch.config.planning.seed,
        force_schedule: saved.force_schedule.as_deref(),
    };
    Ok(planner::plan(&input, &saved.plan, from, add_ms(from, a.hours.clamp(1, 24 * 14) * 3_600_000)))
}

pub fn run(ctx: &Ctx, a: &PlanArgs) -> Result<()> {
    let mut r = compute(ctx, a)?;
    if a.programs_only {
        r.items.retain(|i| i.origin == Origin::Main);
    }
    let fmt = if ctx.json { Format::Json } else { a.format };
    match fmt {
        Format::Json => println!("{}", serde_json::to_string_pretty(&r)?),
        Format::Csv => {
            println!("start,end,duration_s,media,title,kind,origin,block,rule,optional,truncated,deviation_s");
            for i in &r.items {
                println!(
                    "{},{},{:.1},{},\"{}\",{:?},{:?},{},{},{},{},{}",
                    i.start,
                    i.end(),
                    i.duration_ms as f64 / 1000.0,
                    i.media,
                    i.title.replace('"', "\"\""),
                    i.kind,
                    i.origin,
                    i.block.clone().unwrap_or_default(),
                    i.rule.clone().unwrap_or_default(),
                    i.optional,
                    i.truncated,
                    i.fixed_deviation_ms().map(|d| (d / 1000).to_string()).unwrap_or_default()
                );
            }
        }
        Format::Table => {
            let mut t = Table::new(&["start", "end", "dur", "kind", "title", "block", "note"]);
            for i in &r.items {
                let mut note = Vec::new();
                if let Some(d) = i.fixed_deviation_ms().filter(|d| *d != 0) {
                    note.push(format!("fixed {}", fmt_ms(d)));
                }
                if i.truncated {
                    note.push("truncated".into());
                }
                if let Some(rule) = &i.rule {
                    note.push(format!("rule {rule}"));
                }
                t.row(vec![
                    i.start.format("%a %H:%M:%S").to_string(),
                    i.end().format("%H:%M:%S").to_string(),
                    fmt_ms(i.duration_ms),
                    format!("{:?}", i.kind).to_lowercase(),
                    clip(&i.title, 40),
                    i.block.clone().unwrap_or_default(),
                    note.join(", "),
                ]);
            }
            t.print();
            for w in &r.warnings {
                eprintln!("warning: {} {}", w.at.map(|t| t.to_string()).unwrap_or_default(), w.message);
            }
        }
    }
    Ok(())
}
