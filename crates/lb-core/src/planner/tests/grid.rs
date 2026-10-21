use super::*;
use crate::schedule::LatePolicy;

/// Morning block 06:00 with 25-minute items, news at 07:00.
fn morning_news() -> Fx {
    let mut fx = Fx::new();
    fx.add(vec![prog("p1", 25), prog("p2", 25), prog("p3", 25), prog("news", 15)])
        .add(vec![media("f1", MediaKind::Filler, 180), media("f2", MediaKind::Filler, 120)])
        .playlist("morning", &["p1", "p2", "p3"])
        .playlist("news", &["news"]);
    fx.blocks.push(fixed("morning", 6, 0, pl_src("morning")));
    fx.blocks.push(fixed("news", 7, 0, pl_src("news")));
    fx
}

#[test]
fn fixed_start_is_exact_and_gap_is_filled() {
    let fx = morning_news();
    let r = fx.plan(at(6, 0), at(7, 15));
    let news = first_of_block(&r, "news");
    assert_eq!(news.start, at(7, 0), "news must start exactly at 07:00");
    assert_eq!(news.fixed_start, Some(at(7, 0)));
    // two 25-min items fit, third does not (no truncation allowed)
    let mains: Vec<_> = r.items.iter().filter(|i| i.block.as_deref() == Some("morning") && i.origin == Origin::Main).collect();
    assert_eq!(mains.len(), 2);
    assert!(mains.iter().all(|i| !i.truncated && i.duration_ms == 25 * 60_000));
    // 10 minutes gap: fillers (3m + 2m) and slate for the rest
    let fills: Vec<_> = r.items.iter().filter(|i| i.origin == Origin::Filler).collect();
    assert_eq!(fills.len(), 2);
    assert!(r.items.iter().any(|i| i.is_slate()));
    assert_contiguous(&r);
}

#[test]
fn content_is_never_truncated_without_permission() {
    let mut fx = Fx::new();
    fx.add(vec![prog("movie", 50), prog("news", 10)]).playlist("m", &["movie"]).playlist("n", &["news"]);
    fx.blocks.push(fixed("movie", 6, 0, pl_src("m")));
    fx.blocks.push(fixed("news", 6, 30, pl_src("n")));
    let r = fx.plan(at(6, 0), at(6, 40));
    assert!(r.items.iter().all(|i| !i.truncated));
    assert!(!ids(&r).contains(&"movie"));
    assert!(has_warning(&r, WarningKind::Overlap));
    assert_eq!(first_of_block(&r, "news").start, at(6, 30));
}

#[test]
fn truncate_policy_cuts_only_when_allowed() {
    let mut fx = Fx::new();
    fx.add(vec![prog("movie", 50), prog("news", 10)]).playlist("m", &["movie"]).playlist("n", &["news"]);
    let mut b = fixed("movie", 6, 0, pl_src("m"));
    b.late_policy = LatePolicy::Truncate;
    b.allow_truncate = true;
    fx.blocks.push(b);
    fx.blocks.push(fixed("news", 6, 30, pl_src("n")));
    let r = fx.plan(at(6, 0), at(6, 40));
    let movie = r.items.iter().find(|i| i.media == "movie").unwrap();
    assert!(movie.truncated);
    assert_eq!(movie.duration_ms, 30 * 60_000);
    assert_eq!(first_of_block(&r, "news").start, at(6, 30));
    assert!(has_warning(&r, WarningKind::Truncated));
}

#[test]
fn finish_policy_delays_next_block() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 20), prog("news", 10)]).playlist("m", &["a"]).playlist("n", &["news"]);
    let mut b = fixed("m", 6, 0, pl_src("m"));
    b.late_policy = LatePolicy::Finish;
    fx.blocks.push(b);
    fx.blocks.push(fixed("news", 6, 30, pl_src("n")));
    let r = fx.plan(at(6, 0), at(7, 0));
    // a(20) + a(20) -> second starts 06:20 and runs to 06:40
    let news = first_of_block(&r, "news");
    assert_eq!(news.start, at(6, 40));
    assert_eq!(news.fixed_deviation_ms(), Some(10 * 60_000));
    assert!(has_warning(&r, WarningKind::LateStart));
}

#[test]
fn optional_interstitial_dropped_to_keep_fixed_time() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 10), prog("b", 10), prog("c", 10), prog("news", 10), media("j", MediaKind::Jingle, 30)])
        .playlist("m", &["a", "b", "c"])
        .playlist("n", &["news"]);
    let mut b = fixed("m", 6, 0, pl_src("m"));
    b.max_items = Some(3);
    fx.blocks.push(b);
    fx.blocks.push(fixed("news", 6, 30, pl_src("n")));
    fx.rules.rules.push(jingle_rule("outro", Trigger::BlockEnd, &["j"]));
    let r = fx.plan(at(6, 0), at(6, 40));
    let order = ids(&r);
    let news_pos = order.iter().position(|i| *i == "news").unwrap();
    assert!(!order[..news_pos].contains(&"j"), "optional jingle must be dropped: {order:?}");
    assert_eq!(first_of_block(&r, "news").start, at(6, 30));
    assert!(has_warning(&r, WarningKind::Dropped));
}

