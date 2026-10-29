//! Bridge between the API client (`net`, `model`) and the Slint UI.
//!
//! All state lives in one [`Ctx`] owned by the UI thread (thread-local).
//! Background threads only send [`Msg`]s and wake the event loop; the pump
//! applies them to the model and pushes view data into the Slint globals.
//! UI callbacks go through [`with`], which defers itself if the context is
//! already borrowed (a callback fired while Rust was updating the UI).
//!
//! One module per area: [`shell`] (window, navigation, status, overlays),
//! [`air`] (home + on-air), [`playlists`], [`library`], [`schedule`] (grid),
//! [`rules`] (automation), [`graphics`] (on-screen graphics editor),
//! [`thumbs`] (media thumbnails).

pub mod air;
pub mod archive;
pub mod avatars;
pub mod channels;
pub mod download;
pub mod editor;
pub mod encoder;
pub mod folders;
mod forms;
pub mod graphics;
pub mod jobs;
pub mod library;
pub mod log;
pub mod media_display;
pub mod notifications;
pub mod picker;
pub mod playlists;
pub mod record;
pub mod rules;
pub mod scenes;
pub mod schedule;
pub mod settings;
pub mod shell;
pub mod sources;
pub mod stats;
pub mod system;
pub mod thumbs;
pub mod update;

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use serde_json::{Value, json};
use slint::ComponentHandle;

use crate::GuiOptions;
use crate::model::{LogLine, Model};
use crate::net::api::Api;
use crate::net::poller::Poller;
use crate::net::worker::Worker;
use crate::net::{Msg, Wake, events};
use crate::prefs::UiPrefs;
use crate::shell::{App, AppWindow};

type Action = Box<dyn FnOnce(&mut Ctx)>;

