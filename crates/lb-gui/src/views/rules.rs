//! Interstitial rules editor: jingles, ad breaks, promos, policies.

use chrono::NaiveTime;
use eframe::egui::{self, RichText, Ui};

use lb_core::Dur;
use lb_core::rules::{InsertAction, InsertionRule, RuleSet, SelectMode, Trigger};

use super::save_doc;
use crate::app::App;
use crate::widgets::list_edit;

#[derive(Default)]
pub struct RulesView {
    draft: Option<RuleSet>,
    dirty: bool,
    selected: usize,
    from_buf: String,
    to_buf: String,
}

fn trigger_name(t: &Trigger) -> String {
    match t {
        Trigger::EveryNItems { n } => format!("каждые {n} ролика"),
        Trigger::EveryMinutes { minutes } => format!("каждые {minutes} мин"),
        Trigger::BlockStart => "в начале блока".into(),
        Trigger::BlockEnd => "в конце блока".into(),
        Trigger::BetweenBlocks => "между блоками".into(),
        Trigger::AfterCategory { category } => format!("после «{category}»"),
        Trigger::BeforeCategory { category } => format!("перед «{category}»"),
    }
}

fn action_name(a: &InsertAction) -> String {
    match a {
        InsertAction::Jingle { category, items } => format!("заставка {}", category.clone().unwrap_or_else(|| items.join(","))),
        InsertAction::AdBreak { ad_break } => format!("реклама «{ad_break}»"),
        InsertAction::Promos { count, .. } => format!("анонсы ×{count}"),
        InsertAction::Media { items } => format!("ролики {}", items.join(",")),
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let mut v = std::mem::take(&mut app.views.rules);
    if !v.dirty {
        v.draft = Some(app.model.rules.clone());
    }
    let Some(mut draft) = v.draft.take() else {
        app.views.rules = v;
        return;
    };
    let rs = &mut draft;
    ui.horizontal(|ui| {
        ui.heading("Правила вставок");
        if ui.add_enabled(v.dirty, egui::Button::new(RichText::new("💾 Сохранить").strong())).clicked() {
            save_doc(app, "/rules", rs.clone(), "правила");
            v.dirty = false;
        }
        if ui.add_enabled(v.dirty, egui::Button::new("Отменить")).clicked() {
            v.dirty = false;
        }
        if ui.button("＋ Правило").clicked() {
            let n = rs.rules.len() + 1;
            rs.rules.push(InsertionRule {
                id: format!("rule-{n}"),
                name: "Новое правило".into(),
                enabled: false,
                trigger: Trigger::EveryNItems { n: 3 },
                action: InsertAction::Jingle { category: None, items: vec![] },
                conditions: Default::default(),
                select: SelectMode::Random,
                probability: 1.0,
                no_repeat: true,
                priority: 0,
                group: None,
                optional: true,
                skip_on_manual: true,
            });
            v.selected = rs.rules.len() - 1;
            v.dirty = true;
        }
    });
    egui::Panel::left("rules-list").resizable(true).default_size(380.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, r) in rs.rules.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    v.dirty |= ui.checkbox(&mut r.enabled, "").changed();
                    let text = format!("[{}] {} — {} → {}", r.priority, r.name, trigger_name(&r.trigger), action_name(&r.action));
                    if ui.selectable_label(v.selected == i, text).clicked() {
                        v.selected = i;
                        v.from_buf.clear();
                        v.to_buf.clear();
                    }
                });
            }
        });
        ui.separator();
        policies(ui, rs, &mut v.dirty);
    });
    egui::CentralPanel::default().show(ui, |ui| {
        let ad_breaks: Vec<String> = rs.ad_breaks.iter().map(|b| b.id.clone()).collect();
        let mut delete = false;
        if let Some(r) = rs.rules.get_mut(v.selected) {
            rule_form(ui, r, &ad_breaks, &mut v);
            if ui.button("Удалить правило").clicked() {
                delete = true;
            }
        }
        if delete {
            rs.rules.remove(v.selected);
            v.selected = 0;
            v.dirty = true;
        }
    });
    v.draft = Some(draft);
    app.views.rules = v;
}

