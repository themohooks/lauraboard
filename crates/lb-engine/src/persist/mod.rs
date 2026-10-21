//! Persistence of runtime data (separate from user configuration).
//!
//! * [`state`] — crash-recovery snapshot (`state/state.json`)
//! * [`asrun`] — as-run log (`asrun/YYYY-MM-DD.jsonl`) and history loading
//! * [`lock`] — single-instance lock per channel

pub mod asrun;
pub mod lock;
pub mod state;
