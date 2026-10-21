use super::*;
use crate::media::PromoTarget;
use crate::rules::{AdBreak, AdOrder};

fn ad_break(id: &str, items: &[&str]) -> AdBreak {
    AdBreak {
        id: id.into(),
        name: id.into(),
        items: items.iter().map(|s| s.to_string()).collect(),
        category: None,
        order: AdOrder::Listed,
        duration: None,
        max_spots: None,
        intro: None,
        outro: None,
        fill_category: None,
    }
}

fn with_ads() -> Fx {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 10), media("ad1", MediaKind::Ad, 20), media("ad2", MediaKind::Ad, 25), media("ad3", MediaKind::Ad, 30)])
        .playlist("main", &["a"]);
    fx.mode = PlanMode::Sequential { playlist: "main".into() };
    fx.rules.ads.min_interval = None;
    fx
}

#[test]
fn ad_break_inserted_with_daily_cap() {
    let mut fx = with_ads();
    fx.rules.ad_breaks.push(ad_break("brk", &["ad1", "ad2", "ad3"]));
    fx.rules.ads.max_per_day = Some(1);
    fx.rules.rules.push(rule("ads", Trigger::EveryNItems { n: 1 }, InsertAction::AdBreak { ad_break: "brk".into() }));
    let r = fx.plan(at(10, 0), at(11, 0));
    for ad in ["ad1", "ad2", "ad3"] {
        assert_eq!(ids(&r).iter().filter(|i| **i == ad).count(), 1, "{ad} airs once");
    }
    assert!(has_warning(&r, WarningKind::EmptySource));
}

#[test]
fn ad_min_interval_respected() {
    let mut fx = with_ads();
    fx.rules.ad_breaks.push(ad_break("brk", &["ad1"]));
    fx.rules.ads.min_interval = Some(Dur::mins(25));
    fx.rules.rules.push(rule("ads", Trigger::EveryNItems { n: 1 }, InsertAction::AdBreak { ad_break: "brk".into() }));
    let r = fx.plan(at(10, 0), at(11, 0));
    let starts: Vec<_> = r.items.iter().filter(|i| i.media == "ad1").map(|i| i.start).collect();
    assert!(starts.len() >= 2);
    assert!(starts.windows(2).all(|w| (w[1] - w[0]).num_minutes() >= 25));
}

#[test]
fn fixed_length_break_is_padded() {
    let mut fx = with_ads();
    let mut b = ad_break("brk", &["ad1", "ad2", "ad3"]);
    b.duration = Some(Dur::secs(60));
    fx.rules.ad_breaks.push(b);
    fx.rules.rules.push(rule("ads", Trigger::EveryNItems { n: 1 }, InsertAction::AdBreak { ad_break: "brk".into() }));
    let r = fx.plan(at(10, 0), at(10, 11));
    let brk: Vec<_> = r.items.iter().filter(|i| i.rule.as_deref() == Some("ads")).collect();
    let total: i64 = brk.iter().map(|i| i.duration_ms).sum();
    assert_eq!(total, 60_000);
    assert_eq!(brk.iter().filter(|i| i.media.starts_with("ad")).count(), 2); // 20 + 25 fit, 30 does not
    assert!(brk.last().unwrap().is_slate());
}

#[test]
fn ad_break_as_fixed_block() {
    let mut fx = with_ads();
    fx.mode = PlanMode::Grid;
    fx.rules.ad_breaks.push(ad_break("noon", &["ad1", "ad2"]));
    fx.blocks.push(fixed("ads", 12, 0, BlockSource::AdBreak { ad_break: "noon".into() }));
    fx.blocks.push(fixed("show", 12, 5, pl_src("main")));
    let r = fx.plan(at(12, 0), at(12, 20));
    assert_eq!(&ids(&r)[..2], ["ad1", "ad2"]);
    assert_eq!(first_of_block(&r, "show").start, at(12, 5));
}

#[test]
fn promos_only_for_upcoming_programmes() {
    let mut fx = Fx::new();
    let mut promo = media("promo-evening", MediaKind::Promo, 30);
    promo.promo = Some(PromoTarget { block: Some("evening".into()), media: None, category: None });
    fx.add(vec![prog("a", 25), prog("e", 60), promo]).playlist("day", &["a"]).playlist("ev", &["e"]);
    fx.rules.promos.lookahead = Dur::mins(6 * 60);
    fx.rules.promos.min_interval = Dur::mins(1);
    fx.blocks.push(fixed("day", 18, 0, pl_src("day")));
    fx.blocks.push(fixed("evening", 20, 0, pl_src("ev")));
    fx.blocks.push(fixed("late", 21, 0, pl_src("day")));
    fx.rules.rules.push(rule("promo", Trigger::EveryNItems { n: 1 }, InsertAction::Promos { count: 1, category: None }));
    let r = fx.plan(at(18, 0), at(23, 0));
    let promos: Vec<_> = r.items.iter().filter(|i| i.media == "promo-evening").collect();
    assert!(!promos.is_empty(), "promo should air before 20:00");
    assert!(promos.iter().all(|p| p.start < at(20, 0)), "promo must expire once the programme started");
}
