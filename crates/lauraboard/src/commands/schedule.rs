//! `schedule`: broadcast grid operations.

use anyhow::Result;
use chrono::Weekday;

use lb_core::project::ChannelData;
use lb_core::schedule::{DateException, Schedule, ScheduleSet};

use crate::cli::ScheduleCmd;
use crate::ctx::Ctx;
use crate::exit::{Code, bail};
use crate::table::Table;

fn parse_days(s: &str) -> Result<Vec<Weekday>> {
    use Weekday::*;
    let mut out = Vec::new();
    for part in s.split(',').map(|x| x.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "all" => out.extend([Mon, Tue, Wed, Thu, Fri, Sat, Sun]),
            "weekdays" => out.extend([Mon, Tue, Wed, Thu, Fri]),
            "weekend" => out.extend([Sat, Sun]),
            d => out.push(d.parse::<Weekday>().map_err(|_| bail(Code::Invalid, format!("unknown day '{d}'")))?),
        }
    }
    Ok(out)
}

fn schedule_mut<'a>(ch: &'a mut ChannelData, id: Option<&str>) -> Result<&'a mut Schedule> {
    let s = match id {
        Some(id) => ch.schedules.schedules.iter_mut().find(|s| s.id == id),
        None => ch.schedules.schedules.first_mut(),
    };
    s.ok_or_else(|| bail(Code::NotFound, "schedule not found (create one in schedule.toml)"))
}

pub fn run(ctx: &Ctx, cmd: ScheduleCmd) -> Result<()> {
    let (p, mut ch) = ctx.channel_data()?;
    match cmd {
        ScheduleCmd::List => {
            if ctx.print_json(&ch.schedules)? {
                return Ok(());
            }
            for s in &ch.schedules.schedules {
                let act = s.activate_at.map(|a| format!(", active from {a}")).unwrap_or_default();
                println!("schedule '{}' ({}{act}){}", s.id, s.name, if s.enabled { "" } else { " [disabled]" });
                let w = &s.week;
                let days =
                    [("mon", &w.mon), ("tue", &w.tue), ("wed", &w.wed), ("thu", &w.thu), ("fri", &w.fri), ("sat", &w.sat), ("sun", &w.sun)];
                let map: Vec<String> =
                    days.iter().map(|(d, t)| format!("{d}={}", t.as_deref().or(w.default.as_deref()).unwrap_or("-"))).collect();
                println!("  week: {}", map.join(" "));
                for t in &s.templates {
                    let mut tb = Table::new(&["  start", "block", "source"]);
                    for b in &t.blocks {
                        tb.row(vec![
                            format!("  {}", b.start.map(|s| s.format("%H:%M:%S").to_string()).unwrap_or_else(|| "(follow)".into())),
                            b.id.clone(),
                            serde_json::to_string(&b.source).unwrap_or_default(),
                        ]);
                    }
                    println!("  template '{}' ({})", t.id, t.name);
                    tb.print();
                }
                for e in &s.exceptions {
                    println!("  exception {}: {}", e.date, e.template.as_deref().unwrap_or("none"));
                }
            }
            return Ok(());
        }
        ScheduleCmd::Export { file } => {
            let text = serde_json::to_string_pretty(&ch.schedules)?;
            match file {
                Some(f) => std::fs::write(f, text)?,
                None => println!("{text}"),
            }
            return Ok(());
        }
        ScheduleCmd::Import { file } => {
            let set: ScheduleSet = serde_json::from_str(&std::fs::read_to_string(&file)?)?;
            ch.schedules = set;
            let r = lb_core::validate::validate_channel(&ch);
            if r.has_errors() {
                return Err(bail(Code::Invalid, "imported schedule has errors (run `lauraboard validate` after fixing)"));
            }
            println!("imported {} schedule(s)", ch.schedules.schedules.len());
        }
        ScheduleCmd::Assign { schedule, template, days } => {
            let days = parse_days(&days)?;
            let s = schedule_mut(&mut ch, schedule.as_deref())?;
            if s.template(&template).is_none() {
                return Err(bail(Code::NotFound, format!("template '{template}' not found")));
            }
            s.assign_days(&template, &days);
            println!("template '{template}' assigned to {days:?}");
        }
        ScheduleCmd::Duplicate { schedule, from, to } => {
            let s = schedule_mut(&mut ch, schedule.as_deref())?;
            if s.template(&to).is_some() {
                return Err(bail(Code::Invalid, format!("template '{to}' exists")));
            }
            if !s.duplicate_template(&from, &to) {
                return Err(bail(Code::NotFound, format!("template '{from}' not found")));
            }
            println!("template '{from}' copied to '{to}'");
        }
        ScheduleCmd::Exception { schedule, date, template } => {
            let s = schedule_mut(&mut ch, schedule.as_deref())?;
            let t = if template == "none" { None } else { Some(template) };
            if let Some(t) = &t
                && s.template(t).is_none()
            {
                return Err(bail(Code::NotFound, format!("template '{t}' not found")));
            }
            s.exceptions.retain(|e| e.date != date);
            s.exceptions.push(DateException { date, template: t, note: String::new() });
            s.exceptions.sort_by_key(|e| e.date);
            println!("exception for {date} saved");
        }
    }
    ctx.save_channel(&p, &ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days() {
        assert_eq!(parse_days("weekend").unwrap(), vec![Weekday::Sat, Weekday::Sun]);
        assert_eq!(parse_days("mon,fri").unwrap(), vec![Weekday::Mon, Weekday::Fri]);
        assert!(parse_days("xyz").is_err());
    }
}
