//! `run`: headless engine (+ API) until Ctrl+C / service stop.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::watch;

use lb_engine::Engine;

use crate::ctx::Ctx;

/// Run until `stop` flips to true (or Ctrl+C).
pub async fn serve(ctx: &Ctx, api: bool, mut stop: watch::Receiver<bool>) -> Result<()> {
    let engine = Arc::new(Engine::start(&ctx.root)?);
    let hls_monitor = tokio::spawn(lb_api::hls::monitor(engine.clone()));
    let version = lb_media::hw::ffmpeg_version(&engine.app.ffmpeg.ffmpeg).await;
    match &version {
        Some(v) => tracing::info!("{v}"),
        None => {
            tracing::error!("ffmpeg not found ('{}'); install FFmpeg or set [ffmpeg] ffmpeg in lauraboard.toml", engine.app.ffmpeg.ffmpeg)
        }
    }
    for ch in engine.channels() {
        tracing::info!(channel = %ch.id(), "channel loaded");
    }
    let (api_stop_tx, api_stop_rx) = tokio::sync::oneshot::channel::<()>();
    let api_task = if api && engine.app.api.enabled {
        let e = engine.clone();
        Some(tokio::spawn(async move {
            if let Err(err) = lb_api::serve(e, async {
                let _ = api_stop_rx.await;
            })
            .await
            {
                tracing::error!("API stopped: {err}");
            }
        }))
    } else {
        None
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("interrupted, shutting down"),
        _ = async { while stop.changed().await.is_ok() { if *stop.borrow() { break; } } } => tracing::info!("stop requested"),
    }
    engine.shutdown().await;
    hls_monitor.abort();
    let _ = api_stop_tx.send(());
    if let Some(t) = api_task {
        let _ = t.await;
    }
    Ok(())
}

pub fn run(ctx: &Ctx, api: bool) -> Result<()> {
    let (_tx, rx) = watch::channel(false);
    super::block_on(serve(ctx, api, rx))
}

/// GUI: connect to a running instance or start an embedded engine first.
#[cfg(feature = "gui")]
pub fn gui(ctx: &Ctx, connect: Option<String>) -> Result<()> {
    let project = ctx.project()?;
    // the next start from a shortcut opens the same project
    crate::desktop::remember(&ctx.root);
    let mut embedded_engine = false;
    // a server of this project runs here: graphics drafts are drawn locally
    let local_project = connect.is_none().then(|| ctx.root.clone());
    let (base, token) = match connect {
        Some(url) => (url, ctx.token.clone()),
        None => {
            let base = format!("http://{}", project.app.api.bind.replace("0.0.0.0", "127.0.0.1"));
            let token = ctx.token.clone().or_else(|| project.app.api.tokens.iter().max_by_key(|t| t.role).map(|t| t.token.clone()));
            // embedded engine on a background runtime unless one is already running
            if crate::api_client::Client::new(&base, token.clone(), std::time::Duration::from_secs(1)).get("/version").is_err() {
                let (tx, rx) = watch::channel(false);
                let ctx2 = Ctx { root: ctx.root.clone(), channel: ctx.channel.clone(), token: ctx.token.clone(), json: false };
                std::thread::spawn(move || {
                    if let Err(e) = super::block_on(serve(&ctx2, true, rx)) {
                        tracing::error!("embedded engine failed: {e}");
                    }
                });
                std::mem::forget(tx);
                embedded_engine = true;
            }
            (base, token)
        }
    };
    lb_gui::run(lb_gui::GuiOptions { base_url: base, token, channel: ctx.channel.clone(), embedded_engine, project: local_project })
        .map_err(|e| anyhow::anyhow!("GUI error: {e}"))
}
