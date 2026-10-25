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

#[test]
fn not_due_interval_rule_does_not_block_its_group() {
    let mut fx = with_jingles();
    let mut every = jingle_rule("every", Trigger::EveryMinutes { minutes: 60.0 }, &["j2"]);
    every.group = Some("g".into());
    every.priority = 10;
    let mut nth = jingle_rule("nth", Trigger::EveryNItems { n: 1 }, &["j1"]);
    nth.group = Some("g".into());
    fx.rules.rules.extend([every, nth]);
    let r = fx.plan(at(10, 0), at(10, 30));
    assert!(ids(&r).contains(&"j1"), "{:?}", ids(&r));
}

// ------------------------------------------------------------ scenes

fn scene(id: &str, secs: i64) -> MediaItem {
    MediaItem::scene(id, MediaKind::Filler, crate::Dur::secs(secs), crate::media::Scene::default())
}

fn scene_rule(id: &str, trigger: Trigger, items: &[&str], secs: Option<i64>) -> InsertionRule {
    rule(id, trigger, InsertAction::Scene { items: items.iter().map(|s| s.to_string()).collect(), duration: secs.map(crate::Dur::secs) })
}

#[test]
fn scene_after_media_with_own_length() {
    let mut fx = with_jingles();
    fx.add(vec![scene("clock", 30)]);
    fx.rules.rules.push(scene_rule("s", Trigger::AfterMedia { items: vec!["b".into()] }, &["clock"], Some(90)));
    let r = fx.plan(at(10, 0), at(10, 45));
    assert_eq!(&ids(&r)[..5], ["a", "b", "clock", "a", "b"]);
    let s = r.items.iter().find(|i| i.media == "clock").unwrap();
    assert_eq!(s.duration_ms, 90_000, "rule length wins over the scene's");
    assert_eq!(s.rule.as_deref(), Some("s"));
    assert_contiguous(&r);
}

#[test]
fn scene_rule_without_items_takes_any_scene_and_skips_files() {
    let mut fx = with_jingles();
    fx.add(vec![scene("only", 20)]);
    fx.rules.rules.push(scene_rule("s", Trigger::EveryNItems { n: 1 }, &[], None));
    let r = fx.plan(at(10, 0), at(10, 25));
    assert_eq!(&ids(&r)[..4], ["a", "only", "b", "only"]);
    assert_eq!(r.items[1].duration_ms, 20_000);
}

#[test]
fn scene_after_playlist_pass() {
    let mut fx = with_jingles();
    fx.add(vec![scene("break", 60)]);
    fx.rules.rules.push(scene_rule("s", Trigger::AfterPlaylist { playlist: "main".into() }, &["break"], None));
    let r = fx.plan(at(10, 0), at(10, 50));
    assert_eq!(&ids(&r)[..6], ["a", "b", "break", "a", "b", "break"]);
}

#[test]
fn scene_at_daily_times() {
    let mut fx = with_jingles();
    fx.add(vec![scene("hour", 60)]);
    fx.rules.rules.push(scene_rule("s", Trigger::AtTimes { times: vec![tm(10, 15), tm(10, 45)] }, &["hour"], None));
    let r = fx.plan(at(10, 0), at(11, 10));
    let starts: Vec<_> = r.items.iter().filter(|i| i.media == "hour").map(|i| i.start).collect();
    // after the first item ending at or after each time
    assert_eq!(starts, [at(10, 20), at(10, 51)], "{:?}", ids(&r));
}

