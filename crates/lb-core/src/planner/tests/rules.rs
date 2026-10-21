use super::*;

fn with_jingles() -> Fx {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 10), prog("b", 10), media("j1", MediaKind::Jingle, 10), media("j2", MediaKind::Jingle, 10)])
        .playlist("main", &["a", "b"]);
    fx.mode = PlanMode::Sequential { playlist: "main".into() };
    fx
}

#[test]
fn every_n_items_with_sequential_rotation() {
    let mut fx = with_jingles();
    fx.rules.rules.push(jingle_rule("j", Trigger::EveryNItems { n: 2 }, &["j1", "j2"]));
    let r = fx.plan(at(10, 0), at(10, 41));
    assert_eq!(ids(&r), ["a", "b", "j1", "a", "b", "j2", "a"]);
    assert_contiguous(&r);
}

#[test]
fn random_selection_never_repeats() {
    let mut fx = with_jingles();
    let mut r0 = jingle_rule("j", Trigger::EveryNItems { n: 1 }, &["j1", "j2"]);
    r0.select = SelectMode::Random;
    fx.rules.rules.push(r0);
    let r = fx.plan(at(10, 0), at(14, 0));
    let js: Vec<_> = ids(&r).into_iter().filter(|i| i.starts_with('j')).collect();
    assert!(js.len() > 10);
    assert!(js.windows(2).all(|w| w[0] != w[1]), "{js:?}");
}

#[test]
fn probability_zero_never_fires() {
    let mut fx = with_jingles();
    let mut r0 = jingle_rule("j", Trigger::EveryNItems { n: 1 }, &["j1"]);
    r0.probability = 0.0;
    fx.rules.rules.push(r0);
    assert!(!ids(&fx.plan(at(10, 0), at(12, 0))).contains(&"j1"));
}

#[test]
fn category_triggers_and_conditions() {
    let mut fx = Fx::new();
    fx.add(vec![
        prog("n1", 10).with_category("news"),
        prog("f1", 10).with_category("films"),
        media("before", MediaKind::Jingle, 5),
        media("after", MediaKind::Jingle, 5),
    ])
    .playlist("main", &["n1", "f1"]);
    fx.mode = PlanMode::Sequential { playlist: "main".into() };
    fx.rules.rules.push(jingle_rule("b", Trigger::BeforeCategory { category: "films".into() }, &["before"]));
    fx.rules.rules.push(jingle_rule("a", Trigger::AfterCategory { category: "news".into() }, &["after"]));
    let r = fx.plan(at(10, 0), at(10, 20));
    assert_eq!(&ids(&r)[..4], ["n1", "after", "before", "f1"]);
}

#[test]
fn group_keeps_only_highest_priority() {
    let mut fx = with_jingles();
    let mut lo = jingle_rule("lo", Trigger::EveryNItems { n: 1 }, &["j1"]);
    lo.group = Some("g".into());
    let mut hi = jingle_rule("hi", Trigger::EveryNItems { n: 1 }, &["j2"]);
    hi.group = Some("g".into());
    hi.priority = 10;
    fx.rules.rules.extend([lo, hi]);
    let r = fx.plan(at(10, 0), at(10, 30));
    assert!(!ids(&r).contains(&"j1"));
    assert!(ids(&r).contains(&"j2"));
}

#[test]
fn hour_window_condition() {
    let mut fx = with_jingles();
    let mut r0 = jingle_rule("j", Trigger::EveryNItems { n: 1 }, &["j1"]);
    r0.conditions.from = Some(tm(11, 0));
    fx.rules.rules.push(r0);
    let r = fx.plan(at(10, 0), at(11, 30));
    let first_j = r.items.iter().find(|i| i.media == "j1").unwrap();
    assert!(first_j.start >= at(11, 0));
}

#[test]
fn every_minutes_trigger() {
    let mut fx = with_jingles();
    fx.rules.rules.push(jingle_rule("j", Trigger::EveryMinutes { minutes: 30.0 }, &["j1"]));
    let r = fx.plan(at(10, 0), at(11, 0));
    let js: Vec<_> = r.items.iter().filter(|i| i.media == "j1").collect();
    assert_eq!(js.len(), 2, "{:?}", ids(&r));
    assert!(js[0].start >= at(10, 30));
    assert!((js[1].start - js[0].start).num_minutes() >= 30);
}

#[test]
fn block_bookends_and_start_rule() {
    let mut fx = Fx::new();
    fx.add(vec![
        prog("a", 10),
        media("intro", MediaKind::Jingle, 5),
        media("outro", MediaKind::Jingle, 5),
        media("id", MediaKind::Jingle, 3),
    ])
    .playlist("m", &["a"]);
    let mut b = fixed("m", 6, 0, pl_src("m"));
    b.intro = Some("intro".into());
    b.outro = Some("outro".into());
    b.max_items = Some(1);
    fx.blocks.push(b);
    fx.rules.rules.push(jingle_rule("ident", Trigger::BlockStart, &["id"]));
    let r = fx.plan(at(6, 0), at(6, 10));
    assert_eq!(&ids(&r)[..4], ["intro", "id", "a", "outro"]);
}
