//! `validate`: configuration and media checks.

use anyhow::Result;

use lb_core::validate::{Severity, validate_channel, validate_channel_full};

use crate::ctx::Ctx;
use crate::exit::{Code, bail};
use crate::table::Table;

pub fn run(ctx: &Ctx, media: bool) -> Result<()> {
    let p = ctx.project()?;
    let mut errors = 0;
    let mut all = Vec::new();
    for ch in p.channels.iter().filter(|c| ctx.channel.as_deref().is_none_or(|id| id == c.id())) {
        let r = if media { validate_channel_full(ch, &p.root) } else { validate_channel(ch) };
        errors += r.count(Severity::Error);
        all.push((ch.id().to_string(), r));
    }
    if !ctx.print_json(&all.iter().map(|(id, r)| serde_json::json!({ "channel": id, "issues": r.issues })).collect::<Vec<_>>())? {
        for (id, r) in &all {
            if r.issues.is_empty() {
                println!("channel '{id}': OK");
                continue;
            }
            let mut t = Table::new(&["severity", "where", "message"]);
            for i in &r.issues {
                t.row(vec![format!("{:?}", i.severity).to_lowercase(), i.location.clone(), i.message.clone()]);
            }
            println!("channel '{id}': {} error(s), {} warning(s)", r.count(Severity::Error), r.count(Severity::Warning));
            t.print();
        }
    }
    if errors > 0 {
        return Err(bail(Code::Invalid, format!("{errors} validation error(s)")));
    }
    Ok(())
}
