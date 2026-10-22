//! Notification-area icon: the window closes into it and playout keeps
//! running. The menu shows what is on air and runs the everyday actions
//! (start / stop, skip, pause the schedule, ad break, ticker) without
//! opening the window; click or "Открыть" brings the window back.
//!
//! Windows / macOS use `tray-icon` (it needs the UI thread's message loop,
//! which Slint runs), Linux the D-Bus StatusNotifierItem protocol (`ksni`,
//! its own thread). Without a tray host [`start`] returns `false` and the
//! close button quits as before.

use std::cell::RefCell;

/// What the user asked for in the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    /// Show the window on a section.
    Section(&'static str),
    /// Transport action of the Air screen ("start", "stop", "skip", "manual-on", ...).
    Transport(&'static str),
    /// Show the window with an on-air dialog (1 ad break, 3 ticker).
    Dialog(i32),
    ToggleCloseToTray,
    Quit,
}

/// What the menu shows; [`update`] it whenever the status changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayState {
    /// First line: state and the item on air.
    pub status: String,
    pub online: bool,
    pub on_air: bool,
    pub manual: bool,
    pub close_to_tray: bool,
}

impl TrayState {
    fn tooltip(&self) -> String {
        if self.status.is_empty() { TOOLTIP.into() } else { format!("Laurabrod — {}", self.status) }
    }
    fn start_stop(&self) -> (&'static str, TrayAction) {
        if self.on_air { ("Остановить вещание…", TrayAction::Transport("stop")) } else { ("Начать вещание", TrayAction::Transport("start")) }
    }
    fn manual_entry(&self) -> (&'static str, TrayAction) {
        if self.manual {
            ("Вернуться к расписанию", TrayAction::Transport("manual-off"))
        } else {
            ("Пауза расписания", TrayAction::Transport("manual-on"))
        }
    }
}

/// Handler; called on the UI thread.
type OnAction = std::rc::Rc<dyn Fn(TrayAction)>;
type Post = std::sync::Arc<dyn Fn(TrayAction) + Send + Sync>;

const TOOLTIP: &str = "Laurabrod — управление эфиром";

thread_local! {
    static HANDLER: RefCell<Option<OnAction>> = const { RefCell::new(None) };
    static TRAY: RefCell<Option<platform::Tray>> = const { RefCell::new(None) };
    static LAST: RefCell<TrayState> = RefCell::new(TrayState::default());
}

/// RGBA pixels of the application icon (largest image of the .ico).
fn icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory_with_format(include_bytes!("../assets/app/laurabrod.ico"), image::ImageFormat::Ico).ok()?;
    let img = img.resize(64, 64, image::imageops::FilterType::Lanczos3).to_rgba8();
    let (w, h) = img.dimensions();
    Some((img.into_raw(), w, h))
}

/// Show the icon; `on` runs on the UI thread. `false`: no tray here.
pub fn start(on: OnAction) -> bool {
    HANDLER.with(|h| *h.borrow_mut() = Some(on));
    // tray callbacks are `Send`: hop to the UI thread, where `on` lives
    let post: Post = std::sync::Arc::new(|a| {
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(f) = HANDLER.with(|h| h.borrow().clone()) {
                f(a);
            }
        });
    });
    let state = LAST.with(|l| l.borrow().clone());
    match platform::Tray::new(post, &state) {
        Some(t) => {
            TRAY.with(|s| *s.borrow_mut() = Some(t));
            true
        }
        None => {
            HANDLER.with(|h| *h.borrow_mut() = None);
            false
        }
    }
}

/// Refresh the menu and tooltip (no-op when nothing changed or no tray).
pub fn update(state: TrayState) {
    if LAST.with(|l| *l.borrow() == state) {
        return;
    }
    LAST.with(|l| *l.borrow_mut() = state.clone());
    TRAY.with(|t| {
        if let Some(t) = t.borrow().as_ref() {
            t.update(&state);
        }
    });
}

/// The state the menu shows now.
pub fn state() -> TrayState {
    LAST.with(|l| l.borrow().clone())
}

/// Remove the icon.
pub fn stop() {
    TRAY.with(|t| t.borrow_mut().take());
    HANDLER.with(|h| *h.borrow_mut() = None);
}

/// The GUI has no logger of its own: stderr is what the host process keeps.
fn note(e: &str) {
    eprintln!("tray icon unavailable: {e}");
}

#[cfg(any(windows, target_os = "macos"))]
mod platform {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::{Post, TrayAction, TrayState};

    pub struct Tray {
        icon: TrayIcon,
        status: MenuItem,
        start_stop: MenuItem,
        skip: MenuItem,
        manual: MenuItem,
        ad: MenuItem,
        ticker: MenuItem,
        close: CheckMenuItem,
    }