#[test]
fn gap_is_covered_by_a_scene() {
    let mut fx = Fx::new();
    fx.add(vec![prog("p", 25), prog("news", 10), media("f", MediaKind::Filler, 120), scene("wait", 30)])
        .playlist("m", &["p"])
        .playlist("n", &["news"]);
    fx.blocks.push(fixed("m", 6, 0, pl_src("m")));
    fx.blocks.push(fixed("n", 6, 30, pl_src("n")));
    fx.rules.filler.scenes = vec!["wait".into()];
    let r = fx.plan(at(6, 0), at(6, 40));
    // 5 minutes: the filler (2 min), then the scene for the rest, no slate
    assert!(!r.items.iter().any(|i| i.is_slate()), "{:?}", ids(&r));
    let w = r.items.iter().find(|i| i.media == "wait").unwrap();
    assert_eq!(w.duration_ms, 3 * 60_000);
    assert_eq!(first_of_block(&r, "n").start, at(6, 30));
    assert_contiguous(&r);

    fx.rules.filler.scenes_first = true;
    let r = fx.plan(at(6, 0), at(6, 40));
    assert!(!ids(&r).contains(&"f"), "the scene takes the whole gap");
    assert_eq!(r.items.iter().find(|i| i.media == "wait").unwrap().duration_ms, 5 * 60_000);
}

#[test]
fn scene_at_the_time_a_block_starts() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 25), prog("n", 10), scene("hour", 60)]).playlist("a", &["a"]).playlist("n", &["n"]);
    fx.blocks.push(fixed("a", 6, 0, pl_src("a")));
    fx.blocks.push(fixed("n", 7, 0, pl_src("n")));
    fx.rules.rules.push(scene_rule("s", Trigger::AtTimes { times: vec![tm(7, 0)] }, &["hour"], None));
    let r = fx.plan(at(6, 0), at(7, 20));
    let s = r.items.iter().find(|i| i.media == "hour" && i.rule.is_some()).expect("fires in the 07:00 block");
    assert_eq!(s.start, at(7, 10));
}

#[test]
fn scene_dropped_before_a_fixed_block_fires_in_it() {
    let mut fx = Fx::new();
    // the last item before 07:00 ends right at it: the rule fires there,
    // is dropped to keep 07:00, and must fire again in the 07:00 block
    fx.add(vec![prog("a", 30), prog("n", 10), scene("hour", 60)]).playlist("a", &["a"]).playlist("n", &["n"]);
    fx.blocks.push(fixed("a", 6, 0, pl_src("a")));
    fx.blocks.push(fixed("n", 7, 0, pl_src("n")));
    fx.rules.rules.push(scene_rule("s", Trigger::AtTimes { times: vec![tm(7, 0)] }, &["hour"], None));
    let r = fx.plan(at(6, 0), at(7, 20));
    let s = r.items.iter().find(|i| i.media == "hour" && i.rule.is_some()).expect("fires in the 07:00 block");
    assert_eq!(s.start, at(7, 10));
    assert_eq!(first_of_block(&r, "n").start, at(7, 0));
}

#[test]
fn scene_at_the_start_of_a_one_item_block_before_a_follow_block() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 20), prog("n", 10), prog("s", 15), scene("hour", 60)]).playlist("a", &["a"]).playlist("s", &["s"]);
    fx.blocks.push(fixed("a", 6, 0, pl_src("a")));
    fx.blocks.push(fixed("n", 7, 0, BlockSource::Media { items: vec!["n".into()] }));
    fx.blocks.push(Block::new("s", None, pl_src("s")));
    fx.rules.rules.push(scene_rule("s", Trigger::AtTimes { times: vec![tm(7, 0)] }, &["hour"], None));
    // an intro moves the first item of the 07:00 block past 07:00
    let mut intro = jingle_rule("intro", Trigger::BlockStart, &["j"]);
    intro.conditions.blocks = vec!["n".into()];
    fx.add(vec![media("j", MediaKind::Jingle, 10)]);
    fx.rules.rules.push(intro);
    let r = fx.plan(at(6, 0), at(7, 40));
    let s = r.items.iter().find(|i| i.media == "hour" && i.rule.is_some()).unwrap_or_else(|| panic!("fires: {:?}", ids(&r)));
    assert_eq!(s.start, at(7, 10) + chrono::Duration::seconds(10));
}