/// Everything the UI thread knows.
pub struct Ctx {
    pub ui: slint::Weak<AppWindow>,
    pub model: Model,
    pub worker: Worker,
    pub poller: Poller,
    pub prefs: UiPrefs,
    rx: Receiver<Msg>,
    pending_confirm: Option<Action>,
    toast_action: Option<Action>,
    toast_serial: i32,
    /// Scale factor reported by the OS (before the user's UI scale).
    pub system_scale: f32,
    pub thumbs: thumbs::Thumbs,
    /// (on-air uid, next uid) the queue was last marked with.
    pub air_marks: (Option<String>, Option<String>),
    pub pl: playlists::PlaylistsState,
    pub lib: library::LibraryState,
    pub download: download::DownloadState,
    /// Video editor.
    pub ed: editor::EditorState,
    pub grid: schedule::ScheduleState,
    pub rules: rules::RulesState,
    pub gfx: graphics::GraphicsState,
    pub log: log::LogState,
    pub stats: stats::StatsState,
    pub settings: settings::SettingsState,
    pub src: sources::SourcesState,
    pub picker: picker::PickerState,
    pub scenes: scenes::ScenesState,
    pub notify: notifications::NotifyState,
    pub avatars: avatars::Avatars,
    pub system: system::SystemState,
    pub jobs: jobs::JobsState,
    /// New versions from GitHub.
    pub upd: update::UpdateState,
    /// The notification-area icon is up (closing hides the window).
    pub has_tray: bool,
    /// Channel configuration (admin only; `None` when not available).
    pub channel_config: Option<Value>,
    /// When [`Ctx::load_all`] last ran.
    last_load: std::cell::Cell<Option<std::time::Instant>>,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

/// Run `f` with the context; if it is busy, retry on the next event-loop turn.
pub fn with(f: impl FnOnce(&mut Ctx) + 'static) {
    let f = RefCell::new(Some(f));
    let ran = CTX.with(|c| match c.try_borrow_mut() {
        Ok(mut g) => {
            if let (Some(ctx), Some(f)) = (g.as_mut(), f.borrow_mut().take()) {
                f(ctx);
            }
            true
        }
        Err(_) => false,
    });
    if !ran && let Some(f) = f.into_inner() {
        slint::Timer::single_shot(std::time::Duration::ZERO, move || with(f));
    }
}

/// Create the context and connect everything (called once, on the UI thread).
pub fn start(ui: &AppWindow, opts: &GuiOptions, prefs: UiPrefs) {
    let api = Api::new(&opts.base_url, opts.token.clone());
    // Coalesce wake-ups: one pending pump at a time.
    let queued = Arc::new(AtomicBool::new(false));
    let wake: Wake = Arc::new(move || {
        if !queued.swap(true, Ordering::AcqRel) {
            let q = queued.clone();
            let _ = slint::invoke_from_event_loop(move || {
                q.store(false, Ordering::Release);
                with(|c| c.pump());
            });
        }
    });
    let (worker, rx) = Worker::new(api.clone(), wake.clone());
    let poller = Poller::start(api.clone(), worker.sender(), wake.clone());
    events::start(&api, worker.sender(), wake);
    let ctx = Ctx {
        ui: ui.as_weak(),
        model: Model { channel: opts.channel.clone(), ..Default::default() },
        worker,
        poller,
        prefs,
        rx,
        pending_confirm: None,
        toast_action: None,
        toast_serial: 0,
        system_scale: ui.window().scale_factor(),
        thumbs: thumbs::Thumbs::default(),
        air_marks: (None, None),
        pl: Default::default(),
        lib: Default::default(),
        download: Default::default(),
        ed: Default::default(),
        grid: Default::default(),
        rules: Default::default(),
        gfx: Default::default(),
        log: Default::default(),
        stats: Default::default(),
        settings: Default::default(),
        src: Default::default(),
        picker: Default::default(),
        scenes: Default::default(),
        avatars: Default::default(),
        notify: Default::default(),
        system: Default::default(),
        jobs: Default::default(),
        upd: update::UpdateState::new(opts.embedded_engine),
        has_tray: false,
        channel_config: None,
        last_load: Default::default(),
    };
    ui.global::<App>().set_api_url(api.base.clone().into());
    CTX.with(|c| *c.borrow_mut() = Some(ctx));
    forms::connect(ui);
    shell::connect(ui);
    channels::connect(ui);
    jobs::connect(ui);
    air::connect(ui);
    playlists::connect(ui);
    library::connect(ui);
    download::connect(ui);
    editor::connect(ui);
    media_display::connect(ui);
    folders::connect(ui);
    schedule::connect(ui);
    rules::connect(ui);
    graphics::connect(ui);
    log::connect(ui);
    stats::connect(ui);
    system::connect(ui);
    sources::connect(ui);
    scenes::connect(ui);
    encoder::connect(ui);
    record::connect(ui);
    settings::connect(ui);
    picker::connect(ui);
    #[cfg(not(target_os = "android"))]
    update::connect(ui);
    #[cfg(target_os = "android")]
    update::connect_android(ui);
    with(|c| {
        forms::refresh(c);
        shell::apply_prefs(c);
        settings::apply_preview_rate(c);
        air::apply_prefs(c);
        playlists::apply_prefs(c);
        library::apply_prefs(c);
        c.load_channels();
    });
}

/// Save preferences and drop the context (window closed).
/// Tray icon state; decides what the close button does.
pub fn set_tray(on: bool) {
    with(move |c| {
        c.has_tray = on;
        tray_refresh(c);
        if let Some(ui) = c.window() {
            ui.global::<crate::shell::Settings>().set_has_tray(on);
        }
    });
}

/// Menu of the tray icon from the current status.
pub fn tray_refresh(c: &Ctx) {
    if !c.has_tray {
        return;
    }
    let s = &c.model.status;
    let on_air = c.model.online && matches!(s.state.as_str(), "on_air" | "starting" | "reconnecting");
    let title = s.on_air.as_ref().and_then(|o| o.item.as_ref()).map(|i| i.title.clone()).unwrap_or_default();
    let status = if !c.model.online {
        "Нет связи с сервером".to_string()
    } else {
        match s.state.as_str() {
            "on_air" if !title.is_empty() => format!("● В эфире: {title}"),
            "on_air" => "● В эфире".into(),
            "starting" => "Запуск вещания…".into(),
            "reconnecting" => "Переподключение…".into(),
            "error" => "Ошибка вещания".into(),
            _ => "Вещание остановлено".into(),
        }
    };
    crate::tray::update(crate::tray::TrayState {
        status: if s.manual && on_air { format!("{status} (ручной режим)") } else { status },
        online: c.model.online,
        on_air,
        manual: s.manual,
        close_to_tray: c.prefs.close_to_tray,
    });
}

/// Flip "close into the tray" (tray menu) and show it in Settings.
pub fn toggle_close_to_tray() {
    with(|c| {
        c.prefs.close_to_tray = !c.prefs.close_to_tray;
        c.prefs.save();
        if let Some(ui) = c.window() {
            ui.global::<crate::shell::Settings>().set_close_to_tray(c.prefs.close_to_tray);
        }
        tray_refresh(c);
    });
}

/// Hide into the tray instead of quitting?
pub fn close_to_tray() -> bool {
    CTX.with(|c| c.try_borrow().ok().and_then(|g| g.as_ref().map(|c| c.has_tray && c.prefs.close_to_tray)).unwrap_or(false))
}

pub fn stop() {
    CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().take() {
            ctx.prefs.save();
        }
    });
}