    impl Tray {
        pub fn new(post: Post, st: &TrayState) -> Option<Self> {
            let (rgba, w, h) = super::icon_rgba()?;
            let icon = Icon::from_rgba(rgba, w, h).ok()?;
            let status = MenuItem::new("", false, None);
            let open = MenuItem::new("Открыть Laurabrod", true, None);
            let air = MenuItem::new("Экран «Эфир»", true, None);
            let start_stop = MenuItem::new("", true, None);
            let skip = MenuItem::new("Следующий элемент", true, None);
            let manual = MenuItem::new("", true, None);
            let ad = MenuItem::new("Рекламный блок…", true, None);
            let ticker = MenuItem::new("Бегущая строка…", true, None);
            let close = CheckMenuItem::new("Закрывать окно в трей", true, st.close_to_tray, None);
            let quit = MenuItem::new("Выход", true, None);
            let sep = PredefinedMenuItem::separator;
            let menu = Menu::new();
            menu.append_items(&[
                &status,
                &sep(),
                &open,
                &air,
                &sep(),
                &start_stop,
                &skip,
                &manual,
                &ad,
                &ticker,
                &sep(),
                &close,
                &quit,
            ])
            .ok()?;
            let icon = TrayIconBuilder::new()
                .with_tooltip(super::TOOLTIP)
                .with_icon(icon)
                .with_menu(Box::new(menu))
                // left click opens the window, right click the menu
                .with_menu_on_left_click(false)
                .build()
                .map_err(|e| super::note(&e.to_string()))
                .ok()?;

            // ids → actions; start/stop and manual depend on the state, so
            // they are resolved on the UI thread from the last state
            let fixed: Vec<(MenuId, TrayAction)> = vec![
                (open.id().clone(), TrayAction::Show),
                (air.id().clone(), TrayAction::Section("air")),
                (skip.id().clone(), TrayAction::Transport("skip")),
                (ad.id().clone(), TrayAction::Dialog(1)),
                (ticker.id().clone(), TrayAction::Dialog(3)),
                (close.id().clone(), TrayAction::ToggleCloseToTray),
                (quit.id().clone(), TrayAction::Quit),
            ];
            let (ss_id, manual_id) = (start_stop.id().clone(), manual.id().clone());
            let p = post.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if let Some((_, a)) = fixed.iter().find(|(id, _)| *id == e.id) {
                    p(*a);
                } else if e.id == ss_id {
                    p(TrayAction::Transport("start-stop"));
                } else if e.id == manual_id {
                    p(TrayAction::Transport("manual-toggle"));
                }
            }));
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| match e {
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => post(TrayAction::Show),
                _ => {}
            }));
            let t = Tray { icon, status, start_stop, skip, manual, ad, ticker, close };
            t.update(st);
            Some(t)
        }

        pub fn update(&self, st: &TrayState) {
            self.status.set_text(if st.status.is_empty() { "Laurabrod" } else { &st.status });
            self.start_stop.set_text(st.start_stop().0);
            self.start_stop.set_enabled(st.online);
            self.skip.set_enabled(st.online && st.on_air);
            self.manual.set_text(st.manual_entry().0);
            self.manual.set_enabled(st.online && st.on_air);
            self.ad.set_enabled(st.online && st.on_air);
            self.ticker.set_enabled(st.online);
            // the check item toggles itself on click: keep it on the setting
            self.close.set_checked(st.close_to_tray);
            let _ = self.icon.set_tooltip(Some(st.tooltip()));
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use ksni::blocking::TrayMethods;

    use super::{Post, TrayAction, TrayState};

    struct Item {
        post: Post,
        /// ARGB32, network byte order
        icon: Vec<ksni::Icon>,
        state: TrayState,
    }

    impl Item {
        fn entry(&self, label: &str, enabled: bool, a: TrayAction) -> ksni::MenuItem<Self> {
            ksni::menu::StandardItem {
                label: label.into(),
                enabled,
                activate: Box::new(move |t: &mut Self| (t.post)(a)),
                ..Default::default()
            }
            .into()
        }
    }

    impl ksni::Tray for Item {
        fn id(&self) -> String {
            "laurabrod".into()
        }
        fn title(&self) -> String {
            "Laurabrod".into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icon.clone()
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip { title: self.state.tooltip(), ..Default::default() }
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            (self.post)(TrayAction::Show);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            let s = &self.state;
            let live = s.online && s.on_air;
            let (ss, ss_a) = s.start_stop();
            let (man, man_a) = s.manual_entry();
            vec![
                ksni::menu::StandardItem {
                    label: if s.status.is_empty() { "Laurabrod".into() } else { s.status.clone() },
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
                self.entry("Открыть Laurabrod", true, TrayAction::Show),
                self.entry("Экран «Эфир»", true, TrayAction::Section("air")),
                ksni::MenuItem::Separator,
                self.entry(ss, s.online, ss_a),
                self.entry("Следующий элемент", live, TrayAction::Transport("skip")),
                self.entry(man, live, man_a),
                self.entry("Рекламный блок…", live, TrayAction::Dialog(1)),
                self.entry("Бегущая строка…", s.online, TrayAction::Dialog(3)),
                ksni::MenuItem::Separator,
                ksni::menu::CheckmarkItem {
                    label: "Закрывать окно в трей".into(),
                    checked: s.close_to_tray,
                    activate: Box::new(|t: &mut Self| (t.post)(TrayAction::ToggleCloseToTray)),
                    ..Default::default()
                }
                .into(),
                self.entry("Выход", true, TrayAction::Quit),
            ]
        }
    }

    pub struct Tray(ksni::blocking::Handle<Item>);

    impl Tray {
        pub fn new(post: Post, st: &TrayState) -> Option<Self> {
            let (rgba, w, h) = super::icon_rgba()?;
            // RGBA → ARGB
            let data = rgba.chunks_exact(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
            let item = Item { post, icon: vec![ksni::Icon { width: w as i32, height: h as i32, data }], state: st.clone() };
            match item.spawn() {
                Ok(handle) => Some(Tray(handle)),
                Err(e) => {
                    super::note(&e.to_string());
                    None
                }
            }
        }

        pub fn update(&self, st: &TrayState) {
            let st = st.clone();
            self.0.update(move |i| i.state = st);
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            let _ = self.0.shutdown();
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    use super::{Post, TrayState};

    pub struct Tray;

    impl Tray {
        pub fn new(_: Post, _: &TrayState) -> Option<Self> {
            None
        }
        pub fn update(&self, _: &TrayState) {}
    }
}
