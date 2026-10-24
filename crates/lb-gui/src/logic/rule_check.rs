//! Static checks of insertion rules: conflicts between rules, broken
//! references, dependencies, human-readable summaries.

use std::collections::HashSet;

use lb_core::rules::{Conditions, InsertAction, InsertionRule, RuleSet, Trigger};

pub fn trigger_text(t: &Trigger) -> String {
    match t {
        Trigger::EveryNItems { n } => format!("каждые {n} ролика"),
        Trigger::EveryMinutes { minutes } => format!("каждые {minutes} мин"),
        Trigger::BlockStart => "в начале блока".into(),
        Trigger::BlockEnd => "в конце блока".into(),
        Trigger::BetweenBlocks => "между блоками".into(),
        Trigger::AfterCategory { category } => format!("после «{category}»"),
        Trigger::BeforeCategory { category } => format!("перед «{category}»"),
        Trigger::AfterMedia { items } if items.len() == 1 => format!("после «{}»", items[0]),
        Trigger::AfterMedia { items } => format!("после одного из {} роликов", items.len()),
        Trigger::AfterPlaylist { playlist } => format!("после плейлиста «{playlist}»"),
        Trigger::AtTimes { times } => {
            let t: Vec<String> = times.iter().map(|t| t.format("%H:%M").to_string()).collect();
            format!("в {}", if t.is_empty() { "—".into() } else { t.join(", ") })
        }
    }
}

pub fn action_text(a: &InsertAction) -> String {
    match a {
        InsertAction::Jingle { category, items } => match (category, items.is_empty()) {
            (Some(c), _) => format!("заставка из «{c}»"),
            (None, false) => format!("заставка: {}", items.join(", ")),
            (None, true) => "заставка (не выбрана)".into(),
        },
        InsertAction::AdBreak { ad_break } => format!("рекламный блок «{ad_break}»"),
        InsertAction::Promos { count, .. } => format!("анонсы × {count}"),
        InsertAction::Media { items } => format!("ролик: {}", items.join(", ")),
        InsertAction::Scene { items, duration } => {
            let what = match items.len() {
                0 => "любая сцена".to_string(),
                1 => format!("сцена «{}»", items[0]),
                n => format!("одна из {n} сцен"),
            };
            match duration {
                Some(d) => format!("{what} на {d}"),
                None => what,
            }
        }
    }
}

/// Kind id for icons / colors (jingle, ad, promo, program).
pub fn action_kind(a: &InsertAction) -> &'static str {
    match a {
        InsertAction::Jingle { .. } => "jingle",
        InsertAction::AdBreak { .. } => "ad",
        InsertAction::Promos { .. } => "promo",
        InsertAction::Media { .. } => "program",
        InsertAction::Scene { .. } => "scene",
    }
}

/// Which screen a rule belongs to: "ads", "scenes" or "jingles".
pub fn rule_scope(r: &InsertionRule) -> &'static str {
    match r.action {
        InsertAction::AdBreak { .. } | InsertAction::Promos { .. } => "ads",
        InsertAction::Scene { .. } => "scenes",
        _ => "jingles",
    }
}

fn overlap(a: &[String], b: &[String]) -> bool {
    a.is_empty() || b.is_empty() || a.iter().any(|x| b.contains(x))
}

fn conditions_overlap(a: &Conditions, b: &Conditions) -> bool {
    overlap(&a.categories, &b.categories)
        && overlap(&a.blocks, &b.blocks)
        && overlap(&a.playlists, &b.playlists)
        && (a.weekdays.is_empty() || b.weekdays.is_empty() || a.weekdays.iter().any(|d| b.weekdays.contains(d)))
}

