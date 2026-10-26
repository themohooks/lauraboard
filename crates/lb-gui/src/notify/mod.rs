//! Notification popups drawn by the program itself (`NotifyWindow` in
//! `ui/shell/notify-window.slint`) instead of the OS notification service:
//! a stack of cards in the corner of the screen with the channel avatar.
//!
//! The window is created on the first notice and then only shown / hidden;
//! it never takes the focus from the window the operator works in.
//! Cards close by themselves after [`LIFETIME`] (critical ones later),
//! never while the pointer is over the stack.

mod place;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Image, Model, VecModel};

use crate::shell::{NoticeRow, NotifyWindow, Popups, Theme, ThemeMode};

pub use place::select_backend;

/// Importance of a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Info,
    Warning,
    Critical,
}

/// What to show: text and the channel it is about.
pub struct Notice {
    pub title: String,
    pub body: String,
    pub urgency: Urgency,
    /// Channel avatar, or the letter shown without one.
    pub avatar: Option<Image>,
    pub initial: String,
}

/// How long a card stays.
const LIFETIME: Duration = Duration::from_secs(7);
const LIFETIME_CRITICAL: Duration = Duration::from_secs(20);
/// Cards at most; the oldest goes first.
const MAX_CARDS: usize = 4;
const TICK: Duration = Duration::from_millis(250);

struct Card {
    id: i32,
    left: Duration,
}

struct Popup {
    win: NotifyWindow,
    model: Rc<VecModel<NoticeRow>>,
    cards: Vec<Card>,
    next_id: i32,
    hovered: Option<i32>,
    timer: slint::Timer,
    visible: bool,
    shown_once: bool,
}

thread_local! {
    static POPUP: RefCell<Option<Popup>> = const { RefCell::new(None) };
    /// Set before the first notice: brings the main window up.
    static ON_OPEN: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
    static THEME: RefCell<ThemeMode> = const { RefCell::new(ThemeMode::System) };
}

/// What a click on a card does (show the main window).
pub fn set_on_open(f: impl Fn() + 'static) {
    ON_OPEN.with(|o| *o.borrow_mut() = Some(Rc::new(f)));
}

/// Follow the theme of the main window.
pub fn set_theme(mode: ThemeMode) {
    THEME.with(|t| *t.borrow_mut() = mode);
    POPUP.with(|p| {
        if let Some(p) = p.borrow().as_ref() {
            p.win.global::<Theme>().set_mode(mode);
        }
    });
}

/// Show a notice (UI thread only).
pub fn show(n: Notice) {
    POPUP.with(|p| {
        let mut p = p.borrow_mut();
        if p.is_none() {
            match create() {
                Ok(new) => *p = Some(new),
                Err(e) => return eprintln!("notification window: {e}"),
            }
        }
        let Some(p) = p.as_mut() else { return };
        p.next_id += 1;
        let id = p.next_id;
        let row = NoticeRow {
            id,
            title: n.title.into(),
            body: n.body.into(),
            time: chrono::Local::now().format("%H:%M").to_string().into(),
            urgency: match n.urgency {
                Urgency::Info => 0,
                Urgency::Warning => 1,
                Urgency::Critical => 2,
            },
            has_avatar: n.avatar.is_some(),
            avatar: n.avatar.unwrap_or_default(),
            initial: n.initial.into(),
        };
        p.model.push(row);
        p.cards.push(Card { id, left: if n.urgency == Urgency::Critical { LIFETIME_CRITICAL } else { LIFETIME } });
        while p.cards.len() > MAX_CARDS {
            let old = p.cards.remove(0);
            remove_row(p, old.id);
        }
        layout(p);
        if !p.timer.running() {
            p.timer.start(slint::TimerMode::Repeated, TICK, tick);
        }
    });
}

fn create() -> Result<Popup, String> {
    let win = place::create().map_err(|e| e.to_string())?;
    win.global::<Theme>().set_mode(THEME.with(|t| *t.borrow()));
    let model = Rc::new(VecModel::<NoticeRow>::default());
    let g = win.global::<Popups>();
    g.set_items(model.clone().into());
    g.on_close(close);
    g.on_open(|id| {
        close(id);
        if let Some(f) = ON_OPEN.with(|o| o.borrow().clone()) {
            f();
        }
    });
    g.on_hover(|id, on| {
        with(|p| {
            if on {
                p.hovered = Some(id);
            } else if p.hovered == Some(id) {
                p.hovered = None;
            }
        })
    });
    Ok(Popup {
        win,
        model,
        cards: Vec::new(),
        next_id: 0,
        hovered: None,
        timer: slint::Timer::default(),
        visible: false,
        shown_once: false,
    })
}

fn with(f: impl FnOnce(&mut Popup)) {
    POPUP.with(|p| {
        if let Ok(mut p) = p.try_borrow_mut()
            && let Some(p) = p.as_mut()
        {
            f(p);
        }
    });
}

/// Close one card.
fn close(id: i32) {
    with(|p| {
        p.cards.retain(|c| c.id != id);
        if p.hovered == Some(id) {
            p.hovered = None;
        }
        remove_row(p, id);
        layout(p);
    });
}

fn remove_row(p: &Popup, id: i32) {
    if let Some(i) = p.model.iter().position(|r| r.id == id) {
        p.model.remove(i);
    }
}

/// Count down the cards; the stack waits while the pointer is over it.
fn tick() {
    with(|p| {
        if p.hovered.is_some() {
            return;
        }
        let mut gone = Vec::new();
        for c in &mut p.cards {
            c.left = c.left.saturating_sub(TICK);
            if c.left.is_zero() {
                gone.push(c.id);
            }
        }
        if gone.is_empty() {
            return;
        }
        p.cards.retain(|c| !gone.contains(&c.id));
        for id in gone {
            remove_row(p, id);
        }
        layout(p);
    });
}

/// Size the window to the cards, put it in the corner, show or hide it.
fn layout(p: &mut Popup) {
    let n = p.cards.len();
    if n == 0 {
        p.timer.stop();
        if p.visible {
            p.visible = false;
            place::hide(&p.win);
        }
        return;
    }
    let g = p.win.global::<Popups>();
    let (card, gap, margin) = (g.get_card_height(), g.get_gap(), g.get_margin());
    let size = slint::LogicalSize::new(380.0, n as f32 * card + (n as f32 - 1.0) * gap + 2.0 * margin);
    p.win.window().set_size(size);
    place::corner(&p.win, size);
    if !p.visible {
        p.visible = true;
        place::show(&p.win, !p.shown_once);
        p.shown_once = true;
        // first show: the platform window exists only now
        place::corner(&p.win, size);
    }
}
