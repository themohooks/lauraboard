//! Updates from GitHub releases. Unobtrusive by design: a quiet check at
//! most once a day (a while after start), a small mark in the app bar when
//! a new version is out, and nothing is downloaded or installed until the
//! operator asks for it in Settings.
//!
//! * [`release`] — release data, versions, the file for this computer
//! * [`fetch`] — GitHub requests and checked downloads
//! * [`local`] — how this copy was installed and how to install the update

pub mod fetch;
pub mod local;
pub mod release;

use std::time::Duration;

/// First quiet check after start (the operator is busy getting on air).
pub const FIRST_CHECK: Duration = Duration::from_secs(3 * 60);
/// How often a long-running window looks whether a check is due.
pub const TICK: Duration = Duration::from_secs(60 * 60);
/// Quiet checks are this far apart.
pub const INTERVAL_SECS: i64 = 24 * 60 * 60;

/// A quiet check is due (`last`: unix time of the last successful one).
pub fn due(last: Option<i64>, now: i64) -> bool {
    match last {
        None => true,
        // a clock moved back also allows a check
        Some(t) => now - t >= INTERVAL_SECS || now < t,
    }
}

/// Version of this program.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_once_a_day() {
        assert!(due(None, 1000));
        assert!(!due(Some(1000), 1000 + 3600));
        assert!(due(Some(1000), 1000 + INTERVAL_SECS));
        assert!(due(Some(1000), 10));
    }
}
