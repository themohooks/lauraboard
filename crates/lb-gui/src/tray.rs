//! Notification-area icon: the window closes into it and playout keeps
//! running. Click (or "Открыть") brings the window back, "Выход" quits.
//!
//! Windows / macOS use `tray-icon` (it needs the UI thread's message loop,
//! which Slint runs), Linux the D-Bus StatusNotifierItem protocol (`ksni`,
//! its own thread). Without a tray host [`start`] returns `None` and the
//! close button quits as before.

/// What the user asked for in the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Quit,
}

/// Handler; called on the UI thread.
type OnAction = std::rc::Rc<dyn Fn(TrayAction)>;

/// Keeps the icon alive; dropping it removes the icon.
pub struct Tray {
    #[cfg(any(windows, target_os = "macos"))]
    _icon: tray_icon::TrayIcon,
    #[cfg(target_os = "linux")]
    handle: ksni::blocking::Handle<sni::Item>,
}

impl Drop for Tray {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            let _ = self.handle.shutdown();
        }
    }
}

const TOOLTIP: &str = "Laurabrod — управление эфиром";

/// RGBA pixels of the application icon (largest image of the .ico).
fn icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory_with_format(include_bytes!("../assets/app/laurabrod.ico"), image::ImageFormat::Ico).ok()?;
    let img = img.resize(64, 64, image::imageops::FilterType::Lanczos3).to_rgba8();
    let (w, h) = img.dimensions();
    Some((img.into_raw(), w, h))
}

/// Show the icon; `on` runs on the UI thread.
pub fn start(on: OnAction) -> Option<Tray> {
    // tray callbacks are `Send`: hop to the UI thread, where `on` lives
    thread_local! {
        static HANDLER: std::cell::RefCell<Option<OnAction>> = const { std::cell::RefCell::new(None) };
    }
    HANDLER.with(|h| *h.borrow_mut() = Some(on));
    let post: std::sync::Arc<dyn Fn(TrayAction) + Send + Sync> = std::sync::Arc::new(|a| {
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(f) = HANDLER.with(|h| h.borrow().clone()) {
                f(a);
            }
        });
    });
    let tray = platform(post);
    if tray.is_none() {
        HANDLER.with(|h| *h.borrow_mut() = None);
    }
    tray
}

#[cfg(any(windows, target_os = "macos"))]
fn platform(post: std::sync::Arc<dyn Fn(TrayAction) + Send + Sync>) -> Option<Tray> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let (rgba, w, h) = icon_rgba()?;
    let icon = Icon::from_rgba(rgba, w, h).ok()?;
    let menu = Menu::new();
    let show = MenuItem::new("Открыть Laurabrod", true, None);
    let quit = MenuItem::new("Выход", true, None);
    menu.append_items(&[&show, &PredefinedMenuItem::separator(), &quit]).ok()?;
    let tray = TrayIconBuilder::new()
        .with_tooltip(TOOLTIP)
        .with_icon(icon)
        .with_menu(Box::new(menu))
        // left click opens the window, right click the menu
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| tracing_note(&e.to_string()))
        .ok()?;

    let (show_id, quit_id) = (show.id().clone(), quit.id().clone());
    let p = post.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == show_id {
            p(TrayAction::Show);
        } else if e.id == quit_id {
            p(TrayAction::Quit);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| match e {
        TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
        | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => post(TrayAction::Show),
        _ => {}
    }));
    Some(Tray { _icon: tray })
}

#[cfg(target_os = "linux")]
mod sni {
    use super::{TOOLTIP, TrayAction};

    pub struct Item {
        pub post: std::sync::Arc<dyn Fn(TrayAction) + Send + Sync>,
        /// ARGB32, network byte order
        pub icon: Vec<ksni::Icon>,
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
            ksni::ToolTip { title: TOOLTIP.into(), ..Default::default() }
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            (self.post)(TrayAction::Show);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            vec![
                StandardItem { label: "Открыть Laurabrod".into(), activate: Box::new(|t: &mut Self| (t.post)(TrayAction::Show)), ..Default::default() }
                    .into(),
                ksni::MenuItem::Separator,
                StandardItem { label: "Выход".into(), activate: Box::new(|t: &mut Self| (t.post)(TrayAction::Quit)), ..Default::default() }.into(),
            ]
        }
    }
}

#[cfg(target_os = "linux")]
fn platform(post: std::sync::Arc<dyn Fn(TrayAction) + Send + Sync>) -> Option<Tray> {
    use ksni::blocking::TrayMethods;

    let (rgba, w, h) = icon_rgba()?;
    // RGBA → ARGB
    let data = rgba.chunks_exact(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
    let item = sni::Item { post, icon: vec![ksni::Icon { width: w as i32, height: h as i32, data }] };
    match item.spawn() {
        Ok(handle) => Some(Tray { handle }),
        Err(e) => {
            tracing_note(&e.to_string());
            None
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn platform(_: std::sync::Arc<dyn Fn(TrayAction) + Send + Sync>) -> Option<Tray> {
    None
}

/// The GUI has no logger of its own: stderr is what the host process keeps.
fn tracing_note(e: &str) {
    eprintln!("tray icon unavailable: {e}");
}
