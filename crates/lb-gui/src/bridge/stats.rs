//! Statistics screen: period selection, KPI cards, sortable tables of ads,
//! promos and all media, CSV export through a native save dialog.

use chrono::{Local, NaiveDate};
use serde::Deserialize;
use slint::{ComponentHandle, SharedString};

use super::{Ctx, vec_model, with};
use crate::fmt::{hms, kind_id};
use crate::logic::period::{self, Preset};
use crate::net::Msg;
use crate::shell::{AppWindow, Kpi, StatRow, Stats};

/// Mirror of the server report (the GUI depends only on the API).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Report {
    pub items: usize,
    pub airtime_ms: i64,
    pub skipped: usize,
    pub interrupted: usize,
    pub avg_deviation_ms: i64,
    pub max_deviation_ms: i64,
    pub by_media: Vec<MediaStats>,
    pub ads: Vec<MediaStats>,
    pub promos: Vec<MediaStats>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MediaStats {
    pub media: String,
    pub title: String,
    pub kind: Option<lb_core::media::MediaKind>,
    pub plays: u32,
    pub completed: u32,
    pub interrupted: u32,
    pub skipped: u32,
    pub airtime_ms: i64,
}

#[derive(Default)]
pub struct StatsState {
    report: Option<Report>,
    /// Period of the shown report.
    period: Option<(NaiveDate, NaiveDate)>,
    seq: u64,
    sort_key: i32,
    sort_desc: bool,
    initialized: bool,
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Stats>();
    g.on_preset_picked(|i| {
        with(move |c| {
            let Some(p) = Preset::ALL.get(i as usize).copied() else { return };
            let (f, t) = p.range(Local::now().date_naive());
            if let Some(ui) = c.window() {
                let g = ui.global::<Stats>();
                g.set_from(f.to_string().into());
                g.set_to(t.to_string().into());
            }
            load(c);
        })
    });
    g.on_edited(|| {
        with(|c| {
            validate(c);
        })
    });
    g.on_load(|| with(load));
    g.on_sort(|k| {
        with(move |c| {
            if c.stats.sort_key == k {
                c.stats.sort_desc = !c.stats.sort_desc;
            } else {
                c.stats.sort_key = k;
                // names ascending, numbers descending by default
                c.stats.sort_desc = k >= 2;
            }
            rows(c);
        })
    });
    g.on_tab_changed(|| with(rows));
    g.on_export(|kind| {
        let kind = kind.to_string();
        with(move |c| export(c, &kind))
    });
}

pub fn opened(c: &mut Ctx) {
    if !c.stats.initialized {
        c.stats.initialized = true;
        c.stats.sort_key = 6;
        c.stats.sort_desc = true;
        let (f, t) = Preset::Today.range(Local::now().date_naive());
        if let Some(ui) = c.window() {
            let g = ui.global::<Stats>();
            g.set_from(f.to_string().into());
            g.set_to(t.to_string().into());
        }
    }
    load(c);
}

/// Check the typed dates; show errors and the matching preset.
fn validate(c: &mut Ctx) -> Option<(NaiveDate, NaiveDate)> {
    let ui = c.window()?;
    let g = ui.global::<Stats>();
    let today = Local::now().date_naive();
    let r = period::validate(&g.get_from(), &g.get_to(), today);
    let (fe, te) = match &r {
        Ok(_) => (String::new(), String::new()),
        Err(e) => (e.from.clone().unwrap_or_default(), e.to.clone().unwrap_or_default()),
    };
    g.set_from_error(fe.into());
    g.set_to_error(te.into());
    let preset = r.as_ref().ok().and_then(|(f, t)| Preset::of(*f, *t, today)).and_then(|p| Preset::ALL.iter().position(|x| *x == p));
    g.set_preset(preset.map(|i| i as i32).unwrap_or(-1));
    r.ok()
}

fn load(c: &mut Ctx) {
    let Some((from, to)) = validate(c) else { return };
    if c.model.ch().is_empty() {
        return;
    }
    let Some(ui) = c.window() else { return };
    ui.global::<Stats>().set_loading(true);
    c.stats.seq += 1;
    let seq = c.stats.seq;
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        let r: Result<Report, String> = api.get(&format!("/channels/{ch}/stats?from={from}&to={to}"));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            if c.stats.seq != seq {
                return;
            }
            if let Some(ui) = c.window() {
                ui.global::<Stats>().set_loading(false);
            }
            match r {
                Ok(rep) => {
                    c.stats.report = Some(rep);
                    c.stats.period = Some((from, to));
                }
                Err(e) => {
                    c.stats.report = None;
                    c.toast(format!("Статистика: {e}"), true);
                }
            }
            show(c);
        })))
    });
}

