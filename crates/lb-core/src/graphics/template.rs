//! Text templates with `{placeholders}` and item timing helpers.

use std::collections::HashMap;

use crate::rundown::RundownItem;
use crate::{Ms, WallTime, diff_ms};

/// Replace `{key}` placeholders using `vars`. Unknown keys become empty.
pub fn render_template(tpl: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while let Some(s) = rest.find('{') {
        out.push_str(&rest[..s]);
        match rest[s..].find('}') {
            Some(e) => {
                let key = &rest[s + 1..s + e];
                if let Some(v) = vars.get(key) {
                    out.push_str(v);
                }
                rest = &rest[s + e + 1..];
            }
            None => {
                out.push_str(&rest[s..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Remaining / elapsed helpers for the current item.
pub fn item_times(item: &RundownItem, actual_start: WallTime, now: WallTime) -> (Ms, Ms) {
    let elapsed = diff_ms(now, actual_start).max(0);
    (elapsed, (item.duration_ms - elapsed).max(0))
}
