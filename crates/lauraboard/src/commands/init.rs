//! `init`: create a project (optionally the demo channel).

use anyhow::Result;

use lb_core::project::Project;

use crate::ctx::Ctx;
use crate::demo;

pub fn run(ctx: &Ctx, id: &str, demo: bool) -> Result<()> {
    std::fs::create_dir_all(&ctx.root)?;
    let mut project = Project::init(&ctx.root, id)?;
    println!("project created in {}", ctx.root.display());
    if demo {
        let ch = project.channel_mut(id).expect("channel");
        demo::populate(&ctx.root, ch)?;
        ch.save()?;
        println!("demo channel '{id}' configured. Next:");
        println!("  lauraboard -p {} validate", ctx.root.display());
        println!("  lauraboard -p {} plan --hours 3", ctx.root.display());
        println!("  lauraboard -p {} run", ctx.root.display());
    } else {
        println!("edit channels/{id}/channel.toml, then add media with `lauraboard media add <files>`");
    }
    Ok(())
}
