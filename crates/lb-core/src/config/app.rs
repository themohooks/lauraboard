//! System-level settings shared by all channels (`laurabrod.toml`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FfmpegConfig {
    /// ffmpeg executable (name in PATH or full path).
    #[serde(default = "default_ffmpeg")]
    pub ffmpeg: String,
    #[serde(default = "default_ffprobe")]
    pub ffprobe: String,
    /// FFmpeg log level passed with `-loglevel`.
    #[serde(default = "default_loglevel")]
    pub loglevel: String,
}

fn default_ffmpeg() -> String {
    "ffmpeg".into()
}
fn default_ffprobe() -> String {
    "ffprobe".into()
}
fn default_loglevel() -> String {
    "warning".into()
}

impl Default for FfmpegConfig {
    fn default() -> Self {
        FfmpegConfig { ffmpeg: default_ffmpeg(), ffprobe: default_ffprobe(), loglevel: default_loglevel() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Read-only access (status, rundown, logs).
    Viewer,
    /// Control playback, graphics, ad insertion.
    Operator,
    /// Everything incl. configuration changes.
    Admin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiToken {
    pub name: String,
    pub token: String,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_bind")]
    pub bind: String,
    /// When empty, requests from loopback are allowed without a token (admin).
    #[serde(default)]
    pub tokens: Vec<ApiToken>,
}

fn yes() -> bool {
    true
}
fn default_bind() -> String {
    "127.0.0.1:8787".into()
}

impl Default for ApiConfig {
    fn default() -> Self {
        ApiConfig { enabled: true, bind: default_bind(), tokens: vec![] }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogConfig {
    #[serde(default = "default_level")]
    pub level: String,
    /// Directory for rolling log files (relative to the project root).
    #[serde(default = "default_log_dir")]
    pub dir: String,
}

fn default_level() -> String {
    "info".into()
}
fn default_log_dir() -> String {
    "logs".into()
}

impl Default for LogConfig {
    fn default() -> Self {
        LogConfig { level: default_level(), dir: default_log_dir() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct NotifyConfig {
    /// HTTP endpoint receiving JSON POSTs for critical events.
    #[serde(default)]
    pub webhook: Option<String>,
    /// Command run on critical events (event JSON in env `LB_EVENT`).
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub ffmpeg: FfmpegConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub notify: NotifyConfig,
    /// Channel ids to load; empty = all directories in `channels/`.
    #[serde(default)]
    pub channels: Vec<String>,
}
