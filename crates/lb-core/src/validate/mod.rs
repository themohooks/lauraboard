//! Pre-flight validation of a channel: configuration sanity, cross
//! references and media availability. Run before broadcasting starts and by
//! `laurabrod validate`.
//!
//! * [`settings`] — channel.toml checks
//! * [`refs`] — playlist / schedule / rule / graphics references
//! * [`media`] — media files exist and look playable

mod media;
mod refs;
mod settings;

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::project::ChannelData;

pub use media::check_media_files;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    /// Where: `channel.toml`, `playlists/main`, `schedule/weekday/news` …
    pub location: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn push(&mut self, severity: Severity, location: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue { severity, location: location.into(), message: message.into() });
    }
    pub fn error(&mut self, loc: impl Into<String>, msg: impl Into<String>) {
        self.push(Severity::Error, loc, msg);
    }
    pub fn warn(&mut self, loc: impl Into<String>, msg: impl Into<String>) {
        self.push(Severity::Warning, loc, msg);
    }
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Error)
    }
    pub fn count(&self, s: Severity) -> usize {
        self.issues.iter().filter(|i| i.severity == s).count()
    }
}

/// Validate configuration and references (no file system access).
pub fn validate_channel(ch: &ChannelData) -> Report {
    let mut r = Report::default();
    settings::check(ch, &mut r);
    refs::check(ch, &mut r);
    r
}

/// Full validation including media files relative to `root`.
pub fn validate_channel_full(ch: &ChannelData, root: &Path) -> Report {
    let mut r = validate_channel(ch);
    check_media_files(ch, root, &mut r);
    r
}
