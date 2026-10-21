//! Critical event notifications: webhook POST and/or a local command.

use lb_core::config::NotifyConfig;

use crate::channel::Event;

/// Fire-and-forget notification (runs on a blocking thread).
pub fn send(cfg: &NotifyConfig, ev: &Event) {
    if cfg.webhook.is_none() && cfg.command.is_none() {
        return;
    }
    let Ok(body) = serde_json::to_string(ev) else { return };
    let cfg = cfg.clone();
    std::thread::spawn(move || {
        if let Some(url) = &cfg.webhook
            && let Err(e) = ureq::post(url).header("content-type", "application/json").send(body.as_str())
        {
            tracing::warn!("notification webhook failed: {e}");
        }
        if let Some(cmd) = &cfg.command {
            let mut c = if cfg!(windows) {
                let mut c = std::process::Command::new("cmd");
                c.args(["/C", cmd]);
                c
            } else {
                let mut c = std::process::Command::new("sh");
                c.args(["-c", cmd]);
                c
            };
            if let Err(e) = c.env("LB_EVENT", &body).status() {
                tracing::warn!("notification command failed: {e}");
            }
        }
    });
}
