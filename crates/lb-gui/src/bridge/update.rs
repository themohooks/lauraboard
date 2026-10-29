//! Updates: quiet daily check, the app bar mark and the Settings card.
//! Network work runs on its own thread (GitHub can be slow; the API worker
//! stays free).

use std::path::PathBuf;

use slint::ComponentHandle;

use super::{Ctx, with};
use crate::shell::{AppWindow, Upd};
use crate::update::release::{self, Kind, Release};
use crate::update::{self as upd, fetch, local};

#[derive(Debug, Clone, PartialEq, Default)]
enum Phase {
    #[default]
    Idle,
    Checking,
    Latest,
    Available,
    Downloading,
    Ready(PathBuf),
    Error(String),
}

pub struct UpdateState {
    phase: Phase,
    kind: Kind,
    latest: Option<Release>,
    progress: f32,
    /// The engine runs in this process: quitting stops the broadcast.
    embedded_engine: bool,
    timer: slint::Timer,
}

impl UpdateState {
    pub fn new(embedded_engine: bool) -> Self {
        UpdateState {
            phase: Phase::Idle,
            kind: local::detect(),
            latest: None,
            progress: 0.0,
            embedded_engine,
            timer: slint::Timer::default(),
        }
    }
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Upd>();
    g.on_check(|| with(|c| check(c, false)));
    g.on_install(|| with(install));
    g.on_skip(|| {
        with(|c| {
            if let Some(r) = &c.upd.latest {
                c.prefs.updates.skipped = r.version.clone();
                c.prefs.save();
            }
            show(c);
        })
    });
    g.on_open_page(|| {
        with(|c| {
            let page =
                c.upd.latest.as_ref().map(|r| r.page.clone()).unwrap_or_else(|| format!("https://github.com/{}/releases", release::REPO));
            if let Err(e) = local::open(page) {
                c.toast(e, true);
            }
        })
    });
    g.on_auto_changed(|v| {
        with(move |c| {
            c.prefs.updates.auto = v;
            c.prefs.save();
            show(c);
        })
    });
    g.on_show(|| with(|c| super::shell::navigate(c, "settings")));

    // first quiet look a while after start, then hourly "is a check due?"
    slint::Timer::single_shot(upd::FIRST_CHECK, || with(quiet));
    with(|c| {
        c.upd.timer.start(slint::TimerMode::Repeated, upd::TICK, || with(quiet));
        show(c);
    });
}