fn rule_form(ui: &mut Ui, r: &mut InsertionRule, ad_breaks: &[String], v: &mut RulesView) {
    let d = &mut v.dirty;
    egui::Grid::new("rule").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("id");
        ui.label(&r.id);
        ui.end_row();
        ui.label("Название");
        *d |= ui.text_edit_singleline(&mut r.name).changed();
        ui.end_row();
        ui.label("Когда");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("trig").selected_text(trigger_name(&r.trigger)).show_ui(ui, |ui| {
                let opts = [
                    Trigger::EveryNItems { n: 2 },
                    Trigger::EveryMinutes { minutes: 30.0 },
                    Trigger::BlockStart,
                    Trigger::BlockEnd,
                    Trigger::BetweenBlocks,
                    Trigger::AfterCategory { category: String::new() },
                    Trigger::BeforeCategory { category: String::new() },
                ];
                for o in opts {
                    if ui.selectable_label(std::mem::discriminant(&o) == std::mem::discriminant(&r.trigger), trigger_name(&o)).clicked() {
                        r.trigger = o;
                        *d = true;
                    }
                }
            });
            match &mut r.trigger {
                Trigger::EveryNItems { n } => *d |= ui.add(egui::DragValue::new(n).range(1..=100)).changed(),
                Trigger::EveryMinutes { minutes } => *d |= ui.add(egui::DragValue::new(minutes).range(1.0..=1440.0)).changed(),
                Trigger::AfterCategory { category } | Trigger::BeforeCategory { category } => {
                    *d |= ui.text_edit_singleline(category).changed()
                }
                _ => {}
            }
        });
        ui.end_row();
        ui.label("Что вставить");
        ui.horizontal(|ui| match &mut r.action {
            InsertAction::Jingle { category, items } => {
                ui.label("заставка, категория:");
                let mut c = category.clone().unwrap_or_default();
                if ui.text_edit_singleline(&mut c).changed() {
                    *category = Some(c).filter(|c| !c.is_empty());
                    *d = true;
                }
                *d |= list_edit(ui, "или ролики:", items);
            }
            InsertAction::AdBreak { ad_break } => {
                ui.label("рекламный блок:");
                egui::ComboBox::from_id_salt("ab").selected_text(ad_break.as_str()).show_ui(ui, |ui| {
                    for b in ad_breaks {
                        *d |= ui.selectable_value(ad_break, b.clone(), b).changed();
                    }
                });
            }
            InsertAction::Promos { count, .. } => {
                ui.label("анонсов:");
                *d |= ui.add(egui::DragValue::new(count).range(1..=10)).changed();
            }
            InsertAction::Media { items } => *d |= list_edit(ui, "ролики:", items),
        });
        ui.end_row();
        ui.label("Тип действия");
        ui.horizontal(|ui| {
            if ui.small_button("заставка").clicked() {
                r.action = InsertAction::Jingle { category: None, items: vec![] };
                *d = true;
            }
            if ui.small_button("реклама").clicked() {
                r.action = InsertAction::AdBreak { ad_break: ad_breaks.first().cloned().unwrap_or_default() };
                *d = true;
            }
            if ui.small_button("анонсы").clicked() {
                r.action = InsertAction::Promos { count: 1, category: None };
                *d = true;
            }
        });
        ui.end_row();
        ui.label("Выбор");
        ui.horizontal(|ui| {
            *d |= ui.radio_value(&mut r.select, SelectMode::Sequential, "по очереди").changed();
            *d |= ui.radio_value(&mut r.select, SelectMode::Random, "случайно").changed();
            *d |= ui.checkbox(&mut r.no_repeat, "без повтора подряд").changed();
        });
        ui.end_row();
        ui.label("Вероятность");
        *d |= ui.add(egui::Slider::new(&mut r.probability, 0.0..=1.0)).changed();
        ui.end_row();
        ui.label("Приоритет / группа");
        ui.horizontal(|ui| {
            *d |= ui.add(egui::DragValue::new(&mut r.priority)).changed();
            let mut g = r.group.clone().unwrap_or_default();
            if ui.text_edit_singleline(&mut g).changed() {
                r.group = Some(g).filter(|g| !g.is_empty());
                *d = true;
            }
        });
        ui.end_row();
        ui.label("Флаги");
        ui.horizontal(|ui| {
            *d |= ui.checkbox(&mut r.optional, "можно пропустить ради точного времени").changed();
            *d |= ui.checkbox(&mut r.skip_on_manual, "пропускать при ручном переключении").changed();
        });
        ui.end_row();
        ui.label("Условия");
        ui.vertical(|ui| {
            let c = &mut r.conditions;
            ui.horizontal(|ui| *d |= list_edit(ui, "категории:", &mut c.categories));
            ui.horizontal(|ui| *d |= list_edit(ui, "кроме категорий:", &mut c.exclude_categories));
            ui.horizontal(|ui| *d |= list_edit(ui, "блоки:", &mut c.blocks));
            ui.horizontal(|ui| *d |= list_edit(ui, "кроме роликов:", &mut c.exclude_media));
            ui.horizontal(|ui| {
                ui.label("часы с");
                time_edit(ui, &mut c.from, &mut v.from_buf, d);
                ui.label("до");
                time_edit(ui, &mut c.to, &mut v.to_buf, d);
            });
            ui.horizontal(|ui| {
                ui.label("дни недели:");
                for (i, n) in ["пн", "вт", "ср", "чт", "пт", "сб", "вс"].iter().enumerate() {
                    let day = i as u32 + 1;
                    let mut on = c.weekdays.contains(&day);
                    if ui.checkbox(&mut on, *n).changed() {
                        if on {
                            c.weekdays.push(day);
                        } else {
                            c.weekdays.retain(|x| *x != day);
                        }
                        c.weekdays.sort();
                        *d = true;
                    }
                }
            });
        });
        ui.end_row();
    });
}

