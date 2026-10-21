//! Rundown item constructors and push helpers.

use crate::media::{MediaItem, MediaKind};
use crate::playlist::Cursor;
use crate::rules::InsertionRule;
use crate::rundown::{Origin, RundownItem, WarningKind};
use crate::{Ms, WallTime};

use super::context::{Inst, Planner};

impl<'a, 'h> Planner<'a, 'h>
where
    'a: 'h,
{
    pub(super) fn item_for(&mut self, m: &'a MediaItem, origin: Origin, t: WallTime) -> RundownItem {
        RundownItem {
            uid: self.uid(),
            media: m.id.clone(),
            title: if m.title.is_empty() { m.id.clone() } else { m.title.clone() },
            kind: m.kind,
            origin,
            category: m.category.clone(),
            block: None,
            block_title: None,
            block_key: None,
            playlist: None,
            rule: None,
            start: t,
            duration_ms: m.play_duration(),
            seek_ms: m.in_point.map(|d| d.ms()).unwrap_or(0),
            truncated: false,
            optional: false,
            priority: 0,
            fixed_start: None,
            fade_in_ms: 0,
            fade_out_ms: 0,
            cursor_after: None,
            skip_on_manual: false,
        }
    }

    pub(super) fn slate_item(&mut self, t: WallTime, ms: Ms) -> RundownItem {
        RundownItem {
            uid: self.uid(),
            media: String::new(),
            title: "Slate".into(),
            kind: MediaKind::Image,
            origin: Origin::Slate,
            category: None,
            block: None,
            block_title: None,
            block_key: None,
            playlist: None,
            rule: None,
            start: t,
            duration_ms: ms,
            seek_ms: 0,
            truncated: false,
            optional: true,
            priority: -1000,
            fixed_start: None,
            fade_in_ms: 0,
            fade_out_ms: 0,
            cursor_after: None,
            skip_on_manual: true,
        }
    }

    pub(super) fn slate(&mut self, t: WallTime, ms: Ms, prev: Option<&Inst<'a>>) {
        if ms <= 0 {
            return;
        }
        let mut s = self.slate_item(t, ms);
        self.tag_block(&mut s, prev);
        self.out.push(s);
    }

    pub(super) fn tag_block(&self, it: &mut RundownItem, inst: Option<&Inst<'a>>) {
        if let Some(i) = inst {
            it.block = Some(i.block.id.clone());
            it.block_title = Some(i.block.title.clone());
            it.block_key = Some(i.key.clone());
        }
    }

    pub(super) fn push_interstitial(
        &mut self,
        m: &'a MediaItem,
        origin: Origin,
        t: &mut WallTime,
        inst: Option<&Inst<'a>>,
        r: Option<&InsertionRule>,
    ) {
        let mut it = self.item_for(m, origin, *t);
        if let Some(r) = r {
            it.rule = Some(r.id.clone());
            it.optional = r.optional;
            it.priority = r.priority;
            it.skip_on_manual = r.skip_on_manual;
        }
        self.tag_block(&mut it, inst);
        self.hist.planned.record(&m.id, *t);
        *t = it.end();
        self.out.push(it);
    }

    /// Block intro / outro by media id.
    pub(super) fn push_bookend(&mut self, id: &str, t: &mut WallTime, inst: &Inst<'a>) {
        match self.media(id) {
            Some(m) => self.push_interstitial(m, Origin::BlockBookend, t, Some(inst), None),
            None => self.warn(WarningKind::MissingMedia, Some(*t), format!("block '{}' bookend '{id}' not in library", inst.block.id)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_main(
        &mut self,
        m: &'a MediaItem,
        t: &mut WallTime,
        inst: &Inst<'a>,
        cat: Option<String>,
        playlist: Option<String>,
        cursor_after: Option<(String, Cursor)>,
        planned: Ms,
        truncated: bool,
    ) {
        let mut it = self.item_for(m, Origin::Main, *t);
        it.category = cat;
        it.playlist = playlist;
        it.cursor_after = cursor_after.clone();
        it.duration_ms = planned;
        it.truncated = truncated;
        self.tag_block(&mut it, Some(inst));
        if let Some((pl, c)) = cursor_after {
            self.st.cursors.insert(pl, c);
        }
        self.hist.planned.record(&m.id, *t);
        *t = it.end();
        self.out.push(it);
    }
}
