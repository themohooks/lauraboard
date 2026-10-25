//! Music rules of the scene draft.

use slint::ComponentHandle;

use lb_core::Dur;
use lb_core::media::scene::{MusicOrder, MusicRule};

use super::super::picker::{self, Target};
use super::{Ctx, touched, vec_model, with};
use crate::fmt::hms;
use crate::logic::timeline::parse_time;
use crate::shell::{MusicRow, Scenes, TrackRow};

const ORDERS: [MusicOrder; 3] = [MusicOrder::Sequential, MusicOrder::Shuffle, MusicOrder::Random];

pub fn connect(g: &Scenes) {
    g.on_music_add(|| {
        with(|c| {
            edit(c, |rules| {
                let n = rules.len() + 1;
                rules.push(MusicRule { name: if n == 1 { "Музыка".into() } else { format!("Правило {n}") }, ..Default::default() });
            });
            show(c);
        })
    });
    g.on_music_remove(|i| {
        with(move |c| {
            edit(c, |rules| {
                if (i as usize) < rules.len() {
                    rules.remove(i as usize);
                }
            });
            show(c);
        })
    });
    g.on_music_move(|i, d| {
        with(move |c| {
            edit(c, |rules| {
                let j = i as i64 + d as i64;
                if i >= 0 && (i as usize) < rules.len() && j >= 0 && (j as usize) < rules.len() {
                    rules.swap(i as usize, j as usize);
                }
            });
            show(c);
        })
    });
    g.on_music_edited(|i, f, v| with(move |c| edited(c, i as usize, f.as_str(), v.as_str())));
    g.on_music_fit(|i| with(move |c| fit(c, i as usize)));
    g.on_music_pick(|i| with(move |c| picker::open(c, Target::SceneTracks(i as usize), "Музыка сцены: треки")));
    g.on_music_track_remove(|i, k| {
        with(move |c| {
            edit(c, |rules| {
                if let Some(r) = rules.get_mut(i as usize)
                    && (k as usize) < r.tracks.len()
                {
                    r.tracks.remove(k as usize);
                }
            });
            show(c);
        })
    });
}

fn edit(c: &mut Ctx, f: impl FnOnce(&mut Vec<MusicRule>)) {
    let Some(s) = c.scenes.draft.as_mut().and_then(|m| m.scene.as_mut()) else { return };
    f(&mut s.music);
    touched(c);
}

/// Rules → cards.
pub fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let Some(m) = c.scenes.draft.as_ref() else { return };
    let Some(s) = &m.scene else { return };
    let lib = lb_core::media::Library::new(c.model.library.clone());
    let rows: Vec<MusicRow> = s
        .music
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let tracks: Vec<TrackRow> = r
                .tracks
                .iter()
                .map(|t| match lib.get(t) {
                    Some(m) => TrackRow { id: t.into(), title: super::title(m).into(), duration: hms(m.play_duration()).into() },
                    None => TrackRow { id: t.into(), title: format!("{t} — нет в медиатеке").into(), duration: "".into() },
                })
                .collect();
            let pool = r.pool(&lib);
            let total: i64 = pool.iter().map(|m| m.play_duration()).sum();
            let summary = match pool.len() {
                0 => "нет треков".to_string(),
                n => format!("{n} {} · {}", plural(n), hms(total)),
            };
            let days = (1..=7).map(|d| r.conditions.weekdays.contains(&d)).collect();
            let length = r.length(&lib);
            let scene_ms = m.play_duration();
            let length_text = match (length, r.repeat) {
                (Some(l), _) => format!("Музыка длится {}, сцена — {}{}", hms(l), hms(scene_ms), if l < scene_ms { ": остаток сцены пройдёт в тишине" } else { "" }),
                (None, 0) => String::new(),
                (None, _) => "Длина музыки заранее неизвестна (случайный трек или случайное начало)".into(),
            };
            MusicRow {
                index: i as i32,
                name: r.name.clone().into(),
                enabled: r.enabled,
                tracks: vec_model(tracks),
                folder: r.folder.clone().unwrap_or_default().into(),
                category: r.category.clone().unwrap_or_default().into(),
                order: ORDERS.iter().position(|o| *o == r.order).unwrap_or(0) as i32,
                volume: fmt_db(r.volume_db).into(),
                fade_in: r.fade_in.to_string().into(),
                fade_out: r.fade_out.to_string().into(),
                random_start: r.random_start,
                repeat_mode: match r.repeat {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                },
                repeat_times: if r.repeat > 1 { r.repeat.to_string() } else { String::new() }.into(),
                gap: if r.gap.ms() > 0 { r.gap.to_string() } else { String::new() }.into(),
                delay: if r.delay.ms() > 0 { r.delay.to_string() } else { String::new() }.into(),
                length: length_text.into(),
                can_fit: length.is_some_and(|l| l > 0 && (l - scene_ms).abs() >= 1000),
                from: r.conditions.from.map(|t| t.format("%H:%M").to_string()).unwrap_or_default().into(),
                to: r.conditions.to.map(|t| t.format("%H:%M").to_string()).unwrap_or_default().into(),
                days: vec_model(days),
                summary: summary.into(),
            }
        })
        .collect();
    ui.global::<Scenes>().set_music(vec_model(rows));
}

