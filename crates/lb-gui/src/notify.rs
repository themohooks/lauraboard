//! System notifications: Windows toasts, freedesktop notifications on
//! Linux, Notification Center on macOS. Sent from a background thread
//! (showing one can block for a moment, e.g. D-Bus on Linux).

/// Importance of a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Info,
    Warning,
    Critical,
}

/// Show a notification; failures are only logged (no notification
/// service, notifications disabled in the OS).
pub fn show(title: &str, body: &str, urgency: Urgency) {
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.summary(&title).body(&body).appname("Laurabrod");
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            n.icon("laurabrod");
            n.urgency(match urgency {
                Urgency::Info => notify_rust::Urgency::Low,
                Urgency::Warning => notify_rust::Urgency::Normal,
                Urgency::Critical => notify_rust::Urgency::Critical,
            });
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let _ = urgency;
        if let Err(e) = n.show() {
            eprintln!("system notification failed: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    /// Shows a real notification: `cargo test -p lb-gui notify -- --ignored`.
    #[test]
    #[ignore]
    fn shows_a_notification() {
        let r = notify_rust::Notification::new().summary("Laurabrod: проверка").body("Тестовое уведомление").appname("Laurabrod").show();
        assert!(r.is_ok(), "{r:?}");
    }
}
