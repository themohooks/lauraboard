//! Template variables available to graphics: `{title}`, `{next_title}`,
//! `{next_start}`, `{presenter}`, user variables, …

use std::collections::HashMap;

use lb_core::project::ChannelData;
use lb_core::rundown::RundownItem;

/// Build the variable map for the current moment.
pub fn build(
    data: &ChannelData,
    on_air: Option<&RundownItem>,
    next: Option<&RundownItem>,
    next_program: Option<&RundownItem>,
    user: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut v = data.graphics.vars.clone();
    v.insert("channel".into(), data.config.name.clone());
    if let Some(it) = on_air {
        v.insert("title".into(), it.title.clone());
        v.insert("category".into(), it.category.clone().unwrap_or_default());
        v.insert("block".into(), it.block_title.clone().unwrap_or_default());
        if let Some(m) = data.library.get(&it.media) {
            for (k, val) in &m.meta {
                v.insert(k.clone(), val.clone());
            }
        }
        if let Some(b) = it.block.as_deref().and_then(|b| find_block_meta(data, b)) {
            for (k, val) in b {
                v.entry(k.clone()).or_insert_with(|| val.clone());
            }
        }
    }
    if let Some(n) = next {
        v.insert("next_title".into(), n.title.clone());
        v.insert("next_start".into(), n.start.format("%H:%M").to_string());
    }
    if let Some(p) = next_program {
        v.insert("next_program".into(), p.block_title.clone().unwrap_or_else(|| p.title.clone()));
        v.insert("next_program_title".into(), p.title.clone());
        v.insert("next_program_start".into(), p.start.format("%H:%M").to_string());
    }
    for (k, val) in user {
        v.insert(k.clone(), val.clone());
    }
    v
}

fn find_block_meta<'a>(data: &'a ChannelData, block: &str) -> Option<&'a HashMap<String, String>> {
    data.schedules.schedules.iter().flat_map(|s| s.templates.iter()).flat_map(|t| t.blocks.iter()).find(|b| b.id == block).map(|b| &b.meta)
}