fn plural(n: usize) -> &'static str {
    match (n % 10, n % 100) {
        (1, x) if x != 11 => "трек",
        (2..=4, x) if !(12..=14).contains(&x) => "трека",
        _ => "треков",
    }
}

fn fmt_db(v: f32) -> String {
    if v.fract() == 0.0 { format!("{}", v as i32) } else { format!("{v:.1}") }
}

fn edited(c: &mut Ctx, i: usize, field: &str, v: &str) {
    let mut bad: Option<&str> = None;
    let mut structural = false;
    let v = v.trim().to_string();
    edit(c, |rules| {
        let Some(r) = rules.get_mut(i) else { return };
        match field {
            "enabled" => {
                r.enabled = v == "1";
                structural = true;
            }
            "name" => r.name = v.clone(),
            "folder" => {
                r.folder = (!v.is_empty()).then(|| v.clone());
                structural = true;
            }
            "category" => r.category = (!v.is_empty()).then(|| v.clone()),
            "order" => {
                r.order = v.parse::<usize>().ok().and_then(|k| ORDERS.get(k)).copied().unwrap_or_default();
                structural = true;
            }
            "volume" => match v.replace(',', ".").parse::<f32>() {
                Ok(x) if x.is_finite() && (-60.0..=20.0).contains(&x) => r.volume_db = x,
                _ if v.is_empty() => r.volume_db = 0.0,
                _ => bad = Some("Громкость: число от −60 до 20 дБ"),
            },
            "fade-in" | "fade-out" => match if v.is_empty() { Ok(Dur(0)) } else { v.parse::<Dur>() } {
                Ok(d) if d.ms() >= 0 => {
                    if field == "fade-in" {
                        r.fade_in = d;
                    } else {
                        r.fade_out = d;
                    }
                }
                _ => bad = Some("Нарастание и затухание — время ЧЧ:ММ:СС(.мс)"),
            },
            "repeat-mode" => {
                r.repeat = match v.as_str() {
                    "1" => 1,
                    "2" => r.repeat.max(2),
                    _ => 0,
                };
                structural = true;
            }
            "repeat" => match v.parse::<u32>() {
                Ok(n) if (1..=1000).contains(&n) => r.repeat = n,
                _ if v.is_empty() => r.repeat = 2,
                _ => bad = Some("Сколько раз: число от 1 до 1000"),
            },
            "gap" | "delay" => match if v.is_empty() { Ok(Dur(0)) } else { v.parse::<Dur>() } {
                Ok(d) if d.ms() >= 0 => {
                    if field == "gap" {
                        r.gap = d;
                    } else {
                        r.delay = d;
                    }
                }
                _ => bad = Some("Пауза и задержка — время ЧЧ:ММ:СС(.мс)"),
            },
            "random-start" => {
                r.random_start = v == "1";
                structural = true;
            }
            "day" => {
                if let Ok(k) = v.parse::<u32>() {
                    let d = k + 1;
                    match r.conditions.weekdays.iter().position(|x| *x == d) {
                        Some(p) => {
                            r.conditions.weekdays.remove(p);
                        }
                        None => {
                            r.conditions.weekdays.push(d);
                            r.conditions.weekdays.sort();
                        }
                    }
                }
                structural = true;
            }
            "from" | "to" => match parse_time(&v) {
                Ok(t) => {
                    if field == "from" {
                        r.conditions.from = t;
                    } else {
                        r.conditions.to = t;
                    }
                }
                Err(_) => bad = Some("Время в формате ЧЧ:ММ"),
            },
            _ => {}
        }
    });
    if let Some(msg) = bad {
        c.toast(msg, true);
    }
    // typed fields keep their text and cursor
    if structural || field == "category" && v.is_empty() {
        show(c);
    }
}

/// Snapshots: a rule playing `tracks` three times with pauses.
#[cfg(feature = "snapshot")]
pub fn demo(c: &mut Ctx, tracks: Vec<String>) {
    edit(c, |rules| rules.push(MusicRule { name: "Музыка".into(), tracks, repeat: 3, gap: Dur(5000), ..Default::default() }));
    show(c);
}

/// The scene lasts as long as the music of rule `i`.
fn fit(c: &mut Ctx, i: usize) {
    let lib = lb_core::media::Library::new(c.model.library.clone());
    let Some(m) = c.scenes.draft.as_mut() else { return };
    let Some(len) = m.scene.as_ref().and_then(|s| s.music.get(i)).and_then(|r| r.length(&lib)) else { return };
    m.duration = Dur(len);
    touched(c);
    super::form::show(c);
    show(c);
}

pub fn tracks_picked(c: &mut Ctx, i: usize, ids: Vec<String>) {
    let scenes: Vec<String> = c.model.library.iter().filter(|m| m.is_scene()).map(|m| m.id.clone()).collect();
    edit(c, |rules| {
        if let Some(r) = rules.get_mut(i) {
            for id in ids.into_iter().filter(|id| !scenes.contains(id)) {
                if !r.tracks.contains(&id) {
                    r.tracks.push(id);
                }
            }
        }
    });
    show(c);
}