fn time_edit(ui: &mut Ui, t: &mut Option<NaiveTime>, buf: &mut String, d: &mut bool) {
    if buf.is_empty()
        && let Some(x) = t
    {
        *buf = x.format("%H:%M").to_string();
    }
    if ui.add(egui::TextEdit::singleline(buf).desired_width(50.0).hint_text("—")).changed() {
        let s = buf.trim();
        *t = if s.is_empty() { None } else { NaiveTime::parse_from_str(s, "%H:%M").ok().or(*t) };
        *d = true;
    }
}

fn policies(ui: &mut Ui, rs: &mut RuleSet, d: &mut bool) {
    ui.label(RichText::new("Политики").strong());
    egui::Grid::new("pol").num_columns(2).show(ui, |ui| {
        ui.label("Реклама: показов в сутки");
        let mut mx = rs.ads.max_per_day.unwrap_or(0);
        if ui.add(egui::DragValue::new(&mut mx).range(0..=1000)).changed() {
            rs.ads.max_per_day = (mx > 0).then_some(mx);
            *d = true;
        }
        ui.end_row();
        ui.label("Реклама: мин. интервал, мин");
        let mut mi = rs.ads.min_interval.map(|x| x.ms() / 60_000).unwrap_or(0);
        if ui.add(egui::DragValue::new(&mut mi).range(0..=1440)).changed() {
            rs.ads.min_interval = (mi > 0).then(|| Dur::mins(mi));
            *d = true;
        }
        ui.end_row();
        ui.label("Анонсы: горизонт, ч");
        let mut la = rs.promos.lookahead.ms() / 3_600_000;
        if ui.add(egui::DragValue::new(&mut la).range(1..=168)).changed() {
            rs.promos.lookahead = Dur::mins(la * 60);
            *d = true;
        }
        ui.end_row();
        ui.label("Переход (затемнение), мс");
        let mut f = rs.transitions.fade.ms();
        if ui.add(egui::DragValue::new(&mut f).range(0..=5000).speed(50)).changed() {
            rs.transitions.fade = Dur(f);
            *d = true;
        }
        ui.end_row();
        ui.label("Анонсы в пустотах");
        *d |= ui.checkbox(&mut rs.filler.use_promos, "").changed();
        ui.end_row();
    });
    ui.label(format!("Рекламных блоков: {} (редактируются в rules.toml)", rs.ad_breaks.len()));
}
