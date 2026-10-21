//! Fade transitions applied after planning.

use crate::rules::RuleSet;
use crate::rundown::Rundown;

/// Set fade in/out on items according to [`crate::rules::Transitions`].
pub(super) fn apply_transitions(r: &mut Rundown, rules: &RuleSet) {
    let fade = rules.transitions.fade.ms();
    if fade <= 0 {
        return;
    }
    for it in &mut r.items {
        if it.is_slate() {
            continue;
        }
        if rules.transitions.kinds.is_empty() || rules.transitions.kinds.contains(&it.kind) {
            let f = fade.min(it.duration_ms / 3);
            it.fade_in_ms = f;
            it.fade_out_ms = f;
        }
    }
}
