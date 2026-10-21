//! Shared command context: project location, selected channel, output mode.

use std::path::PathBuf;

use anyhow::{Context as _, Result};

use lb_core::project::{ChannelData, Project};

use crate::exit::{Code, bail};

pub struct Ctx {
    pub root: PathBuf,
    pub channel: Option<String>,
    pub token: Option<String>,
    pub json: bool,
}

impl Ctx {
    pub fn project(&self) -> Result<Project> {
        Project::load(&self.root).with_context(|| format!("loading project at {}", self.root.display()))
    }

    /// Selected channel id (explicit or first).
    pub fn channel_id(&self, p: &Project) -> Result<String> {
        Ok(p.pick_channel(self.channel.as_deref())?.id().to_string())
    }

    /// Load project and the selected channel for editing.
    pub fn channel_data(&self) -> Result<(Project, ChannelData)> {
        let p = self.project()?;
        let ch = p.pick_channel(self.channel.as_deref())?.clone();
        Ok((p, ch))
    }

    /// Save an edited channel and ask a running instance to reload.
    pub fn save_channel(&self, p: &Project, ch: &ChannelData) -> Result<()> {
        ch.save()?;
        crate::api_client::notify_reload(p, ch.id(), self.token.as_deref());
        Ok(())
    }

    /// Print JSON or let the caller print text.
    pub fn print_json<T: serde::Serialize>(&self, v: &T) -> Result<bool> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(v)?);
            return Ok(true);
        }
        Ok(false)
    }
}

/// Ask for confirmation on a terminal (`yes` skips the prompt).
pub fn confirm(question: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    use std::io::{BufRead, Write};
    eprint!("{question} [y/N] ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok();
    if matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes" | "д" | "да") {
        Ok(())
    } else {
        Err(bail(Code::Cancelled, "cancelled"))
    }
}
