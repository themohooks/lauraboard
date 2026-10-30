//! Demo channel configuration.

use std::path::Path;

use anyhow::Result;
use chrono::NaiveTime;

use lb_core::Dur;
use lb_core::config::OutputTarget;
use lb_core::graphics::*;
use lb_core::media::{MediaItem, MediaKind, ProbeInfo, PromoTarget, VideoInfo};
use lb_core::playlist::Playlist;
use lb_core::project::ChannelData;
use lb_core::rules::*;
use lb_core::schedule::{Block, BlockSource, DayTemplate, Schedule, ScheduleSet, WeekMap};

use super::media::CLIPS;

fn kind_of(stem: &str) -> (MediaKind, &'static str) {
    match stem.split('-').next().unwrap_or("") {
        "jingle" => (MediaKind::Jingle, "idents"),
        "ad" => (MediaKind::Ad, "commercials"),
        "promo" => (MediaKind::Promo, "promos"),
        "filler" => (MediaKind::Filler, "fillers"),
        "news" => (MediaKind::Program, "news"),
        "movie" => (MediaKind::Program, "movies"),
        _ => (MediaKind::Program, "shows"),
    }
}

fn library(ch: &mut ChannelData) {
    for (stem, secs, _, _) in CLIPS {
        let (kind, cat) = kind_of(stem);
        let mut m = MediaItem::new(*stem, format!("media/{stem}.mp4"), kind, Dur::secs(*secs as i64));
        m.title = match *stem {
            "news" => "Новости".into(),
            "movie-1" => "Вечерний фильм".into(),
            s if s.starts_with("show") => format!("Передача {}", &s[5..]),
            s => s.replace('-', " "),
        };
        m.category = Some(cat.into());
        m.probe = ProbeInfo {
            video: Some(VideoInfo { width: 1280, height: 720, fps: 25.0, codec: "h264".into() }),
            has_audio: true,
            audio_codec: "aac".into(),
            format: "mov,mp4".into(),
            error: None,
        };
        if *stem == "promo-movie" {
            m.promo = Some(PromoTarget { block: Some("evening".into()), media: None, category: None });
        }
        if kind == MediaKind::Program {
            m.meta.insert("presenter".into(), "Лаура".into());
        }
        ch.library.upsert(m);
    }
}

fn playlists(ch: &mut ChannelData) {
    let mut shows = Playlist::new("shows").with_media(&["show-1", "show-2", "show-3", "show-4"]);
    shows.name = "Дневные передачи".into();
    let mut weekend = shows.clone();
    weekend.id = "shows-weekend".into();
    weekend.name = "Выходные (случайный порядок)".into();
    weekend.shuffle = true;
    weekend.entries[0].pinned = true;
    let mut movies = Playlist::new("movies").with_media(&["movie-1"]);
    movies.name = "Фильмы".into();
    ch.playlists = vec![shows, weekend, movies];
}

fn template(id: &str, shows: &str) -> DayTemplate {
    let mut blocks = Vec::new();
    for h in 0..24u32 {
        let mut news =
            Block::new(&format!("news-{h:02}"), NaiveTime::from_hms_opt(h, 0, 0), BlockSource::Media { items: vec!["news".into()] });
        news.title = "Новости".into();
        news.category = Some("news".into());
        news.rules = true;
        blocks.push(news);
        if h == 20 {
            let mut ev = Block::new(
                "evening",
                NaiveTime::from_hms_opt(20, 5, 0),
                BlockSource::Playlist { playlist: "movies".into(), restart: true },
            );
            ev.title = "Вечерний кинозал".into();
            ev.category = Some("movies".into());
            ev.max_items = Some(1);
            ev.meta.insert("presenter".into(), "Кинозал".into());
            blocks.push(ev);
        }
        let mut day = Block::new(&format!("shows-{h:02}"), None, BlockSource::Playlist { playlist: shows.into(), restart: false });
        day.title = "Дневной эфир".into();
        blocks.push(day);
    }
    DayTemplate { id: id.into(), name: id.into(), blocks }
}

