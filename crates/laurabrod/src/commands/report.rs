//! `report`: as-run log and statistics export.

use anyhow::Result;

use lb_core::fmt_ms;
use lb_engine::persist::asrun;
use lb_engine::stats;

use crate::cli::{Format, ReportArgs, ReportKind};
use crate::ctx::Ctx;
use crate::table::{Table, clip};

pub fn run(ctx: &Ctx, a: &ReportArgs) -> Result<()> {
    let (_, ch) = ctx.channel_data()?;
    let today = chrono::Local::now().date_naive();
    let to = a.to.unwrap_or(today);
    let from = a.from.unwrap_or(to);
    let fmt = if ctx.json { Format::Json } else { a.format };
    let out = match a.kind {
        ReportKind::Asrun => {
            let recs = asrun::read_range(&ch.asrun_dir(), from, to);
            match fmt {
                Format::Json => serde_json::to_string_pretty(&recs)?,
                Format::Csv => stats::asrun_csv(&recs),
                Format::Table => {
                    let mut t = Table::new(&["planned", "started", "dev", "played", "status", "title"]);
                    for r in &recs {
                        t.row(vec![
                            r.planned_start.format("%m-%d %H:%M:%S").to_string(),
                            r.started.format("%H:%M:%S").to_string(),
                            format!("{:+}s", r.deviation_ms() / 1000),
                            fmt_ms(r.played_ms),
                            format!("{:?}", r.status).to_lowercase(),
                            clip(&r.title, 40),
                        ]);
                    }
                    t.render()
                }
            }
        }
        ReportKind::Stats => {
            let r = stats::report(&ch.asrun_dir(), from, to);
            match fmt {
                Format::Json => serde_json::to_string_pretty(&r)?,
                Format::Csv => stats::stats_csv(&r.by_media),
                Format::Table => {
                    let mut s = format!(
                        "{from} … {to}: {} items, airtime {}, skipped {}, interrupted {}, avg deviation {:.1}s, max {:.1}s\n\n",
                        r.items,
                        fmt_ms(r.airtime_ms),
                        r.skipped,
                        r.interrupted,
                        r.avg_deviation_ms as f64 / 1000.0,
                        r.max_deviation_ms as f64 / 1000.0
                    );
                    let mut t = Table::new(&["media", "kind", "plays", "skipped", "airtime", "title"]);
                    for m in &r.by_media {
                        t.row(vec![
                            m.media.clone(),
                            format!("{:?}", m.kind.unwrap_or_default()).to_lowercase(),
                            m.plays.to_string(),
                            m.skipped.to_string(),
                            fmt_ms(m.airtime_ms),
                            clip(&m.title, 40),
                        ]);
                    }
                    s.push_str(&t.render());
                    s
                }
            }
        }
    };
    match &a.out {
        Some(f) => {
            std::fs::write(f, out)?;
            eprintln!("written to {}", f.display());
        }
        None => print!("{out}{}", if out.ends_with('\n') { "" } else { "\n" }),
    }
    Ok(())
}
