//! `laurabrod` — command line entry point.
//!
//! Exit codes: 0 ok, 1 error, 2 invalid configuration, 3 not found,
//! 4 API unreachable / unauthorised, 5 already running, 6 cancelled.

mod api_client;
mod cli;
mod commands;
mod ctx;
mod demo;
mod exit;
mod logging;
mod table;

use clap::Parser;

use cli::{Cli, Cmd};

fn main() {
    let cli = Cli::parse();
    let long_running = matches!(cli.cmd, Cmd::Run { .. }) || is_gui(&cli.cmd);
    let _guard = logging::init(&cli.project, long_running);
    let ctx = ctx::Ctx { root: cli.project.clone(), channel: cli.channel.clone(), token: cli.token.clone(), json: cli.json };
    let code = match commands::dispatch(&ctx, cli.cmd) {
        Ok(()) => exit::Code::Ok,
        Err(e) => {
            let code = exit::code_of(&e);
            if code != exit::Code::Cancelled {
                eprintln!("error: {e:#}");
            }
            code
        }
    };
    std::process::exit(code as i32);
}

#[cfg(feature = "gui")]
fn is_gui(c: &Cmd) -> bool {
    matches!(c, Cmd::Gui { .. })
}

#[cfg(not(feature = "gui"))]
fn is_gui(_: &Cmd) -> bool {
    false
}