fn schedule(ch: &mut ChannelData) {
    let s = Schedule {
        id: "main".into(),
        name: "Основная сетка".into(),
        activate_at: None,
        enabled: true,
        week: WeekMap { sat: Some("weekend".into()), sun: Some("weekend".into()), default: Some("weekday".into()), ..Default::default() },
        exceptions: vec![],
        templates: vec![template("weekday", "shows"), template("weekend", "shows-weekend")],
    };
    ch.schedules = ScheduleSet { schedules: vec![s] };
}

fn rule(id: &str, name: &str, trigger: Trigger, action: InsertAction) -> InsertionRule {
    InsertionRule {
        time_conflict: Default::default(),
        id: id.into(),
        name: name.into(),
        enabled: true,
        trigger,
        action,
        conditions: Conditions::default(),
        select: SelectMode::Random,
        probability: 1.0,
        no_repeat: true,
        priority: 0,
        group: None,
        optional: true,
        skip_on_manual: true,
    }
}

fn rules(ch: &mut ChannelData) {
    let mut ident = rule(
        "ident",
        "Заставка каждые 2 передачи",
        Trigger::EveryNItems { n: 2 },
        InsertAction::Jingle { category: Some("idents".into()), items: vec![] },
    );
    ident.group = Some("between".into());
    let mut news_intro = rule(
        "news-intro",
        "Заставка перед новостями",
        Trigger::BlockStart,
        InsertAction::Jingle { category: None, items: vec!["jingle-1".into()] },
    );
    news_intro.conditions.categories = vec!["news".into()];
    news_intro.select = SelectMode::Sequential;
    news_intro.optional = false;
    news_intro.priority = 10;
    let mut ads = rule(
        "ads",
        "Реклама каждые 20 минут",
        Trigger::EveryMinutes { minutes: 20.0 },
        InsertAction::AdBreak { ad_break: "regular".into() },
    );
    ads.conditions.exclude_categories = vec!["news".into(), "movies".into()];
    ads.priority = 5;
    ads.group = Some("between".into());
    let promos = rule("promo", "Анонс между блоками", Trigger::BetweenBlocks, InsertAction::Promos { count: 1, category: None });
    ch.rules = RuleSet {
        rules: vec![news_intro, ads, ident, promos],
        ad_breaks: vec![AdBreak {
            id: "regular".into(),
            name: "Рекламный блок".into(),
            items: vec![],
            category: Some("commercials".into()),
            order: AdOrder::LeastPlayed,
            duration: None,
            max_spots: Some(2),
            intro: Some("jingle-2".into()),
            outro: None,
            fill_category: None,
        }],
        ads: AdPolicy { max_per_day: Some(48), min_interval: Some(Dur::mins(10)) },
        promos: PromoPolicy { lookahead: Dur::mins(12 * 60), min_interval: Dur::mins(10) },
        filler: FillerPolicy { min_interval: Dur(0), ..FillerPolicy::default() },
        transitions: Transitions { fade: Dur(300), kinds: vec![MediaKind::Program] },
    };
}

fn el(id: &str, kind: ElementKind, placement: Placement, layer: i32) -> Element {
    Element { id: id.into(), name: id.into(), kind, placement, opacity: 1.0, layer, animation: Animation::default(), profiles: vec![] }
}

fn style(size: f32) -> TextStyle {
    TextStyle { size, shadow_color: Some("#000000A0".into()), ..Default::default() }
}

fn grule(id: &str, element: &str, timing: GfxTiming, kinds: Vec<MediaKind>) -> GraphicRule {
    GraphicRule {
        id: id.into(),
        name: id.into(),
        enabled: true,
        element: element.into(),
        priority: 0,
        layer: None,
        timing,
        conditions: Conditions { kinds, ..Default::default() },
        profiles: vec![],
        texts: vec![],
        hide_if_empty: false,
    }
}