fn secs(ms: i64) -> String {
    format!("{:.1} с", ms as f64 / 1000.0)
}

fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Stats>();
    let Some(r) = c.stats.report.clone() else {
        g.set_has_data(false);
        g.set_rows(vec_model(Vec::new()));
        return;
    };
    let (f, t) = c.stats.period.unwrap_or_default();
    g.set_period_text(
        if f == t {
            format!("за {}", f.format("%d.%m.%Y"))
        } else {
            format!("с {} по {}", f.format("%d.%m.%Y"), t.format("%d.%m.%Y"))
        }
        .into(),
    );
    g.set_has_data(r.items > 0);
    let kpi = |label: &str, value: String, detail: String, tone: &str| Kpi {
        label: label.into(),
        value: value.into(),
        detail: detail.into(),
        tone: tone.into(),
    };
    let ad_plays: u32 = r.ads.iter().map(|m| m.plays).sum();
    let promo_plays: u32 = r.promos.iter().map(|m| m.plays).sum();
    g.set_kpis(vec_model(vec![
        kpi("Выходов в эфир", r.items.to_string(), format!("{} роликов", r.by_media.len()), "info"),
        kpi("Время в эфире", hms(r.airtime_ms), String::new(), "success"),
        kpi("Реклама / анонсы", format!("{ad_plays} / {promo_plays}"), format!("{} / {} роликов", r.ads.len(), r.promos.len()), "info"),
        kpi(
            "Пропущено / прервано",
            format!("{} / {}", r.skipped, r.interrupted),
            "не вышли или сняты с эфира".into(),
            if r.skipped + r.interrupted > 0 { "warning" } else { "success" },
        ),
        kpi(
            "Отклонение старта",
            secs(r.avg_deviation_ms),
            format!("максимум {}", secs(r.max_deviation_ms)),
            if r.max_deviation_ms > 60_000 { "warning" } else { "success" },
        ),
    ]));
    let names = [("Реклама", r.ads.len()), ("Анонсы", r.promos.len()), ("Все ролики", r.by_media.len())];
    g.set_tab_names(vec_model(names.iter().map(|(n, k)| SharedString::from(format!("{n} · {k}"))).collect()));
    rows(c);
}

fn rows(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Stats>();
    g.set_sort_key(c.stats.sort_key);
    g.set_sort_desc(c.stats.sort_desc);
    let Some(r) = c.stats.report.as_ref() else { return };
    let mut list = match g.get_tab() {
        0 => r.ads.clone(),
        1 => r.promos.clone(),
        _ => r.by_media.clone(),
    };
    let key = c.stats.sort_key;
    list.sort_by(|a, b| {
        let o = match key {
            0 => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            1 => a.kind.map(kind_id).cmp(&b.kind.map(kind_id)),
            2 => a.plays.cmp(&b.plays),
            3 => a.completed.cmp(&b.completed),
            4 => a.interrupted.cmp(&b.interrupted),
            5 => a.skipped.cmp(&b.skipped),
            _ => a.airtime_ms.cmp(&b.airtime_ms),
        };
        if c.stats.sort_desc { o.reverse() } else { o }
    });
    let max = list.iter().map(|m| m.airtime_ms).max().unwrap_or(0).max(1) as f32;
    g.set_rows(vec_model(
        list.iter()
            .map(|m| StatRow {
                media: m.media.as_str().into(),
                title: if m.title.is_empty() { m.media.as_str().into() } else { m.title.as_str().into() },
                kind: m.kind.map(kind_id).unwrap_or("program").into(),
                plays: m.plays as i32,
                completed: m.completed as i32,
                interrupted: m.interrupted as i32,
                skipped: m.skipped as i32,
                airtime: hms(m.airtime_ms).into(),
                share: m.airtime_ms as f32 / max,
            })
            .collect(),
    ));
}

fn export(c: &mut Ctx, kind: &str) {
    let Some((from, to)) = validate(c) else { return };
    let ch = c.model.ch();
    let name = format!("{kind}-{ch}-{from}-{to}.csv");
    let Some(path) = crate::file_dialog::FileDialog::new().set_file_name(name).add_filter("CSV", &["csv"]).save_file() else { return };
    let kind = kind.to_string();
    c.worker.spawn(move |api| {
        let bytes = api.get_bytes(&format!("/channels/{ch}/{kind}?from={from}&to={to}&format=csv"))?;
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Msg::Done(format!("Сохранено: {}", path.display())))
    });
}
