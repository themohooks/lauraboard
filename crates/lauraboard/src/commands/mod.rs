//! Command implementations, one module per command group.

pub mod backup;
pub mod ctl;
pub mod init;
pub mod media;
pub mod plan;
pub mod playlist;
pub mod report;
pub mod run;
pub mod schedule;
pub mod service;
pub mod validate;

use anyhow::Result;

use crate::cli::Cmd;
use crate::ctx::Ctx;

pub fn dispatch(ctx: &Ctx, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Init { id, demo } => init::run(ctx, &id, demo),
        Cmd::Run { no_api } => run::run(ctx, !no_api),
        #[cfg(feature = "gui")]
        Cmd::Gui { connect } => run::gui(ctx, connect),
        Cmd::Validate { no_media } => validate::run(ctx, !no_media),
        Cmd::Plan(a) => plan::run(ctx, &a),
        Cmd::Media(c) => media::run(ctx, c),
        Cmd::Playlist(c) => playlist::run(ctx, c),
        Cmd::Schedule(c) => schedule::run(ctx, c),
        Cmd::Report(a) => report::run(ctx, &a),
        Cmd::Backup { out } => backup::backup(ctx, out),
        Cmd::Restore { file, force } => backup::restore(ctx, &file, force),
        Cmd::Ctl(c) => ctl::run(ctx, c),
        Cmd::Service(c) => service::run(ctx, c),
    }
}

/// Parse a media kind name.
pub fn parse_kind(s: &str) -> Result<lb_core::media::MediaKind> {
    serde_json::from_value(serde_json::Value::String(s.to_ascii_lowercase())).map_err(|_| {
        crate::exit::bail(crate::exit::Code::Invalid, format!("unknown kind '{s}' (program|jingle|ad|promo|filler|image|stream)"))
    })
}

/// Async helper for commands that need the tokio runtime.
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime").block_on(f)
}