fn graphics(ch: &mut ChannelData) {
    let programs = vec![MediaKind::Program, MediaKind::Filler];
    let mut logo = el(
        "logo",
        ElementKind::Image { path: "media/logo.png".into() },
        Placement { anchor: Anchor::TopRight, x: 30.0, y: 30.0, width: Some(160.0), height: None },
        10,
    );
    logo.opacity = 0.85;
    let clock = el(
        "clock",
        ElementKind::Clock { format: "%H:%M".into(), style: style(30.0), tz: None },
        Placement { anchor: Anchor::TopLeft, x: 30.0, y: 30.0, width: None, height: None },
        10,
    );
    // lower third: one plate whose lines follow the programme
    let mut title = PlateField::new("Передача", "{title}", 34.0, "#FFFFFF");
    title.style.bold = true;
    let mut lower = el(
        "lower-third",
        ElementKind::Plate {
            fields: vec![title, PlateField::new("Подпись", "{block} · ведущая: {presenter}", 22.0, "#FFD166")],
            color: "#101828E0".into(),
            radius: 8.0,
            shape: ShapeStyle {
                gradient: Some("#1E3A8AE0".into()),
                accent: Some(Accent { side: Side::Left, size: 10.0, color: "#C0182B".into() }),
                shadow_color: Some("#00000080".into()),
                shadow_blur: 12.0,
                shadow_offset: (0.0, 4.0),
                ..Default::default()
            },
            width: None,
            min_width: 520.0,
            padding_x: 24.0,
            padding_y: 12.0,
            spacing: 2.0,
            align: TextAlign::Left,
            layout: Default::default(),
            valign: Default::default(),
            fit: false,
            icon: None,
        },
        Placement { anchor: Anchor::BottomLeft, x: 60.0, y: 110.0, width: None, height: None },
        20,
    );
    lower.animation = Animation { anim_in: AnimKind::SlideRight, anim_out: AnimKind::Fade, duration: Dur(600) };
    let mut next = el(
        "next-up",
        ElementKind::Text {
            text: "Далее: {next_title} в {next_start}".into(),
            style: TextStyle { size: 28.0, background: Some("#C0182BE0".into()), corner_radius: 6.0, ..style(28.0) },
        },
        Placement { anchor: Anchor::BottomRight, x: 60.0, y: 110.0, width: None, height: None },
        20,
    );
    next.animation = Animation { anim_in: AnimKind::SlideLeft, anim_out: AnimKind::Fade, duration: Dur(500) };
    let ticker = el(
        "ticker",
        ElementKind::Ticker {
            text: "LAURA TV — круглосуточное вещание • {ticker} • Сейчас: {title} • Далее: {next_program} в {next_program_start}".into(),
            style: TextStyle { size: 26.0, background: Some("#000000B0".into()), padding: 8.0, ..style(26.0) },
            speed: 110.0,
            width: None,
        },
        Placement { anchor: Anchor::BottomLeft, x: 0.0, y: 20.0, width: None, height: None },
        30,
    );
    let lt_timing = GfxTiming { after_start: Some(Dur::secs(5)), duration: Some(Dur::secs(10)), ..Default::default() };
    let next_timing = GfxTiming { last: Some(Dur::secs(20)), before_end: Some(Dur::secs(1)), ..Default::default() };
    let mut ticker_rule = grule("ticker", "ticker", GfxTiming::default(), vec![]);
    ticker_rule.conditions.exclude_categories = vec!["commercials".into()];
    ch.graphics = GraphicsConfig {
        layers: vec![Layer { exclusive: true, ..Layer::new(20, "Плашки") }],
        elements: vec![logo, clock, lower, next, ticker],
        rules: vec![
            grule("logo", "logo", GfxTiming::default(), programs.clone()),
            grule("clock", "clock", GfxTiming::default(), programs.clone()),
            grule("lower-third", "lower-third", lt_timing, vec![MediaKind::Program]),
            {
                let mut r = grule("next-up", "next-up", next_timing, vec![MediaKind::Program]);
                r.priority = 5;
                r
            },
            ticker_rule,
        ],
        vars: [("ticker".to_string(), "Смотрите вечерний кинозал в 20:05".to_string())].into_iter().collect(),
    };
}

pub fn apply(_root: &Path, ch: &mut ChannelData, _generated: bool) -> Result<()> {
    ch.config.name = "LAURA TV (demo)".into();
    ch.config.encoder.video_bitrate = "2500k".into();
    ch.config.outputs = vec![
        OutputTarget { name: "hls".into(), url: "output/hls/live.m3u8".into(), enabled: true, format: None },
        OutputTarget {
            name: "youtube".into(),
            url: "rtmp://a.rtmp.youtube.com/live2/YOUR-STREAM-KEY".into(),
            enabled: false,
            format: None,
        },
    ];
    library(ch);
    playlists(ch);
    schedule(ch);
    rules(ch);
    graphics(ch);
    Ok(())
}
