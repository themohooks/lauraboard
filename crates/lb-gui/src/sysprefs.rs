//! Best-effort reading of OS accessibility preferences.

/// `true` when the OS asks applications to minimise animations.
#[cfg(windows)]
pub fn reduce_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};
    let mut enabled: i32 = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes a BOOL into the provided pointer.
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&mut enabled as *mut i32).cast(), 0) };
    ok != 0 && enabled == 0
}

/// `true` when the OS asks applications to minimise animations
/// (GNOME / GTK-based desktops via `gsettings`).
#[cfg(not(any(windows, target_os = "android")))]
pub fn reduce_motion() -> bool {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "false")
}

#[cfg(target_os = "android")]
pub fn reduce_motion() -> bool {
    true
}
