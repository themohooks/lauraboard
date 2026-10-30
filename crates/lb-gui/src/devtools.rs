//! Developer tools: headless rendering of the Slint UI for visual review.

use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slint::ComponentHandle;
use slint::Rgb8Pixel;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{EventLoopProxy, Platform, WindowAdapter, WindowEvent};

use crate::shell::{Air, App, AppWindow, GfxRow, LogRow, MediaPick, NextInfo, NowInfo, QueueRow, StatusInfo, Theme, ThemeMode};

type Job = Box<dyn FnOnce() + Send>;

/// Jobs posted with `invoke_from_event_loop`, run by [`pump_for`].
#[derive(Clone, Default)]
struct Jobs(Arc<Mutex<Vec<Job>>>);

impl EventLoopProxy for Jobs {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        Ok(())
    }
    fn invoke_from_event_loop(&self, event: Job) -> Result<(), slint::EventLoopError> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(event);
        Ok(())
    }
}

struct Headless(Rc<MinimalSoftwareWindow>, Jobs);

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        Some(Box::new(self.1.clone()))
    }
}

/// A minimal event loop: run posted jobs and timers for `d`.
fn pump_for(jobs: &Jobs, d: Duration) {
    let until = Instant::now() + d;
    while Instant::now() < until {
        let batch: Vec<Job> = std::mem::take(&mut *jobs.0.lock().unwrap_or_else(|e| e.into_inner()));
        for j in batch {
            j();
        }
        slint::platform::update_timers_and_animations();
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// Render `section` in light and dark themes to `<out>/<section>-{light,dark}.png`.
/// `gallery:<page>` renders one gallery block. With `live` (API base URL) the
/// real bridge runs against that server for a few seconds before rendering;
/// otherwise built-in demo data is shown.
pub fn snapshot(out: &Path, section: &str, width: u32, height: u32, scale: f32, live: Option<&str>) -> Result<(), String> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let jobs = Jobs::default();
    slint::platform::set_platform(Box::new(Headless(window.clone(), jobs.clone()))).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    if section == "notify" {
        return snapshot_notify(out, &window, scale);
    }
    if section == "crash" {
        return snapshot_crash(out, &window, width, height, scale);
    }
    let ui = AppWindow::new().map_err(|e| e.to_string())?;
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
    window.set_size(slint::PhysicalSize::new(width, height));
    let (name, page) = section.split_once(':').unwrap_or((section, ""));
    ui.set_section(if name == "theme-transition" { "settings" } else if name == "graphics-delete" { "graphics" } else { name }.into());
    ui.set_gallery_page(page.into());
    // No time passes between frames here, so animated properties would keep stale values.
    ui.global::<Theme>().set_animations(false);
    // `LB_SNAPSHOT_COMPACT=1`: the compact density
    if std::env::var("LB_SNAPSHOT_COMPACT").is_ok() {
        ui.global::<Theme>().set_compact(true);
        ui.global::<Theme>().set_density(1.0);
    }
    ui.show().map_err(|e| e.to_string())?;
    match live {
        Some(url) => {
            let opts = crate::GuiOptions {
                base_url: url.to_string(),
                token: std::env::var("LB_TOKEN").ok(),
                channel: None,
                embedded_engine: false,
            };
            let prefs = crate::prefs::UiPrefs { section: name.to_string(), persist: false, ..Default::default() };
            crate::bridge::start(&ui, &opts, prefs);
            pump_for(&jobs, Duration::from_secs(6));
            prepare_live(name);
            pump_for(&jobs, Duration::from_secs(2));
            ui.global::<Theme>().set_animations(false);
        }
        None => {
            demo_state(&ui);
            demo_air(&ui);
            demo_overlay(&ui, name);
        }
    }
    // `LB_SNAPSHOT_CLICKS="x,y;x,y,2"`: logical-pixel clicks before rendering
    // (menus and popups only exist after a click); `,2` double-clicks
    for p in std::env::var("LB_SNAPSHOT_CLICKS").unwrap_or_default().split(';') {
        let n: Vec<f32> = p.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let [x, y, rest @ ..] = n.as_slice() else { continue };
        let position = slint::LogicalPosition::new(*x, *y);
        let button = if p.split(',').nth(2) == Some("right") {
            slint::platform::PointerEventButton::Right
        } else {
            slint::platform::PointerEventButton::Left
        };
        window.dispatch_event(WindowEvent::PointerMoved { position });
        for _ in 0..rest.first().map_or(1, |c| *c as usize) {
            window.dispatch_event(WindowEvent::PointerPressed { position, button });
            window.dispatch_event(WindowEvent::PointerReleased { position, button });
        }
        pump_for(&jobs, Duration::from_millis(live.map_or(300, |_| 1500)));
    }
    if name == "theme-transition" {
        ui.global::<Theme>().set_mode(ThemeMode::Light);
        let old = ui.window().take_snapshot().map_err(|e| e.to_string())?;
        ui.global::<Theme>().set_mode(ThemeMode::Dark);
        let new = ui.window().take_snapshot().map_err(|e| e.to_string())?;
        // the software renderer clips to a square; the GPU draws a circle
        crate::theme_transition::start(&ui, old, new, Some((width as f32 - 60.0, 40.0)));
        let mut buf = vec![Rgb8Pixel::default(); (width * height) as usize];
        // the first frame creates the overlay (its clock starts there)
        window.draw_if_needed(|r| {
            r.render(&mut buf, width as usize);
        });
        pump_for(&jobs, Duration::from_millis(200));
        window.request_redraw();
        window.draw_if_needed(|r| {
            r.render(&mut buf, width as usize);
        });
        let raw: Vec<u8> = buf.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        let img = image::RgbImage::from_raw(width, height, raw).ok_or("bad buffer")?;
        img.save(out.join("theme-transition.png")).map_err(|e| e.to_string())?;
        return Ok(());
    }
    for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
        ui.global::<Theme>().set_mode(mode);
        let mut buf = vec![Rgb8Pixel::default(); (width * height) as usize];
        window.request_redraw();
        window.draw_if_needed(|r| {
            r.render(&mut buf, width as usize);
        });
        let raw: Vec<u8> = buf.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        let img = image::RgbImage::from_raw(width, height, raw).ok_or("bad buffer")?;
        let path = out.join(format!("{}-{name}.png", section.replace(':', "-")));
        img.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

/// A long report exercises the resizable, scrollable crash window.
fn snapshot_crash(out: &Path, window: &Rc<MinimalSoftwareWindow>, width: u32, height: u32, scale: f32) -> Result<(), String> {
    let ui = crate::shell::CrashWindow::new().map_err(|e| e.to_string())?;
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
    window.set_size(slint::PhysicalSize::new(width, height));
    ui.set_log_path("C:\\Users\\Operator\\Documents\\LauraBoard\\logs\\session-20261027-120000.log".into());
    let mut log = String::from("Критическая ошибка: thread 'main' panicked at engine.rs:42\n");
    for i in 0..1000 {
        log.push_str(&format!("2026-10-27 12:00:00 INFO Журнал сеанса: запись {i:04} — обработка события вещания\n"));
    }
    ui.set_log(log.into());
    ui.show().map_err(|e| e.to_string())?;
    for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
        ui.global::<Theme>().set_mode(mode);
        let mut buf = vec![Rgb8Pixel::default(); (width * height) as usize];
        window.request_redraw();
        window.draw_if_needed(|r| {
            r.render(&mut buf, width as usize);
        });
        let raw: Vec<u8> = buf.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        let img = image::RgbImage::from_raw(width, height, raw).ok_or("bad buffer")?;
        img.save(out.join(format!("crash-{name}.png"))).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The notification popups (`notify` section) with a sample stack.
fn snapshot_notify(out: &Path, window: &Rc<MinimalSoftwareWindow>, scale: f32) -> Result<(), String> {
    use crate::shell::{NoticeRow, NotifyWindow, Popups};
    let ui = NotifyWindow::new().map_err(|e| e.to_string())?;
    let n = |id, title: &str, body: &str, urgency, initial: &str| NoticeRow {
        id,
        title: title.into(),
        body: body.into(),
        time: "21:04".into(),
        urgency,
        initial: initial.into(),
        ..Default::default()
    };
    let rows = vec![
        n(1, "Главный: в эфире", "Вечерние новости", 0, "Г"),
        n(2, "Главный: точка вещания недоступна", "rtmp://live.example.com/app — соединение отклонено", 1, "Г"),
        n(
            3,
            "Детский: кодировщик остановлен",
            "ffmpeg завершился с ошибкой 1
Эфир прерван, идёт перезапуск.",
            2,
            "Д",
        ),
    ];
    let count = rows.len() as u32;
    ui.global::<Popups>().set_items(model(rows));
    ui.global::<Theme>().set_animations(false);
    let (width, height) = ((380.0 * scale) as u32, ((count as f32 * 96.0 + 12.0) * scale) as u32);
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
    window.set_size(slint::PhysicalSize::new(width, height));
    ui.show().map_err(|e| e.to_string())?;
    for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
        ui.global::<Theme>().set_mode(mode);
        // a neutral "desktop" under the transparent window
        let mut buf = vec![Rgb8Pixel { r: 96, g: 110, b: 128 }; (width * height) as usize];
        window.request_redraw();
        window.draw_if_needed(|r| {
            r.render(&mut buf, width as usize);
        });
        let raw: Vec<u8> = buf.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        let img = image::RgbImage::from_raw(width, height, raw).ok_or("bad buffer")?;
        let path = out.join(format!("notify-{name}.png"));
        img.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

/// Representative live data so screens can be reviewed without a server.
fn demo_state(ui: &AppWindow) {
    let g = ui.global::<App>();
    let channels: Vec<slint::SharedString> = vec!["main".into(), "kids".into()];
    g.set_channels(slint::ModelRc::new(slint::VecModel::from(channels)));
    g.set_channel_index(0);
    g.set_channel_name("main — Основной канал".into());
    g.set_online(true);
    g.set_api_url("http://127.0.0.1:8787".into());
    g.set_status(StatusInfo {
        state: "on_air".into(),
        state_text: "В ЭФИРЕ".into(),
        manual: false,
        encoder_ok: true,
        encoder_text: "h264 · 25.0 fps".into(),
        encoder_detail: "4500 кбит/с · 1.00x · пропуски: 3 · повторы: 2 · отставание: 0.0 с".into(),
        stream_text: "4500 кбит/с".into(),
        stream_detail: "Битрейт видео и звука · обновление каждую секунду".into(),
        stream_ok: true,
        plan_warnings: 2,
        last_error: "".into(),
        clock: "09:28:31".into(),
        to_next_program: "00:31:29".into(),
        outputs: "rtmp://live.example/app".into(),
        manual_queue: 0,
        force_schedule: "".into(),
    });
}

/// Dialog "sections" (`channels`, `channel-form`) over the home screen.
fn demo_overlay(ui: &AppWindow, name: &str) {
    use crate::shell::{Chan, ChannelRow};
    let c = ui.global::<Chan>();
    let r = |id: &str, name: &str, on: bool, auto: bool, cur: bool| ChannelRow {
        id: id.into(),
        name: name.into(),
        state_text: if on { "В эфире" } else { "Остановлен" }.into(),
        on_air: on,
        autostart: auto,
        current: cur,
        initial: name.chars().next().map(|c| c.to_string()).unwrap_or_default().into(),
        ..Default::default()
    };
    c.set_rows(model(vec![r("main", "Основной канал", true, true, true), r("kids", "Детский", false, false, false)]));
    match name {
        "channels" => c.set_open(true),
        "graphics-delete" => {
            let a = ui.global::<App>();
            a.set_confirm_title("Удалить элемент?".into());
            a.set_confirm_text("«Текст» будет удалён вместе с правилами показа. Удаление можно отменить.".into());
            a.set_confirm_action("Удалить".into());
            a.set_confirm_option("Больше не напоминать".into());
            a.set_confirm_open(true);
        }
        "channel-form" => {
            c.set_open(true);
            c.set_form_source("Основной канал".into());
            c.set_e_id("main-2".into());
            c.set_e_name("Основной канал (копия)".into());
            c.set_form("duplicate".into());
        }
        "jobs" => {
            use crate::shell::{JobRow, Jobs};
            let j = ui.global::<Jobs>();
            j.set_running(1);
            j.set_progress(0.42);
            j.set_summary("42 % · Экспорт канала «Основной канал»".into());
            j.set_rows(model(vec![
                JobRow {
                    id: 2,
                    title: "Экспорт канала «Основной канал»".into(),
                    channel: "main".into(),
                    state: "running".into(),
                    text: "42 % · 17/42: media/5f1c….mp4".into(),
                    progress: 0.42,
                },
                JobRow {
                    id: 1,
                    title: "Копирование файлов в медиатеку (3)".into(),
                    channel: "main".into(),
                    state: "done".into(),
                    text: "Готово".into(),
                    progress: 1.0,
                },
            ]));
            j.set_open(true);
        }
        "channel-export" => {
            c.set_form_source("Основной канал".into());
            c.set_x_path("C:/Users/operator/Documents/main-2026-10-23.zip".into());
            c.set_form("export".into());
        }
        "channel-import" => {
            c.set_i_path("C:/Users/operator/Documents/main-2026-10-23.zip".into());
            c.set_i_info(
                "Канал «Основной канал» (main), экспортирован 2026-10-23 17:40:12.
В архиве: настройки, медиатека, плейлисты, сетка, правила, графика; медиафайлов: 42 (3.4 ГБ)."
                    .into(),
            );
            c.set_i_valid(true);
            c.set_e_id("main-2".into());
            c.set_e_name("Основной канал".into());
            c.set_form("import".into());
        }
        // Settings with a new version found (and its mark in the app bar)
        "update" => {
            let u = ui.global::<crate::shell::Upd>();
            u.set_current("0.1.0".into());
            u.set_phase("available".into());
            u.set_latest(env!("CARGO_PKG_VERSION").into());
            u.set_notes(
                "— Скачивание роликов с сайтов в медиатеку\n— Монтаж: обрезка, скрытие логотипа, кадрирование\n— Обновления из программы"
                    .into(),
            );
            u.set_status("проверено 27.10 09:12".into());
            u.set_install_hint(crate::update::local::install_hint(&crate::update::release::Kind::Setup).into());
            u.set_install_label("Скачать и установить".into());
            u.set_badge(true);
            ui.set_section("settings".into());
            return;
        }
        // download dialog: YouTube asks to sign in
        "download-login" => {
            let d = ui.global::<crate::shell::Dl>();
            d.set_tool_checking(false);
            d.set_tool_ok(true);
            d.set_tool_version("2026.09.30".into());
            d.set_url("https://www.youtube.com/watch?v=abc".into());
            d.set_error("[youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users.".into());
            d.set_step("error".into());
            d.set_login_needed(true);
            d.set_login_cookies(true);
            d.set_open(true);
            ui.set_section("library".into());
            return;
        }
        // the video editor over the library
        "editor" => {
            use crate::shell::{EditRegion, Ved};
            let v = ui.global::<Ved>();
            v.set_media_title("Утренний эфир — выпуск 12".into());
            v.set_duration_ms(754_000.0);
            v.set_position_ms(81_500.0);
            v.set_start_ms(12_000.0);
            v.set_end_ms(700_000.0);
            v.set_has_audio(true);
            v.set_audio_start_ms(24_000.0);
            v.set_audio_end_ms(680_000.0);
            v.set_crop_bottom(6.0);
            let r = EditRegion {
                x: 0.82,
                y: 0.05,
                w: 0.14,
                h: 0.1,
                mode: 0,
                strength: 0.5,
                color: slint::Color::from_rgb_u8(0, 0, 0),
                timed: false,
                from_ms: 0.0,
                to_ms: 754_000.0,
            };
            v.set_regions(model(vec![r.clone(), EditRegion { x: 0.04, y: 0.84, w: 0.3, h: 0.08, mode: 2, ..r.clone() }]));
            v.set_selected(0);
            v.set_current(model(vec![r]));
            v.set_can_replace(true);
            v.set_new_title("Утренний эфир — выпуск 12 (ред.)".into());
            v.set_open(true);
            ui.set_section("library".into());
            return;
        }
        // start screen of a project without channels
        "welcome" => {
            let g = ui.global::<crate::shell::App>();
            g.set_no_channels(true);
            g.set_channels(model(Vec::new()));
            g.set_channel_index(-1);
            g.set_channel_name("".into());
            g.set_channel_initial("".into());
        }
        _ => return,
    }
    ui.set_section("home".into());
}

fn model<T: Clone + 'static>(v: Vec<T>) -> slint::ModelRc<T> {
    slint::ModelRc::new(slint::VecModel::from(v))
}

fn row(start: &str, dur: &str, title: &str, kind: &str, block: &str) -> QueueRow {
    QueueRow {
        uid: format!("{start}-{title}").into(),
        media: title.into(),
        start: start.into(),
        end: "".into(),
        duration: dur.into(),
        title: title.into(),
        kind: kind.into(),
        block: block.into(),
        note: "".into(),
        block_start: false,
        current: false,
        next: false,
        main: kind == "program",
    }
}

/// On-air demo data.
fn demo_air(ui: &AppWindow) {
    let a = ui.global::<Air>();
    a.set_now(NowInfo {
        has_item: true,
        title: "Утренние новости".into(),
        kind: "program".into(),
        block: "Утро".into(),
        elapsed: "00:16:26".into(),
        remaining: "00:12:04".into(),
        duration: "00:28:30".into(),
        progress: 0.58,
        deviation: "+3 с".into(),
        deviation_warn: false,
        started: "09:00:00".into(),
        ends: "09:28:30".into(),
        seekable: true,
        duration_s: 1710,
    });
    a.set_next(NextInfo {
        has_item: true,
        title: "Реклама: блок 1".into(),
        kind: "ad".into(),
        start: "09:28:30".into(),
        duration: "00:01:30".into(),
        media: "ad-1".into(),
        program_title: "Утро — Погода".into(),
        program_start: "10:00".into(),
        program_in: "00:31:29".into(),
    });
    let mut q = vec![
        row("09:00:00", "00:28:30", "Утренние новости", "program", "Утро"),
        row("09:28:30", "00:01:30", "Реклама: блок 1", "ad", "Утро"),
        row("09:30:00", "00:00:05", "Заставка канала", "jingle", "Утро"),
        row("09:30:05", "00:00:20", "Анонс: Вечернее шоу", "promo", "Утро"),
        row("09:30:25", "00:29:35", "Документальный фильм", "program", "Утро"),
        row("10:00:00", "00:00:05", "Заставка канала", "jingle", "Погода"),
        row("10:00:05", "00:04:55", "", "gap", "Погода"),
        row("10:05:00", "00:25:00", "Прогноз погоды", "program", "Погода"),
    ];
    q[0].current = true;
    q[0].block_start = true;
    q[1].next = true;
    q[5].block_start = true;
    a.set_queue(model(q));
    a.set_queue_summary("48 элементов · 23:59:10".into());
    a.set_warnings(model(vec!["10:00:05 — пустота 4 мин 55 с в блоке «Погода»".into()]));
    a.set_schedules(model(vec!["авто (по времени)".into(), "main — Основная".into(), "summer — Летняя".into()]));
    a.set_schedule_index(0);
    a.set_ad_breaks(model(vec!["morning — Утренний".into()]));
    a.set_media_results(model(vec![
        MediaPick { id: "news".into(), title: "Новости".into(), kind: "program".into(), duration: "00:28:30".into() },
        MediaPick { id: "jingle-1".into(), title: "Заставка 1".into(), kind: "jingle".into(), duration: "00:00:05".into() },
        MediaPick { id: "promo".into(), title: "Анонс: кино".into(), kind: "promo".into(), duration: "00:00:20".into() },
    ]));
    a.set_graphics(model(vec![
        GfxRow { id: "logo".into(), name: "Логотип".into(), kind: "изображение".into(), layer: "10".into(), on: true },
        GfxRow {
            id: "ticker".into(), name: "Бегущая строка".into(), kind: "бегущая строка".into(), layer: "20".into(), on: false
        },
    ]));
    a.set_recent_events(model(vec![
        LogRow { at: "09:12:40".into(), level: "warning".into(), text: "[main] 'Новости' decoder restarted".into() },
        LogRow { at: "08:55:02".into(), level: "critical".into(), text: "[main] encoder stopped: rtmp connection reset".into() },
    ]));
}

/// Put a screen into a representative state for review (live mode).
fn prepare_live(section: &str) {
    if section == "library"
        && let Some(row) = std::env::var("LB_SNAPSHOT_EDIT").ok().and_then(|v| v.parse().ok())
    {
        crate::bridge::with(move |c| {
            crate::bridge::library::refresh(c);
            if let Some(ui) = c.window() {
                ui.global::<crate::shell::Lib>().invoke_open_edit(row);
            }
        });
    }
    if section == "library" && std::env::var("LB_SNAPSHOT_GRID").is_ok() {
        crate::bridge::with(|c| {
            if let Some(ui) = c.window() {
                ui.global::<crate::shell::Lib>().set_grid(true);
            }
            crate::bridge::library::refresh(c);
        });
    }
    if section == "jingles" || section == "ads" {
        crate::bridge::with(|c| {
            if let Ok(tab) = std::env::var("LB_SNAPSHOT_TAB")
                && let Some(ui) = c.window()
            {
                ui.global::<crate::shell::Rules>().set_tab(tab.parse().unwrap_or(0));
            }
            crate::bridge::rules::select_first(c);
            if let Ok(step) = std::env::var("LB_SNAPSHOT_WIZARD") {
                crate::bridge::rules::new_rule(c);
                if let Some(ui) = c.window() {
                    ui.global::<crate::shell::Rules>().set_wizard_step(step.parse().unwrap_or(0));
                }
            }
            if let Ok(ids) = std::env::var("LB_SNAPSHOT_RULE_ITEMS") {
                let ids = ids.split(',').map(String::from).collect();
                crate::bridge::rules::picked(c, crate::bridge::picker::Target::RuleItems, ids);
            }
            if std::env::var("LB_SNAPSHOT_SPOTS").is_ok() {
                let ids = ["ad-1", "ad-3", "gone"].map(String::from).to_vec();
                crate::bridge::rules::picked(c, crate::bridge::picker::Target::AdItems, ids);
            }
            if std::env::var("LB_SNAPSHOT_PICKER").is_ok() {
                crate::bridge::picker::open(c, crate::bridge::picker::Target::AdItems, "Ролики рекламного блока");
            }
        });
    }
    if section == "scenes" && std::env::var("LB_SNAPSHOT_SELECT").is_ok() {
        crate::bridge::with(|c| {
            let pick = std::env::var("LB_SNAPSHOT_SELECT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
            crate::bridge::scenes::select(c, pick);
            if let Ok(ids) = std::env::var("LB_SNAPSHOT_MUSIC") {
                crate::bridge::scenes::music::demo(c, ids.split(',').map(String::from).collect());
            }
        });
    }
    if section == "graphics" {
        crate::bridge::with(|c| {
            if let Ok(tab) = std::env::var("LB_SNAPSHOT_TAB")
                && let Some(ui) = c.window()
            {
                ui.global::<crate::shell::Gfx>().set_tab(tab.parse().unwrap_or(0));
            }
            let pick = std::env::var("LB_SNAPSHOT_SELECT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
            crate::bridge::graphics::select(c, pick);
            if std::env::var("LB_SNAPSHOT_PLATE_EDITOR").is_ok() {
                crate::bridge::graphics::plate_editor::demo_styles(c, &["Утреннее шоу", "Гость студии", "Погода"]);
                crate::bridge::graphics::plate_editor::open(c);
            }
        });
    }
    if section == "sources" && std::env::var("LB_SNAPSHOT_RECORD").is_ok() {
        crate::bridge::with(|c| {
            if let Some(ui) = c.window() {
                ui.global::<crate::shell::Src>().invoke_record_edit();
            }
        });
    }
    if section == "log"
        && let Ok(tab) = std::env::var("LB_SNAPSHOT_TAB")
    {
        crate::bridge::with(move |c| {
            if let Some(ui) = c.window() {
                ui.global::<crate::shell::Log>().set_tab(tab.parse().unwrap_or(0));
            }
            crate::bridge::log::opened(c);
        });
    }
    if section == "stats"
        && let Ok(view) = std::env::var("LB_SNAPSHOT_TAB")
    {
        crate::bridge::with(move |c| {
            if let Some(ui) = c.window() {
                ui.global::<crate::shell::Stats>().set_view(view.parse().unwrap_or(0));
            }
            crate::bridge::system::opened(c);
        });
    }
    if section == "playlists" {
        crate::bridge::with(|c| {
            if let Some(id) = c.model.playlists.first().map(|p| p.id.clone()) {
                crate::bridge::playlists::open_id(c, id);
            }
        });
    }
}
