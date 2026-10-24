use super::*;
use crate::media::MediaKind;
use crate::rules::Conditions;
use crate::rundown::Origin;
use crate::rundown::RundownItem;
use crate::{Dur, WallTime};
use chrono::NaiveTime;
use std::collections::HashMap;

fn item(kind: MediaKind, cat: &str) -> RundownItem {
    RundownItem {
        uid: "u".into(),
        media: "m".into(),
        title: "T".into(),
        kind,
        origin: Origin::Main,
        category: Some(cat.into()),
        block: None,
        block_title: None,
        block_key: None,
        playlist: None,
        rule: None,
        start: now(),
        duration_ms: 600_000,
        seek_ms: 0,
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

fn now() -> WallTime {
    chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap().and_hms_opt(12, 0, 0).unwrap()
}

fn el(id: &str, layer: i32) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        kind: ElementKind::Image { path: "x.png".into() },
        placement: Placement::default(),
        opacity: 1.0,
        layer,
        animation: Animation::default(),
        profiles: vec![],
    }
}

fn rule(id: &str, el: &str, prio: i32) -> GraphicRule {
    GraphicRule {
        id: id.into(),
        name: id.into(),
        enabled: true,
        element: el.into(),
        priority: prio,
        layer: None,
        timing: GfxTiming::default(),
        conditions: Conditions::default(),
        profiles: vec![],
        texts: vec![],
        hide_if_empty: false,
    }
}

#[test]
fn logo_hidden_on_ads() {
    let mut r = rule("logo", "logo", 0);
    r.conditions.kinds = vec![MediaKind::Program];
    let cfg = GraphicsConfig { layers: vec![], elements: vec![el("logo", 0)], rules: vec![r], vars: Default::default() };
    let manual = HashMap::new();
    let prog = item(MediaKind::Program, "news");
    let ad = item(MediaKind::Ad, "ads");
    let ctx = |i| GfxContext { now: now(), item: Some(i), elapsed_ms: 1000, remaining_ms: 1000, profile: None, manual: &manual, vars: None };
    assert_eq!(evaluate(&cfg, &ctx(&prog)).len(), 1);
    assert_eq!(evaluate(&cfg, &ctx(&ad)).len(), 0);
}

#[test]
fn timing_after_start_and_before_end() {
    let mut r = rule("lt", "lt", 0);
    r.timing.after_start = Some(Dur::secs(10));
    r.timing.before_end = Some(Dur::secs(30));
    let cfg = GraphicsConfig { layers: vec![], elements: vec![el("lt", 1)], rules: vec![r], vars: Default::default() };
    let manual = HashMap::new();
    let it = item(MediaKind::Program, "x");
    let at = |e, rem| {
        evaluate(&cfg, &GfxContext { now: now(), item: Some(&it), elapsed_ms: e, remaining_ms: rem, profile: None, manual: &manual, vars: None })
    };
    assert!(at(5_000, 500_000).is_empty());
    let a = at(12_000, 500_000);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].shown_ms, 2_000);
    assert!(at(500_000, 20_000).is_empty());
}

#[test]
fn exclusive_layer_and_manual() {
    let cfg = GraphicsConfig {
        layers: vec![Layer { exclusive: true, ..Layer::new(5, "lower") }],
        elements: vec![el("a", 5), el("b", 5), el("c", 2)],
        rules: vec![rule("ra", "a", 1), rule("rb", "b", 9)],
        vars: Default::default(),
    };
    let mut manual = HashMap::new();
    let it = item(MediaKind::Program, "x");
    let r = evaluate(&cfg, &GfxContext { now: now(), item: Some(&it), elapsed_ms: 0, remaining_ms: 1, profile: None, manual: &manual, vars: None });
    assert_eq!(r.iter().map(|a| a.element.as_str()).collect::<Vec<_>>(), ["b"]);
    manual.insert("b".to_string(), Manual::Hide);
    manual.insert("c".to_string(), Manual::Show);
    let r = evaluate(&cfg, &GfxContext { now: now(), item: Some(&it), elapsed_ms: 0, remaining_ms: 1, profile: None, manual: &manual, vars: None });
    assert_eq!(r.iter().map(|a| a.element.as_str()).collect::<Vec<_>>(), ["c", "a"]);
}

