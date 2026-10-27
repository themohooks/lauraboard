//! LauraBoard core: data model, configuration and the broadcast planner.
//!
//! This crate is intentionally free of media / network IO so that the
//! scheduling logic can be tested deterministically. File loading lives in
//! [`project`], everything else is pure.

pub mod browser;
pub mod config;
pub mod duration;
pub mod error;
pub mod graphics;
pub mod history;
pub mod media;
pub mod planner;
pub mod playlist;
pub mod project;
pub mod rng;
pub mod rules;
pub mod rundown;
pub mod schedule;
pub mod validate;

pub use duration::Dur;
pub use error::{CoreError, Result};

/// Milliseconds — the single time unit used across planning code.
pub type Ms = i64;

/// Wall-clock time in the channel's local time zone.
pub type WallTime = chrono::NaiveDateTime;

/// Add milliseconds to a wall time.
pub fn add_ms(t: WallTime, ms: Ms) -> WallTime {
    t + chrono::Duration::milliseconds(ms)
}

/// Difference `a - b` in milliseconds.
pub fn diff_ms(a: WallTime, b: WallTime) -> Ms {
    (a - b).num_milliseconds()
}

/// Format milliseconds as `HH:MM:SS` (with sign for negative values).
pub fn fmt_ms(ms: Ms) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let s = ms.abs() / 1000;
    format!("{sign}{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}