#[test]
fn mandatory_interstitial_delays_instead_of_cutting() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 10), prog("b", 10), prog("c", 10), prog("news", 10), media("j", MediaKind::Jingle, 30)])
        .playlist("m", &["a", "b", "c"])
        .playlist("n", &["news"]);
    let mut b = fixed("m", 6, 0, pl_src("m"));
    b.max_items = Some(3);
    fx.blocks.push(b);
    fx.blocks.push(fixed("news", 6, 30, pl_src("n")));
    let mut r0 = jingle_rule("outro", Trigger::BlockEnd, &["j"]);
    r0.optional = false;
    fx.rules.rules.push(r0);
    let r = fx.plan(at(6, 0), at(6, 40));
    assert!(ids(&r).contains(&"j"));
    assert_eq!(first_of_block(&r, "news").start, at(6, 30) + chrono::Duration::seconds(30));
    assert!(has_warning(&r, WarningKind::LateStart));
}

#[test]
fn starting_mid_block_continues_it() {
    let fx = morning_news();
    let r = fx.plan(at(6, 10), at(7, 15));
    assert_eq!(r.items[0].block.as_deref(), Some("morning"));
    assert_eq!(r.items[0].start, at(6, 10));
    assert_eq!(first_of_block(&r, "news").start, at(7, 0));
}

#[test]
fn follow_block_gets_airtime_after_one_pass() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 10), prog("b", 10), prog("x", 5), prog("news", 10)])
        .playlist("ab", &["a", "b"])
        .playlist("x", &["x"])
        .playlist("n", &["news"]);
    fx.blocks.push(fixed("ab", 6, 0, pl_src("ab")));
    let mut follow = Block::new("x", None, pl_src("x"));
    follow.max_items = Some(1);
    fx.blocks.push(follow);
    fx.blocks.push(fixed("news", 7, 0, pl_src("n")));
    let r = fx.plan(at(6, 0), at(7, 10));
    let order: Vec<_> = r.items.iter().filter(|i| i.origin == Origin::Main).map(|i| i.media.as_str()).collect();
    assert_eq!(&order[..4], ["a", "b", "x", "news"]);
    assert_eq!(first_of_block(&r, "x").start, at(6, 20));
}

#[test]
fn gap_before_first_block_is_filled() {
    let fx = morning_news();
    let r = fx.plan(at(5, 50), at(6, 30));
    assert_eq!(first_of_block(&r, "morning").start, at(6, 0));
    assert_contiguous(&r);
}

#[test]
fn plans_across_midnight() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 30)]).playlist("m", &["a"]);
    fx.blocks.push(fixed("night", 23, 0, pl_src("m")));
    fx.blocks.push(fixed("morning", 6, 0, pl_src("m")));
    let r = fx.plan(at(23, 0), at(23, 0) + chrono::Duration::hours(8));
    let morning = r.items.iter().find(|i| i.block.as_deref() == Some("morning")).unwrap();
    assert_eq!(morning.start, day().succ_opt().unwrap().and_hms_opt(6, 0, 0).unwrap());
    assert_contiguous(&r);
}

#[test]
fn replan_with_prefix_continues_after_block() {
    let fx = morning_news();
    let full = fx.plan(at(6, 0), at(7, 15));
    // engine is playing the first item; keep the rest of the morning block
    let prefix: Vec<_> = full.items[1..].iter().filter(|i| i.block.as_deref() == Some("morning")).cloned().collect();
    let key = full.items[0].block_key.clone();
    let sched = fx.schedules();
    let input = crate::planner::PlanInput {
        library: &fx.lib,
        playlists: &fx.playlists,
        schedules: &sched,
        rules: &fx.rules,
        history: &fx.history,
        mode: &fx.mode,
        seed: 7,
        force_schedule: None,
    };
    // the first item ended 2 minutes late
    let mut req = crate::planner::PlanRequest::new(at(6, 27), at(7, 15));
    req.prefix = prefix;
    req.after_block = key;
    let r = crate::planner::plan_req(&input, &fx.state, &req);
    assert_eq!(r.items[0].start, at(6, 27));
    assert_eq!(first_of_block(&r, "news").start, at(7, 0));
    assert_eq!(r.items.iter().filter(|i| i.media == "news").count(), 1);
    assert_contiguous(&r);
}

#[test]
fn replan_with_late_prefix_drops_optional_fillers() {
    let fx = morning_news();
    let full = fx.plan(at(6, 0), at(7, 15));
    let prefix: Vec<_> = full.items[1..].iter().filter(|i| i.block.as_deref() == Some("morning")).cloned().collect();
    let sched = fx.schedules();
    let input = crate::planner::PlanInput {
        library: &fx.lib,
        playlists: &fx.playlists,
        schedules: &sched,
        rules: &fx.rules,
        history: &fx.history,
        mode: &fx.mode,
        seed: 7,
        force_schedule: None,
    };
    // 9 minutes late: the 10-minute gap fill must shrink
    let mut req = crate::planner::PlanRequest::new(at(6, 34), at(7, 15));
    req.prefix = prefix;
    req.after_block = full.items[0].block_key.clone();
    let r = crate::planner::plan_req(&input, &fx.state, &req);
    assert_eq!(first_of_block(&r, "news").start, at(7, 0));
    assert_contiguous(&r);
}
