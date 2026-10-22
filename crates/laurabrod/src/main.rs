//! `laurabrod` — command line entry point.
//!
//! Exit codes: 0 ok, 1 error, 2 invalid configuration, 3 not found,
//! 4 API unreachable / unauthorised, 5 already running, 6 cancelled.
//!
//! Started without arguments outside a terminal (double click, shortcut)
//! it opens the GUI instead, see [`desktop`].

mod api_client;
mod cli;
mod commands;
mod ctx;
mod demo;
#[cfg(feature = "gui")]
mod desktop;
mod exit;
mod logging;
mod table;

use clap::Parser;

use cli::{Cli, Cmd};

fn main() {
    let (cli, desktop) = parse();
    let long_running = matches!(cli.cmd, Cmd::Run { .. }) || is_gui(&cli.cmd);
    let _guard = logging::init(&cli.project, long_running);
    let ctx = ctx::Ctx { root: cli.project.clone(), channel: cli.channel.clone(), token: cli.token.clone(), json: cli.json };
    let result =
        if desktop { desktop_start(&ctx).and_then(|()| commands::dispatch(&ctx, cli.cmd)) } else { commands::dispatch(&ctx, cli.cmd) };
    let code = match result {
        Ok(()) => exit::Code::Ok,
        Err(e) => {
            let code = exit::code_of(&e);
            if code != exit::Code::Cancelled {
                report(desktop, &format!("{e:#}"));
            }
            code
        }
    };
    std::process::exit(code as i32);
}

/// Command line plus "started from the desktop" (no terminal, GUI command).
#[cfg(feature = "gui")]
fn parse() -> (Cli, bool) {
    let outside = desktop::outside_terminal();
    if outside && std::env::args_os().len() == 1 {
        let mut cli = Cli::parse_from(["laurabrod", "gui"]);
        // `LAURABROD_PROJECT` still wins; otherwise the remembered project
        if cli.project == std::path::Path::new(".") {
            cli.project = desktop::project();
        }
        return (cli, true);
    }
    let cli = Cli::parse();
    let desktop = outside && is_gui(&cli.cmd);
    (cli, desktop)
}

#[cfg(not(feature = "gui"))]
fn parse() -> (Cli, bool) {
    (Cli::parse(), false)
}

/// Desktop start: hide the console and create the project on first run.
#[cfg(feature = "gui")]
fn desktop_start(ctx: &ctx::Ctx) -> anyhow::Result<()> {
    desktop::detach_console();
    if !ctx.root.join(lb_core::project::APP_FILE).exists() {
        commands::init::run(ctx, "main", false)?;
    }
    Ok(())
}

#[cfg(not(feature = "gui"))]
fn desktop_start(_: &ctx::Ctx) -> anyhow::Result<()> {
    Ok(())
}

fn report(desktop: bool, message: &str) {
    #[cfg(feature = "gui")]
    if desktop {
        desktop::show_error(message);
        return;
    }
    let _ = desktop;
    eprintln!("error: {message}");
}

#[cfg(feature = "gui")]
fn is_gui(c: &Cmd) -> bool {
    matches!(c, Cmd::Gui { .. })
}

#[cfg(not(feature = "gui"))]
fn is_gui(_: &Cmd) -> bool {
    false
}
