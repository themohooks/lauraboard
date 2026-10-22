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
pub mod graphics;
pub mod library;
pub mod log;
pub mod picker;
pub mod playlists;
pub mod rules;
pub mod schedule;
pub mod settings;
pub mod shell;
pub mod sources;
pub mod stats;
pub mod thumbs;

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
    pub grid: schedule::ScheduleState,
    pub rules: rules::RulesState,
    pub gfx: graphics::GraphicsState,
    pub log: log::LogState,
    pub stats: stats::StatsState,
    pub settings: settings::SettingsState,
    pub src: sources::SourcesState,
    pub picker: picker::PickerState,
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
        grid: Default::default(),
        rules: Default::default(),
        gfx: Default::default(),
        log: Default::default(),
        stats: Default::default(),
        settings: Default::default(),
        src: Default::default(),
        picker: Default::default(),
        has_tray: false,
        channel_config: None,
        last_load: Default::default(),
    };
    ui.global::<App>().set_api_url(api.base.clone().into());
    CTX.with(|c| *c.borrow_mut() = Some(ctx));
    shell::connect(ui);
    air::connect(ui);
    playlists::connect(ui);
    library::connect(ui);
    schedule::connect(ui);
    rules::connect(ui);
    graphics::connect(ui);
    log::connect(ui);
    stats::connect(ui);
    sources::connect(ui);
    settings::connect(ui);
    picker::connect(ui);
    with(|c| {
        shell::apply_prefs(c);
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
        if let Some(ui) = c.window() {
            ui.global::<crate::shell::Settings>().set_has_tray(on);
        }
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
            Msg::Channels(list) => {
                if self.model.channel.as_ref().is_none_or(|c| !list.contains(c)) {
                    self.model.channel = list.first().cloned();
                }
                self.model.channels = list;
                if let Some(c) = self.model.channel.clone() {
                    self.channel_selected(&c);
                }
                shell::channels_changed(self);
                self.load_all();
            }
            Msg::Status(s) => {
                let schedule_changed = self.model.status.force_schedule != s.force_schedule;
                self.model.status = *s;
                shell::status_changed(self);
                air::status_changed(self);
                sources::refresh(self);
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
            }
            Msg::Playlists(p) => {
                self.model.playlists = p;
                playlists::reloaded(self);
                library::refresh(self);
                schedule::refresh(self);
            }
            Msg::Schedules(s) => {
                self.model.schedules = s;
                air::config_changed(self);
                schedule::reloaded(self);
            }
            Msg::Rules(r) => {
                self.model.rules = r;
                air::config_changed(self);
                schedule::refresh(self);
                rules::reloaded(self);
            }
            Msg::Graphics(g) => {
                self.model.graphics = g;
                air::status_changed(self);
                graphics::reloaded(self);
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
                    _ => {}
                }
            }
            Msg::Event(v) => {
                self.model.push_log(LogLine::from_event(&v));
                air::log_changed(self);
                log::changed(self);
            }
            Msg::Done(text) => self.toast(text, false),
            Msg::Error(e) => {
                self.model.push_log(LogLine { at: chrono::Local::now().format("%H:%M:%S").to_string(), level: "warning".into(), text: e.clone() });
                air::log_changed(self);
                log::changed(self);
                self.toast(e, true);
            }
            Msg::Apply(f) => f(self),
            Msg::Connection(ok) => {
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
            self.pl = Default::default();
            self.gfx = Default::default();
            self.lib.checked.clear();
            self.thumbs.clear();
        }
        self.poller.set_channel(ch);
    }

    // ------------------------------------------------------------ requests

    pub fn load_channels(&self) {
        self.worker.spawn(|api| {
            let list: Vec<Value> = api.get("/channels")?;
            Ok(Msg::Channels(list.iter().filter_map(|c| c["channel"].as_str().map(str::to_string)).collect()))
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
            Ok(Msg::Done(r["message"].as_str().map(crate::ru::server).unwrap_or_else(|| "Выполнено".into())))
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

    /// Ask before running a risky action.
    pub fn ask(&mut self, title: &str, text: &str, action_label: &str, action: impl FnOnce(&mut Ctx) + 'static) {
        let Some(ui) = self.window() else { return };
        self.pending_confirm = Some(Box::new(action));
        let g = ui.global::<App>();
        g.set_confirm_title(title.into());
        g.set_confirm_text(text.into());
        g.set_confirm_action(action_label.into());
        g.set_confirm_danger(true);
        g.set_confirm_open(true);
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