#[cfg(target_os = "android")]
pub fn connect_android(ui: &AppWindow) {
    let g = ui.global::<Upd>();
    let unavailable = || with(|c| c.toast("Обновление Android APK выполняется вручную. Установите новую debug-сборку.", true));
    g.on_check(unavailable);
    g.on_install(unavailable);
    g.on_open_page(unavailable);
    g.set_current(upd::current().into());
    g.set_status("Экспериментальная Android-сборка · обновление вручную".into());
    g.set_auto(false);
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The daily check, if it is on and due.
fn quiet(c: &mut Ctx) {
    if c.prefs.updates.auto
        && upd::due(c.prefs.updates.last_check, now())
        && matches!(c.upd.phase, Phase::Idle | Phase::Latest | Phase::Error(_))
    {
        check(c, true);
    }
}

fn check(c: &mut Ctx, quiet: bool) {
    if matches!(c.upd.phase, Phase::Checking | Phase::Downloading) {
        return;
    }
    let before = std::mem::replace(&mut c.upd.phase, Phase::Checking);
    show(c);
    std::thread::spawn(move || {
        let result = fetch::latest();
        let _ = slint::invoke_from_event_loop(move || with(move |c| checked(c, result, quiet, before)));
    });
}

fn checked(c: &mut Ctx, result: Result<Option<Release>, String>, quiet: bool, before: Phase) {
    match result {
        Ok(latest) => {
            c.prefs.updates.last_check = Some(now());
            c.prefs.save();
            let newer = latest.filter(|r| release::newer(&r.version, upd::current()));
            // a file downloaded earlier for the same version stays usable
            let same = matches!((&before, &newer, &c.upd.latest), (Phase::Ready(_), Some(n), Some(o)) if n.version == o.version);
            c.upd.phase = match &newer {
                Some(_) if same => before,
                Some(_) => Phase::Available,
                None => Phase::Latest,
            };
            c.upd.latest = newer;
        }
        Err(e) => {
            // a quiet check that failed leaves no trace beyond the status line
            if !quiet {
                c.toast(format!("Не удалось проверить обновления: {e}"), true);
            }
            // a version found earlier (or its download) stays offered
            c.upd.phase =
                if c.upd.latest.is_some() && matches!(before, Phase::Available | Phase::Ready(_)) { before } else { Phase::Error(e) };
        }
    }
    show(c);
}

/// "Download and install" / "Install and restart" / "Open the page".
fn install(c: &mut Ctx) {
    let Some(r) = c.upd.latest.clone() else { return };
    match c.upd.phase.clone() {
        Phase::Ready(file) => {
            let kind = c.upd.kind.clone();
            let quits = matches!(kind, Kind::Setup | Kind::AppImage(_));
            let on_air = c.model.online && matches!(c.model.status.state.as_str(), "on_air" | "starting" | "reconnecting");
            if quits && c.upd.embedded_engine && on_air {
                c.ask(
                    "Установить и прервать эфир?",
                    "Вещание идёт из этой программы: на время установки трансляция остановится. Лучше обновиться между передачами.",
                    "Установить",
                    move |c| apply(c, &kind, &file),
                );
            } else {
                apply(c, &kind, &file);
            }
        }
        Phase::Downloading => {}
        _ => {
            let kind = c.upd.kind.clone();
            let asset = release::pick(&r, &kind, std::env::consts::OS, std::env::consts::ARCH).cloned();
            let Some(asset) = asset else {
                // nothing to install here (macOS, an unpublished build): the page
                if let Err(e) = local::open(&r.page) {
                    c.toast(e, true);
                }
                return;
            };
            c.upd.phase = Phase::Downloading;
            c.upd.progress = 0.0;
            show(c);
            let dest = local::destination(&kind, &asset);
            std::thread::spawn(move || {
                let mut shown = 0.0f32;
                let result = fetch::download(&r, &asset, &dest, |p| {
                    if p - shown >= 0.01 || p >= 1.0 {
                        shown = p;
                        let _ = slint::invoke_from_event_loop(move || {
                            with(move |c| {
                                c.upd.progress = p;
                                if let Some(ui) = c.window() {
                                    ui.global::<Upd>().set_progress(p);
                                }
                            })
                        });
                    }
                });
                let _ = slint::invoke_from_event_loop(move || with(move |c| downloaded(c, result.map(|_| dest))));
            });
        }
    }
}

fn downloaded(c: &mut Ctx, result: Result<PathBuf, String>) {
    match result {
        Ok(file) => {
            c.upd.phase = Phase::Ready(file);
            let version = c.upd.latest.as_ref().map(|r| r.version.clone()).unwrap_or_default();
            c.toast_with(format!("Версия {version} скачана и проверена"), "Установить", install);
        }
        Err(e) => {
            c.toast(format!("Обновление не скачалось: {e}"), true);
            c.upd.phase = Phase::Error(e);
        }
    }
    show(c);
}

fn apply(c: &mut Ctx, kind: &Kind, file: &std::path::Path) {
    match local::install(kind, file) {
        Ok(true) => {
            c.prefs.save();
            let _ = slint::quit_event_loop();
        }
        Ok(false) => c.toast("Файл обновления открыт", false),
        Err(e) => c.toast(e, true),
    }
}

/// Status line under the card title.
fn status(c: &Ctx) -> String {
    match &c.upd.phase {
        Phase::Checking => "проверка…".into(),
        Phase::Latest => "это последняя версия".into(),
        Phase::Downloading => format!("загрузка {} %", (c.upd.progress * 100.0).round()),
        Phase::Ready(_) => "новая версия скачана".into(),
        Phase::Error(e) => format!("проверить не удалось: {e}"),
        Phase::Idle | Phase::Available => match c.prefs.updates.last_check.and_then(|t| chrono::DateTime::from_timestamp(t, 0)) {
            Some(t) => format!("проверено {}", t.with_timezone(&chrono::Local).format("%d.%m %H:%M")),
            None => String::new(),
        },
    }
}

pub fn show(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Upd>();
    let p = &c.prefs.updates;
    let latest = c.upd.latest.as_ref().map(|r| r.version.clone()).unwrap_or_default();
    let skipped = !latest.is_empty() && p.skipped == latest;
    let has_file =
        c.upd.latest.as_ref().is_some_and(|r| release::pick(r, &c.upd.kind, std::env::consts::OS, std::env::consts::ARCH).is_some());
    let kind = if has_file { c.upd.kind.clone() } else { Kind::Page };
    g.set_current(upd::current().into());
    g.set_phase(
        match c.upd.phase {
            Phase::Idle => "idle",
            Phase::Checking => "checking",
            Phase::Latest => "latest",
            Phase::Available => "available",
            Phase::Downloading => "downloading",
            Phase::Ready(_) => "ready",
            Phase::Error(_) => "error",
        }
        .into(),
    );
    g.set_latest(latest.clone().into());
    g.set_notes(c.upd.latest.as_ref().map(|r| r.notes.clone()).unwrap_or_default().into());
    g.set_status(status(c).into());
    g.set_progress(c.upd.progress);
    g.set_install_hint(local::install_hint(&kind).into());
    g.set_install_label(
        match (&c.upd.phase, &kind) {
            (_, Kind::Page) => "Открыть страницу выпуска",
            (Phase::Ready(_), Kind::Setup | Kind::AppImage(_)) => "Установить и перезапустить",
            (Phase::Ready(_), _) => "Открыть файл",
            (Phase::Downloading, _) => "Загрузка…",
            (_, Kind::Setup | Kind::AppImage(_)) => "Скачать и установить",
            _ => "Скачать",
        }
        .into(),
    );
    g.set_auto(p.auto);
    g.set_skipped(skipped);
    g.set_badge(!latest.is_empty() && !skipped && !matches!(c.upd.phase, Phase::Latest));
}
