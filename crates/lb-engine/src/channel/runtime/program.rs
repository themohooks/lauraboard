//! The programme on air for the graphics: what of its block instance has
//! aired, the item on air and the planned rest ([`ProgramClock`]).

use lb_core::Ms;
use lb_core::graphics::ProgramClock;
use lb_core::media::MediaKind;
use lb_core::rundown::RundownItem;

use super::Runtime;

/// Items of the current programme that have aired.
#[derive(Debug, Default)]
pub(crate) struct ProgramLog {
    /// Block instance (`date:block`).
    key: Option<String>,
    /// uid, kind, played length.
    past: Vec<(String, MediaKind, Ms)>,
}

/// A grid block instance, or one complete pass of a sequential playlist.
fn program_key(it: &RundownItem, previous: Option<&str>) -> Option<String> {
    let key = it.block_key.as_deref()?;
    if key.starts_with("sequential:") {
        match it.cursor_after.as_ref() {
            Some((playlist, cursor)) => Some(format!("{key}:{playlist}:{}", cursor.cycle)),
            None => previous.filter(|p| p.starts_with(key)).map(str::to_string),
        }
    } else {
        Some(key.to_string())
    }
}

impl ProgramLog {
    /// `item` finished after `played_ms`.
    pub(crate) fn aired(&mut self, item: &RundownItem, played_ms: Ms) {
        let key = program_key(item, self.key.as_deref());
        if key.is_none() || key != self.key {
            self.key = key.clone();
            self.past.clear();
        }
        if key.is_none() {
            return;
        }
        // a resumed item (encoder restart) adds up
        match self.past.iter_mut().find(|(uid, ..)| *uid == item.uid) {
            Some(p) => p.2 += played_ms.max(0),
            None => self.past.push((item.uid.clone(), item.kind, played_ms.max(0))),
        }
    }
}

impl Runtime {
    /// The programme of the item on air.
    pub(super) fn program_clock(&self) -> Option<ProgramClock> {
        let o = self.on_air.as_ref()?;
        let it = &o.item;
        let Some(key) = program_key(it, self.program.key.as_deref()) else {
            return Some(ProgramClock { current: Some((it.kind, it.duration_ms)), ..Default::default() });
        };
        let past = if self.program.key.as_ref() == Some(&key) {
            // the item on air may have aired in part before a restart
            self.program.past.iter().filter(|(uid, ..)| *uid != it.uid).map(|(_, k, ms)| (*k, *ms)).collect()
        } else {
            Vec::new()
        };
        let future = self
            .sched
            .rundown
            .items
            .iter()
            .take_while(|i| program_key(i, Some(&key)).as_ref() == Some(&key))
            .map(|i| (i.kind, i.duration_ms))
            .collect();
        Some(ProgramClock { past, current: Some((it.kind, it.duration_ms)), future })
    }
}