#[test]
fn absolute_time_rule() {
    let mut r = rule("t", "a", 0);
    r.timing.at = vec![NaiveTime::from_hms_opt(11, 59, 0).unwrap()];
    r.timing.duration = Some(Dur::mins(2));
    let cfg = GraphicsConfig { layers: vec![], elements: vec![el("a", 0)], rules: vec![r], vars: Default::default() };
    let manual = HashMap::new();
    let r = evaluate(&cfg, &GfxContext { now: now(), item: None, elapsed_ms: 0, remaining_ms: 0, profile: None, manual: &manual, vars: None });
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].shown_ms, 60_000);
}

#[test]
fn templates() {
    let mut v = HashMap::new();
    v.insert("title".to_string(), "News".to_string());
    assert_eq!(render_template("Now: {title} {unknown}!", &v), "Now: News !");
    assert_eq!(render_template("{oops", &v), "{oops");
}

#[test]
fn last_window_for_next_up_plate() {
    let mut r = rule("next", "n", 0);
    r.timing.last = Some(Dur::secs(30));
    r.timing.before_end = Some(Dur::secs(2));
    let cfg = GraphicsConfig { layers: vec![], elements: vec![el("n", 0)], rules: vec![r], vars: Default::default() };
    let manual = HashMap::new();
    let it = item(MediaKind::Program, "x");
    let at =
        |rem| evaluate(&cfg, &GfxContext { now: now(), item: Some(&it), elapsed_ms: 0, remaining_ms: rem, profile: None, manual: &manual, vars: None });
    assert!(at(40_000).is_empty());
    let a = at(20_000);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].shown_ms, 10_000);
    assert_eq!(a[0].hide_in_ms, Some(18_000));
    assert!(at(1_000).is_empty());
}

fn plate(id: &str) -> Element {
    Element {
        kind: ElementKind::Plate {
            fields: vec![PlateField::new("Имя", "{presenter}", 32.0, "#FFFFFF"), PlateField::new("Подпись", "ведущий", 20.0, "#FFFFFF")],
            color: "#000000".into(),
            radius: 0.0,
            shape: Default::default(),
            width: None,
            min_width: 0.0,
            padding_x: 10.0,
            padding_y: 10.0,
            spacing: 4.0,
            align: Default::default(),
        },
        ..el(id, 0)
    }
}

#[test]
fn rule_texts_replace_plate_lines() {
    let mut r = rule("r", "p", 0);
    r.texts = vec!["Погода".into(), String::new()];
    let el = r.apply_texts(&plate("p")).unwrap();
    assert_eq!(element_texts(&el).unwrap(), vec!["Погода", "ведущий"]);
    r.texts = vec![" ".into()];
    assert!(r.apply_texts(&plate("p")).is_none(), "blank entries keep the design's text");
}

#[test]
fn empty_text_hides_the_plate() {
    let mut r = rule("r", "p", 0);
    r.hide_if_empty = true;
    r.texts = vec!["{presenter}".into(), "{role}".into()];
    let cfg = GraphicsConfig { layers: vec![], elements: vec![plate("p")], rules: vec![r], vars: Default::default() };
    let manual = HashMap::new();
    let it = item(MediaKind::Program, "news");
    let shown = |vars: &HashMap<String, String>| {
        let ctx = GfxContext { now: now(), item: Some(&it), elapsed_ms: 1000, remaining_ms: 1000, profile: None, manual: &manual, vars: Some(vars) };
        evaluate(&cfg, &ctx).len()
    };
    assert_eq!(shown(&HashMap::new()), 0);
    assert_eq!(shown(&HashMap::from([("presenter".to_string(), "Лаура".to_string())])), 1);
}

#[test]
fn switched_off_layer_shows_nothing_by_rules() {
    let off = Layer { enabled: false, ..Layer::new(7, "Сцены") };
    let cfg = GraphicsConfig {
        layers: vec![off],
        elements: vec![el("a", 7), el("b", 1)],
        rules: vec![rule("ra", "a", 1), rule("rb", "b", 1)],
        vars: Default::default(),
    };
    let mut manual = HashMap::new();
    let it = item(MediaKind::Program, "x");
    let ctx = |m: &HashMap<String, Manual>| evaluate(&cfg, &GfxContext { now: now(), item: Some(&it), elapsed_ms: 0, remaining_ms: 1, profile: None, manual: m, vars: None });
    assert_eq!(ctx(&manual).iter().map(|a| a.element.as_str()).collect::<Vec<_>>(), ["b"]);
    manual.insert("a".to_string(), Manual::Show);
    assert_eq!(ctx(&manual).len(), 2, "a manual show still works");
    assert_eq!(cfg.layer_ids(), [7, 1]);
    assert_eq!(cfg.layer_name(1), "Слой 1");
}
