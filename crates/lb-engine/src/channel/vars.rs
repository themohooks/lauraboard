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
        item_vars(&mut v, data, "next", n);
    }
    if let Some(p) = next_program {
        v.insert("next_program".into(), p.block_title.clone().unwrap_or_else(|| p.title.clone()));
        v.insert("next_program_title".into(), p.title.clone());
        v.insert("next_program_start".into(), p.start.format("%H:%M").to_string());
        item_vars(&mut v, data, "next_program", p);
    }
    for (k, val) in user {
        v.insert(k.clone(), val.clone());
    }
    v
}

/// `{prefix_category}`, `{prefix_block}`, `{prefix_end}`, `{prefix_duration}`,
/// `{prefix_date}` and every meta field of the item and its block as
/// `{prefix_<field>}` (say `{next_program_presenter}`), so a plate can
/// describe what comes next, not only what is on air.
fn item_vars(v: &mut HashMap<String, String>, data: &ChannelData, prefix: &str, it: &RundownItem) {
    let mut put = |k: &str, val: String| {
        v.insert(format!("{prefix}_{k}"), val);
    };
    put("category", it.category.clone().unwrap_or_default());
    put("block", it.block_title.clone().unwrap_or_default());
    put("end", it.end().format("%H:%M").to_string());
    put("duration", duration_text(it.duration_ms));
    put("date", it.start.format("%d.%m").to_string());
    put("kind", kind_text(it.kind).into());
    let mut meta: Vec<(&String, &String)> = Vec::new();
    if let Some(m) = data.library.get(&it.media) {
        meta.extend(m.meta.iter());
    }
    if let Some(b) = it.block.as_deref().and_then(|b| find_block_meta(data, b)) {
        // item fields win over the block's
        meta.extend(b.iter().filter(|(k, _)| data.library.get(&it.media).is_none_or(|m| !m.meta.contains_key(*k))));
    }
    for (k, val) in meta {
        v.entry(format!("{prefix}_{k}")).or_insert_with(|| val.clone());
    }
}

/// "1:30:00", "25:00" (mm:ss under an hour).
fn duration_text(ms: lb_core::Ms) -> String {
    let s = (ms.max(0) + 500) / 1000;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn kind_text(k: lb_core::media::MediaKind) -> &'static str {
    use lb_core::media::MediaKind as K;
    match k {
        K::Program => "Передача",
        K::Jingle => "Заставка",
        K::Ad => "Реклама",
        K::Promo => "Анонс",
        K::Filler => "Заполнение",
        K::Image => "Изображение",
        K::Stream => "Поток",
    }
}

fn find_block_meta<'a>(data: &'a ChannelData, block: &str) -> Option<&'a HashMap<String, String>> {
    data.schedules.schedules.iter().flat_map(|s| s.templates.iter()).flat_map(|t| t.blocks.iter()).find(|b| b.id == block).map(|b| &b.meta)
}
