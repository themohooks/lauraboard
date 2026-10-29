//! NativeActivity has one window; show notices inside the operator UI.
use crate::shell::ThemeMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Info,
    Warning,
    Critical,
}

pub struct Notice {
    pub title: String,
    pub body: String,
    pub urgency: Urgency,
    pub avatar: Option<slint::Image>,
    pub initial: String,
}

pub fn set_theme(_: ThemeMode) {}
pub fn set_animations(_: bool) {}
pub fn show(n: Notice) {
    crate::bridge::with(move |c| c.toast(format!("{}: {}", n.title, n.body), n.urgency == Urgency::Critical));
}
