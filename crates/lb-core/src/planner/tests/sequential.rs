use super::*;
use crate::history::PlayHistory;

#[test]
fn sequential_loops_and_is_contiguous() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 10), prog("b", 20)]).playlist("main", &["a", "b"]);
    fx.mode = PlanMode::Sequential { playlist: "main".into() };
    let r = fx.plan(at(10, 0), at(11, 0));
    assert_eq!(ids(&r), ["a", "b", "a", "b"]);
    assert_eq!(r.items[0].start, at(10, 0));
    assert_eq!(r.items[3].start, at(10, 40));
    assert_contiguous(&r);
}

#[test]
fn deterministic_output() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 10), prog("b", 10), prog("c", 10)]).playlist("main", &["a", "b", "c"]);
    fx.playlists[0].shuffle = true;
    fx.mode = PlanMode::Sequential { playlist: "main".into() };
    let strip = |mut r: Rundown| {
        r.items.iter_mut().for_each(|i| i.uid.clear());
        r
    };
    assert_eq!(strip(fx.plan(at(0, 0), at(6, 0))), strip(fx.plan(at(0, 0), at(6, 0))));
    // reshuffled cycles never repeat the same item at the boundary
    let r = fx.plan(at(0, 0), at(12, 0));
    let ids = ids(&r);
    assert!(ids.windows(2).all(|w| w[0] != w[1]), "{ids:?}");
}

#[test]
fn cursor_continues_after_commit() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 10), prog("b", 10), prog("c", 10)]).playlist("main", &["a", "b", "c"]);
    fx.mode = PlanMode::Sequential { playlist: "main".into() };
    let r = fx.plan(at(10, 0), at(10, 20));
    assert_eq!(ids(&r), ["a", "b"]);
    for it in &r.items {
        fx.state.commit(it);
    }
    let r2 = fx.plan(at(10, 20), at(10, 40));
    assert_eq!(ids(&r2), ["c", "a"]);
}

#[test]
fn missing_playlist_yields_slate_and_warning() {
    let mut fx = Fx::new();
    fx.mode = PlanMode::Sequential { playlist: "nope".into() };
    let r = fx.plan(at(10, 0), at(11, 0));
    assert_eq!(ids(&r), ["<slate>"]);
    assert!(has_warning(&r, WarningKind::Config));
}

#[test]
fn exclude_recent_skips_played_items() {
    let mut fx = Fx::new();
    fx.add(vec![prog("a", 10).with_category("films"), prog("b", 10).with_category("films")]);
    let mut b = fixed("films", 10, 0, BlockSource::Category { category: "films".into(), kind: None });
    b.exclude_recent = Some(Dur::mins(120));
    b.max_items = Some(1);
    fx.blocks.push(b);
    fx.history.record("a", at(9, 0));
    assert!(fx.history.last_played("a").is_some());
    let r = fx.plan(at(10, 0), at(10, 10));
    assert_eq!(ids(&r)[0], "b");
}