impl Ctx {
    pub fn window(&self) -> Option<AppWindow> {
        self.ui.upgrade()
    }

    /// Id of the open section.
    pub fn section(&self) -> String {
        self.window().map(|ui| ui.global::<App>().get_section().to_string()).unwrap_or_default()
    }

    /// Apply queued background results.
    fn pump(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            self.apply(m);
        }
    }

    fn apply(&mut self, m: Msg) {
        match m {
            Msg::Scoped(epoch, msg) => {
                if epoch == self.worker.epoch() {
                    self.apply(*msg);
                }
            }
            Msg::ForChannel(ch, revision, msg) => {
                if self.model.channel.as_deref() == Some(ch.as_str()) && self.poller.revision() == revision {
                    self.apply(*msg);
                }
            }
            Msg::Channels(info) => {
                let list: Vec<String> = info.iter().map(|c| c.id.clone()).collect();
                self.model.channel_info = info;
                if self.model.channel.as_ref().is_none_or(|c| !list.contains(c)) {
                    self.model.channel = list.first().cloned();
                }
                self.model.channels = list;
                match self.model.channel.clone() {
                    Some(c) => self.channel_selected(&c),
                    None => self.channel_cleared(),
                }
                // only a list the server sent opens the start screen (not
                // the empty one before the first answer)
                if let Some(ui) = self.window() {
                    ui.global::<App>().set_no_channels(self.model.channels.is_empty());
                }
                shell::channels_changed(self);
                channels::refresh(self);
                self.load_all();
            }
            Msg::Status(s) => {
                let schedule_changed = self.model.status.force_schedule != s.force_schedule;
                self.model.status = *s;
                shell::status_changed(self);
                air::status_changed(self);
                sources::refresh(self);
                notifications::status(self);
                if schedule_changed {
                    air::config_changed(self);
                }
            }
            // polled / reloaded often: rebuilding big list models when nothing
            // changed made the whole interface sluggish
            Msg::Rundown(r) => {
                if r == self.model.rundown {
                    return;
                }
                self.model.rundown = r;
                air::queue_changed(self);
            }
            Msg::Library(l) => {
                if self.lib.loaded && l == self.model.library {
                    return;
                }
                self.model.library = l;
                self.lib.loaded = true;
                library::refresh(self);
                air::media_results(self);
                playlists::lists_changed(self);
                playlists::rows_changed(self);
                playlists::add_results(self);
                scenes::refresh(self);
                graphics::scope::library_changed(self);
            }
            Msg::Playlists(p) => {
                self.model.playlists = p;
                playlists::reloaded(self);
                library::refresh(self);
                schedule::refresh(self);
            }
            Msg::Schedules(s) => {
                self.model.schedules = s;
                forms::refresh(self);
                air::config_changed(self);
                schedule::reloaded(self);
            }
            Msg::Rules(r) => {
                self.model.rules = r;
                forms::refresh(self);
                air::config_changed(self);
                schedule::refresh(self);
                rules::reloaded(self);
            }
            Msg::Graphics(g) => {
                self.model.graphics = g;
                forms::refresh(self);
                air::status_changed(self);
                graphics::reloaded(self);
                scenes::graphics_changed(self);
            }
            Msg::Preview(f) => {
                shell::preview_changed(self, f);
                graphics::preview_changed(self);
            }
            Msg::Thumb(id, f) => {
                thumbs::arrived(self, id.clone(), f);
                air::status_changed(self);
                match self.section().as_str() {
                    "playlists" => playlists::rows_changed(self),
                    "library" => library::thumb_arrived(self, &id),
                    "scenes" => scenes::thumb_arrived(self),
                    _ => {}
                }
            }
            Msg::Event(v) => {
                notifications::event(self, &v);
                self.model.push_log(LogLine::from_event(&v));
                air::log_changed(self);
                log::changed(self);
            }
            Msg::Done(text) => self.toast(text, false),
            Msg::Error(e) => {
                self.model.push_log(LogLine {
                    at: chrono::Local::now().format("%H:%M:%S").to_string(),
                    level: "warning".into(),
                    text: e.clone(),
                });
                air::log_changed(self);
                log::changed(self);
                self.toast(e, true);
            }
            Msg::Apply(f) => f(self),
            Msg::Jobs(list) => jobs::changed(self, list),
            Msg::Connection(ok) => {
                notifications::connection(self, ok);
                self.model.online = ok;
                shell::status_changed(self);
                if self.section() == "settings" {
                    settings::show(self);
                }
                if ok && self.model.channels.is_empty() {
                    self.load_channels();
                }
            }
        }
    }

    /// Point polling and the editors at channel `ch` (drafts of another
    /// channel must not stay open).
    pub fn channel_selected(&mut self, ch: &str) {
        if self.poller.channel().as_deref() != Some(ch) {
            self.worker.invalidate();
            self.pl = Default::default();
            self.gfx = Default::default();
            self.lib.checked.clear();
            self.thumbs.clear();
            self.lib.loaded = false;
            self.rules = Default::default();
            self.model.status = Default::default();
            self.model.rundown = Default::default();
            self.model.graphics = Default::default();
            self.air_marks = (None, None);
            self.clear_preview();
            air::status_changed(self);
            air::queue_changed(self);
        }
        self.poller.set_channel(ch);
    }

    /// The project has no channels (first start, or the last one was
    /// deleted): stop polling and forget the data of the old channel.
    fn channel_cleared(&mut self) {
        self.worker.invalidate();
        self.clear_preview();
        self.poller.clear_channel();
        self.pl = Default::default();
        self.gfx = Default::default();
        self.lib.checked.clear();
        self.lib.loaded = false;
        self.thumbs.clear();
        self.model.status = Default::default();
        self.model.rundown = Default::default();
        air::status_changed(self);
        air::queue_changed(self);
        if let Some(ui) = self.window() {
            ui.global::<crate::shell::Chan>().set_open(false);
        }
    }

    fn clear_preview(&self) {
        if let Some(ui) = self.window() {
            let app = ui.global::<App>();
            app.set_has_preview(false);
            app.set_preview(slint::Image::default());
            let air = ui.global::<crate::shell::Air>();
            air.set_has_next_thumb(false);
            air.set_next_thumb(slint::Image::default());
            air.set_detail_open(false);
        }
    }

    // ------------------------------------------------------------ requests

    pub fn load_channels(&self) {
        self.worker.spawn(|api| {
            let list: Vec<Value> = api.get("/channels")?;
            Ok(Msg::Channels(list.iter().filter_map(crate::model::ChannelEntry::from_json).collect()))
        });
    }

    pub fn loaded_within(&self, d: std::time::Duration) -> bool {
        self.last_load.get().is_some_and(|t| t.elapsed() < d)
    }

    /// Reload all editable documents of the channel.
    pub fn load_all(&self) {
        let ch = self.model.ch();
        if ch.is_empty() {
            return;
        }
        self.last_load.set(Some(std::time::Instant::now()));
        let c = ch.clone();
        self.worker.spawn(move |api| Ok(Msg::Library(api.get(&format!("/channels/{c}/media"))?)));
        folders::load(self);
        let c = ch.clone();
        self.worker.spawn(move |api| {
            let list: Vec<Value> = api.get(&format!("/channels/{c}/playlists"))?;
            let mut full = Vec::new();
            for p in list {
                full.push(api.get(&format!("/channels/{c}/playlists/{}", p["id"].as_str().unwrap_or("")))?);
            }
            Ok(Msg::Playlists(full))
        });
        let c = ch.clone();
        self.worker.spawn(move |api| Ok(Msg::Schedules(api.get(&format!("/channels/{c}/schedules"))?)));
        let c = ch.clone();
        self.worker.spawn(move |api| Ok(Msg::Rules(api.get(&format!("/channels/{c}/rules"))?)));
        let c = ch.clone();
        self.worker.spawn(move |api| Ok(Msg::Graphics(api.get(&format!("/channels/{c}/graphics"))?)));
        // needs the admin role; without it the GUI falls back to defaults
        let c = ch;
        self.worker.spawn(move |api| {
            let cfg: Option<Value> = api.get(&format!("/channels/{c}/config")).ok();
            Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
                c.channel_config = cfg;
                settings::show(c);
                if c.section() == "graphics" {
                    graphics::refresh(c);
                }
                sources::refresh(c);
            })))
        });
    }

    /// Send a channel command (`{"cmd": ...}`).
    pub fn command(&self, cmd: Value) {
        let ch = self.model.ch();
        self.worker.spawn(move |api| {
            let r = api.post(&format!("/channels/{ch}/command"), &cmd)?;
            let status = api.get(&format!("/channels/{ch}/status"))?;
            Ok(Msg::Apply(Box::new(move |c| {
                c.apply(Msg::Status(Box::new(status)));
                c.toast(r["message"].as_str().map(crate::ru::server).unwrap_or_else(|| "Выполнено".into()), false);
            })))
        });
    }

    /// Save a whole document with PUT and reload everything afterwards.
    pub fn save_doc<T: serde::Serialize + Send + 'static>(&self, path: &str, doc: T, what: &'static str) {
        let ch = self.model.ch();
        let path = format!("/channels/{ch}{path}");
        self.change(move |api| {
            api.put(&path, &doc)?;
            Ok(format!("{what}: сохранено"))
        });
    }

    /// Run a modifying request; once it has finished, show its message and
    /// reload the channel documents (reloading in parallel could read the
    /// old version, so new data showed up only after reopening the screen).
    pub fn change(&self, job: impl FnOnce(&Api) -> Result<String, String> + Send + 'static) {
        self.worker.spawn(move |api| {
            let r = job(api);
            Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
                match r {
                    Ok(text) => c.toast(text, false),
                    Err(e) => c.apply(Msg::Error(e)),
                }
                // after an error too: the server may have applied part of a batch
                c.load_all();
            })))
        });
    }

    // ------------------------------------------------------------ overlays

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toast_action = None;
        self.show_toast(text.into(), error, "");
    }

    /// Snackbar with an action button (e.g. "Отменить").
    pub fn toast_with(&mut self, text: impl Into<String>, label: &str, action: impl FnOnce(&mut Ctx) + 'static) {
        self.toast_action = Some(Box::new(action));
        self.show_toast(text.into(), false, label);
    }

    fn show_toast(&mut self, text: String, error: bool, action: &str) {
        let Some(ui) = self.window() else { return };
        self.toast_serial += 1;
        let g = ui.global::<App>();
        g.set_toast_text(text.into());
        g.set_toast_error(error);
        g.set_toast_action(action.into());
        g.set_toast_serial(self.toast_serial);
        g.set_toast_shown(true);
    }

    /// The server runs on this computer (its paths are our paths, so file
    /// dialogs make sense).
    pub fn server_is_local(&self) -> bool {
        let base = self.worker.api().base.to_lowercase();
        base.contains("://127.0.0.1") || base.contains("://localhost") || base.contains("://[::1]")
    }

    /// Ask before running a risky action.
    pub fn ask(&mut self, title: &str, text: &str, action_label: &str, action: impl FnOnce(&mut Ctx) + 'static) {
        let Some(ui) = self.window() else { return };
        self.pending_confirm = Some(Box::new(action));
        let g = ui.global::<App>();
        g.set_confirm_option("".into());
        g.set_confirm_title(title.into());
        g.set_confirm_text(text.into());
        g.set_confirm_action(action_label.into());
        g.set_confirm_danger(true);
        g.set_confirm_open(true);
    }

    /// [`Ctx::ask`] with a checkbox; the action gets its state.
    pub fn ask_with_option(
        &mut self,
        title: &str,
        text: &str,
        action_label: &str,
        option: &str,
        action: impl FnOnce(&mut Ctx, bool) + 'static,
    ) {
        self.ask(title, text, action_label, move |c| {
            let checked = c.window().is_some_and(|ui| ui.global::<App>().get_confirm_option_checked());
            action(c, checked)
        });
        if let Some(ui) = self.window() {
            let g = ui.global::<App>();
            g.set_confirm_option(option.into());
            g.set_confirm_option_checked(false);
        }
    }

    fn confirm_result(&mut self, ok: bool) {
        if let Some(a) = self.pending_confirm.take()
            && ok
        {
            a(self);
        }
    }

    fn toast_action_clicked(&mut self) {
        if let Some(a) = self.toast_action.take() {
            a(self);
        }
    }
}

