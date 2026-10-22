//! Library picker dialog: choose media items instead of typing their ids.
//! The screen that opens it says where the result goes ([`Target`]).

use slint::ComponentHandle;

use super::{Ctx, vec_model, with};
use crate::fmt::{KINDS, hms, kind_id};
use crate::shell::{AppWindow, PickRow, Picker};

/// Rows shown at once; the search reaches the rest.
const LIMIT: usize = 200;

/// Where the picked ids go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Spots of the selected ad break (appended).
    AdItems,
    AdIntro,
    AdOutro,
    /// Media list of a schedule block (appended).
    BlockMedia(usize),
    BlockIntro(usize),
    BlockOutro(usize),
}

impl Target {
    fn multi(self) -> bool {
        matches!(self, Target::AdItems | Target::BlockMedia(_))
    }
}

#[derive(Default)]
pub struct PickerState {
    target: Option<Target>,
    /// Checked ids in click order.
    chosen: Vec<String>,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Picker>();
    g.on_search(|| with(rows));
    g.on_toggle(|i| with(move |c| toggle(c, i as usize)));
    g.on_confirm(|| with(confirm));
}

/// Show the dialog for `target`.
pub fn open(c: &mut Ctx, target: Target, title: &str) {
    let Some(ui) = c.window() else { return };
    c.picker = PickerState { target: Some(target), chosen: Vec::new() };
    let g = ui.global::<Picker>();
    g.set_title(title.into());
    g.set_multi(target.multi());
    g.set_query("".into());
    // spots are usually ads: start there, everything else shows all kinds
    let ad = KINDS.iter().position(|k| kind_id(*k) == "ad").map(|i| i as i32 + 1).unwrap_or(0);
    g.set_kind_index(if target == Target::AdItems { ad } else { 0 });
    rows(c);
    g.set_open(true);
}

fn rows(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Picker>();
    let q = g.get_query().trim().to_lowercase();
    let kind = usize::try_from(g.get_kind_index() - 1).ok().and_then(|i| KINDS.get(i).copied());
    let mut found = c
        .model
        .library
        .iter()
        .filter(|m| m.enabled)
        .filter(|m| kind.is_none_or(|k| m.kind == k))
        .filter(|m| q.is_empty() || m.title.to_lowercase().contains(&q) || m.id.to_lowercase().contains(&q));
    let list: Vec<PickRow> = found
        .by_ref()
        .take(LIMIT)
        .map(|m| PickRow {
            id: m.id.clone().into(),
            title: m.title.clone().into(),
            kind: kind_id(m.kind).into(),
            duration: hms(m.play_duration()).into(),
            category: m.category.clone().unwrap_or_default().into(),
            checked: c.picker.chosen.contains(&m.id),
        })
        .collect();
    g.set_more(found.next().is_some());
    g.set_rows(vec_model(list));
    g.set_chosen(c.picker.chosen.len() as i32);
}

fn toggle(c: &mut Ctx, i: usize) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Picker>();
    let Some(row) = slint::Model::row_data(&g.get_rows(), i) else { return };
    let id = row.id.to_string();
    if !c.picker.target.is_some_and(Target::multi) {
        c.picker.chosen = vec![id];
        confirm(c);
        return;
    }
    if let Some(p) = c.picker.chosen.iter().position(|x| *x == id) {
        c.picker.chosen.remove(p);
    } else {
        c.picker.chosen.push(id);
    }
    let rows = g.get_rows();
    slint::Model::set_row_data(&rows, i, PickRow { checked: !row.checked, ..row });
    g.set_chosen(c.picker.chosen.len() as i32);
}

fn confirm(c: &mut Ctx) {
    if let Some(ui) = c.window() {
        ui.global::<Picker>().set_open(false);
    }
    let Some(target) = c.picker.target.take() else { return };
    let ids = std::mem::take(&mut c.picker.chosen);
    if ids.is_empty() {
        return;
    }
    match target {
        Target::AdItems | Target::AdIntro | Target::AdOutro => super::rules::picked(c, target, ids),
        Target::BlockMedia(_) | Target::BlockIntro(_) | Target::BlockOutro(_) => super::schedule::picked(c, target, ids),
    }
}

/// "Title (duration)" of a library item, or a note that it is missing.
pub fn describe(c: &Ctx, id: &str) -> (String, String, String) {
    match c.model.library.iter().find(|m| m.id == id) {
        Some(m) => (if m.title.is_empty() { m.id.clone() } else { m.title.clone() }, kind_id(m.kind).to_string(), hms(m.play_duration())),
        None => (format!("{id} — нет в медиатеке"), "gap".into(), String::new()),
    }
}