/// Problems of rule `i` (empty = fine). `categories` are known library categories.
pub fn problems(set: &RuleSet, i: usize, categories: &HashSet<String>) -> Vec<String> {
    let Some(r) = set.rules.get(i) else { return vec![] };
    let mut out = Vec::new();
    match &r.trigger {
        Trigger::EveryNItems { n } if *n == 0 => out.push("Триггер: число роликов должно быть больше нуля".into()),
        Trigger::EveryMinutes { minutes } if *minutes <= 0.0 => out.push("Триггер: интервал должен быть больше нуля".into()),
        Trigger::AfterCategory { category } | Trigger::BeforeCategory { category } if category.trim().is_empty() => {
            out.push("Триггер: не указана категория".into())
        }
        Trigger::AfterMedia { items } if items.is_empty() => out.push("Триггер: не выбраны ролики, после которых вставлять".into()),
        Trigger::AfterPlaylist { playlist } if playlist.trim().is_empty() => out.push("Триггер: не выбран плейлист".into()),
        Trigger::AtTimes { times } if times.is_empty() => out.push("Триггер: не указано время".into()),
        _ => {}
    }
    match &r.action {
        InsertAction::AdBreak { ad_break } if !set.ad_breaks.iter().any(|b| &b.id == ad_break) => {
            out.push(format!("Рекламный блок «{ad_break}» не найден"))
        }
        InsertAction::Jingle { category: None, items } if items.is_empty() => out.push("Не выбраны заставки: укажите категорию или ролики".into()),
        InsertAction::Jingle { category: Some(c), .. } if !categories.is_empty() && !categories.contains(c) => {
            out.push(format!("В медиатеке нет категории «{c}»"))
        }
        InsertAction::Media { items } if items.is_empty() => out.push("Не выбраны ролики".into()),
        InsertAction::Scene { duration: Some(d), .. } if d.ms() <= 0 => out.push("Длительность сцены должна быть больше нуля".into()),
        _ => {}
    }
    if let (Some(f), Some(t)) = (r.conditions.from, r.conditions.to)
        && f == t
    {
        out.push("Условие времени: начало и конец совпадают".into());
    }
    if r.enabled {
        for (j, o) in set.rules.iter().enumerate() {
            if j == i || !o.enabled {
                continue;
            }
            let same_group = r.group.is_some() && r.group == o.group;
            if !same_group && o.trigger == r.trigger && o.priority == r.priority && conditions_overlap(&r.conditions, &o.conditions) {
                out.push(format!(
                    "Конфликт с «{}»: одинаковый триггер и приоритет — порядок срабатывания не определён. Задайте разный приоритет или общую группу.",
                    if o.name.is_empty() { &o.id } else { &o.name }
                ));
            }
        }
    }
    out
}

/// Related rules / objects of rule `i`.
pub fn dependencies(set: &RuleSet, i: usize) -> Vec<String> {
    let Some(r) = set.rules.get(i) else { return vec![] };
    let mut out = Vec::new();
    if let Some(g) = &r.group {
        let others: Vec<String> = set
            .rules
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && o.group.as_deref() == Some(g))
            .map(|(_, o)| if o.name.is_empty() { o.id.clone() } else { o.name.clone() })
            .collect();
        if others.is_empty() {
            out.push(format!("Группа «{g}»: других правил нет"));
        } else {
            out.push(format!("Группа «{g}» (срабатывает одно из): {}", others.join(", ")));
        }
    }
    if let InsertAction::AdBreak { ad_break } = &r.action {
        let used: Vec<String> = set
            .rules
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && matches!(&o.action, InsertAction::AdBreak { ad_break: b } if b == ad_break))
            .map(|(_, o)| if o.name.is_empty() { o.id.clone() } else { o.name.clone() })
            .collect();
        out.push(if used.is_empty() {
            format!("Использует рекламный блок «{ad_break}»")
        } else {
            format!("Рекламный блок «{ad_break}» также используют: {}", used.join(", "))
        });
    }
    out
}

/// Unique id for a new rule.
pub fn new_id(set: &RuleSet, base: &str) -> String {
    let mut n = set.rules.len() + 1;
    loop {
        let id = format!("{base}-{n}");
        if !set.rules.iter().any(|r| r.id == id) {
            return id;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::rules::SelectMode;

    fn rule(id: &str, trigger: Trigger, priority: i32) -> InsertionRule {
        InsertionRule {
            id: id.into(),
            name: id.into(),
            enabled: true,
            trigger,
            action: InsertAction::Jingle { category: Some("idents".into()), items: vec![] },
            conditions: Default::default(),
            select: SelectMode::Random,
            probability: 1.0,
            no_repeat: true,
            priority,
            group: None,
            optional: true,
            skip_on_manual: true,
        }
    }

    #[test]
    fn detects_conflicts_and_respects_groups() {
        let mut set = RuleSet::default();
        set.rules.push(rule("a", Trigger::BlockStart, 0));
        set.rules.push(rule("b", Trigger::BlockStart, 0));
        let cats: HashSet<String> = ["idents".to_string()].into();
        assert_eq!(problems(&set, 0, &cats).len(), 1);
        set.rules[0].group = Some("g".into());
        set.rules[1].group = Some("g".into());
        assert!(problems(&set, 0, &cats).is_empty());
        assert!(dependencies(&set, 0)[0].contains("b"));
    }

    #[test]
    fn detects_broken_references() {
        let mut set = RuleSet::default();
        let mut r = rule("a", Trigger::EveryNItems { n: 0 }, 0);
        r.action = InsertAction::AdBreak { ad_break: "nope".into() };
        set.rules.push(r);
        let p = problems(&set, 0, &HashSet::new());
        assert_eq!(p.len(), 2);
        assert_eq!(new_id(&set, "rule"), "rule-2");
    }
}