/// Slint list model from a vector.
pub fn vec_model<T: Clone + 'static>(v: Vec<T>) -> slint::ModelRc<T> {
    slint::ModelRc::new(slint::VecModel::from(v))
}

/// JSON command helper.
pub fn cmd(name: &str) -> Value {
    json!({ "cmd": name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Frame;
    use crate::shell::Air;
    use slint::Model as _;

    #[test]
    fn channel_switch_discards_old_frames_and_graphic_rows_update_live() {
        struct Headless;
        impl slint::platform::Platform for Headless {
            fn create_window_adapter(&self) -> Result<std::rc::Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
                Ok(slint::platform::software_renderer::MinimalSoftwareWindow::new(
                    slint::platform::software_renderer::RepaintBufferType::NewBuffer,
                ))
            }
        }
        slint::platform::set_platform(Box::new(Headless)).unwrap();
        let ui = AppWindow::new().unwrap();
        start(
            &ui,
            &GuiOptions { base_url: "http://127.0.0.1:1".into(), token: None, channel: None, embedded_engine: false },
            UiPrefs { persist: false, ..Default::default() },
        );
        with(|c| {
            let ui = c.window().unwrap();
            ui.window().set_size(slint::PhysicalSize::new(1400, 860));
            let original = ui.window().size();
            let base = c.system_scale;
            for scale in [1.25, 1.5, 2.0, 1.0, 1.25, 1.0] {
                c.prefs.scale = scale;
                shell::apply_scale(c);
                assert!((ui.window().scale_factor() - base * scale).abs() < 0.001);
                assert_eq!(ui.window().size(), original, "UI zoom must not resize the native viewport");
                assert_eq!(c.system_scale, base, "user zoom must never become the OS DPI");
            }
            c.model.channel = Some("old".into());
            c.channel_selected("old");
            let old_epoch = c.worker.epoch();
            let old_revision = c.poller.revision();
            let frame = || Frame { width: 1, height: 1, rgba: vec![255, 0, 0, 255] };
            c.apply(Msg::Preview(frame()));
            assert!(c.window().unwrap().global::<App>().get_has_preview());
            c.model.channel = Some("new".into());
            c.channel_selected("new");
            assert!(!c.window().unwrap().global::<App>().get_has_preview());
            c.apply(Msg::ForChannel("old".into(), old_revision, Box::new(Msg::Preview(frame()))));
            c.apply(Msg::Scoped(old_epoch, Box::new(Msg::Preview(frame()))));
            assert!(!c.window().unwrap().global::<App>().get_has_preview());
            c.apply(Msg::Graphics(
                serde_json::from_value(json!({"elements":[{"id":"logo","type":"rect","width":20,"height":20,"color":"#ffffff"}]})).unwrap(),
            ));
            let mut status = crate::model::StatusView::default();
            status.graphics.push(lb_core::graphics::ActiveGraphic {
                element: "logo".into(),
                layer: 0,
                rule: None,
                priority: 0,
                shown_ms: 0,
                hide_in_ms: None,
            });
            c.apply(Msg::Status(Box::new(status)));
            assert!(c.window().unwrap().global::<Air>().get_graphics().row_data(0).unwrap().on);
            c.apply(Msg::Status(Box::default()));
            assert!(!c.window().unwrap().global::<Air>().get_graphics().row_data(0).unwrap().on);
        });
        stop();
    }
}
