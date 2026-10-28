use super::{Ctx, vec_model};
use crate::shell::{AppWindow, FormChoices};
use slint::{ComponentHandle, Model, SharedString};

fn label(value: &str) -> &str {
    match value {
        "news" => "Новости",
        "movies" => "Фильмы",
        "ads" => "Реклама",
        "music" => "Музыка",
        "jingles" => "Заставки",
        "promo" => "Анонсы",
        "idents" => "Заставки",
        "commercials" => "Реклама",
        "promos" => "Анонсы",
        "filler" => "Заполнение",
        "backgrounds" => "Фоны",
        "maintenance" => "Настроечные таблицы",
        "" => "Без категории",
        _ => value,
    }
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<FormChoices>();
    g.on_slug(|s| crate::logic::slug::slug(&s).into());
    let weak = ui.as_weak();
    g.on_category_index(move |s| {
        weak.upgrade().and_then(|ui| ui.global::<FormChoices>().get_category_values().iter().position(|v| v == s)).map_or(-1, |i| i as i32)
    });
    g.on_category_labels(|s| s.split(',').map(str::trim).filter(|v| !v.is_empty()).map(label).collect::<Vec<_>>().join(", ").into());
    g.on_toggle_category(|s, v| {
        let mut selected: Vec<String> = s.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
        if v.is_empty() {
            selected.clear();
        } else if selected.iter().any(|s| s == v.as_str()) {
            selected.retain(|s| s != v.as_str());
        } else {
            selected.push(v.to_string());
        }
        selected.join(", ").into()
    });
}

fn collect_categories(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if key.contains("categor") {
                    if let Some(s) = value.as_str().filter(|s| !s.is_empty()) {
                        into.push(s.to_string());
                    }
                    if let Some(list) = value.as_array() {
                        into.extend(list.iter().filter_map(|v| v.as_str()).map(str::to_string));
                    }
                }
                collect_categories(value, into);
            }
        }
        serde_json::Value::Array(list) => {
            for value in list {
                collect_categories(value, into);
            }
        }
        _ => {}
    }
}

pub fn refresh(c: &Ctx) {
    let Some(ui) = c.window() else { return };
    let mut values: Vec<String> =
        ["news", "idents", "commercials", "promos", "filler", "backgrounds", "maintenance"].map(str::to_string).to_vec();
    values.extend(c.model.library.iter().filter_map(|m| m.category.clone()));
    if let Ok(value) = serde_json::to_value((&c.model.rules, &c.model.graphics, &c.model.schedules)) {
        collect_categories(&value, &mut values);
    }
    values.retain(|s| !s.is_empty());
    values.sort();
    values.dedup();
    values.insert(0, String::new());
    let mut labels: Vec<SharedString> = values.iter().map(|v| label(v).into()).collect();
    labels.push("Новая категория…".into());
    values.push(String::new());
    let g = ui.global::<FormChoices>();
    if g.get_category_values().iter().map(|s| s.to_string()).collect::<Vec<_>>() == values {
        return;
    }
    g.set_category_values(vec_model(values.into_iter().map(Into::into).collect()));
    g.set_categories(vec_model(labels));
}
