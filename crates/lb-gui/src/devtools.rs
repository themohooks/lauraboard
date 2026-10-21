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
    let ui = AppWindow::new().map_err(|e| e.to_string())?;
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
    window.set_size(slint::PhysicalSize::new(width, height));
    let (name, page) = section.split_once(':').unwrap_or((section, ""));
    ui.set_section(name.into());
    ui.set_gallery_page(page.into());
    // No time passes between frames here, so animated properties would keep stale values.
    ui.global::<Theme>().set_animations(false);
    ui.show().map_err(|e| e.to_string())?;
    match live {
        Some(url) => {
            let opts = crate::GuiOptions { base_url: url.to_string(), token: std::env::var("LB_TOKEN").ok(), channel: None };
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
        }
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
        encoder_detail: "4500 кбит/с · 1.00x · потеряно кадров: 0 · перезапусков: 0".into(),
        plan_warnings: 2,
        last_error: "".into(),
        clock: "09:28:31".into(),
        to_next_program: "00:31:29".into(),
        outputs: "rtmp://live.example/app".into(),
        manual_queue: 0,
        force_schedule: "".into(),
    });
}

fn model<T: Clone + 'static>(v: Vec<T>) -> slint::ModelRc<T> {
    slint::ModelRc::new(slint::VecModel::from(v))
}

fn row(start: &str, dur: &str, title: &str, kind: &str, block: &str) -> QueueRow {
    QueueRow {
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
        GfxRow { id: "ticker".into(), name: "Бегущая строка".into(), kind: "бегущая строка".into(), layer: "20".into(), on: false },
    ]));
    a.set_recent_events(model(vec![
        LogRow { at: "09:12:40".into(), level: "warning".into(), text: "[main] 'Новости' decoder restarted".into() },
        LogRow { at: "08:55:02".into(), level: "critical".into(), text: "[main] encoder stopped: rtmp connection reset".into() },
    ]));
}

/// Put a screen into a representative state for review (live mode).
fn prepare_live(section: &str) {
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
            if std::env::var("LB_SNAPSHOT_WIZARD").is_ok() {
                crate::bridge::rules::new_rule(c);
            }
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
