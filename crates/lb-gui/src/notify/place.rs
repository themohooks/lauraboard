//! Where the popup window goes and how it is shown without taking the
//! focus: bottom-right corner of the work area (above the taskbar) on
//! Windows and Linux, top-right under the menu bar on macOS.

use std::cell::Cell;

use slint::winit_030::{WinitWindowAccessor, winit};
use slint::{ComponentHandle, LogicalSize, PhysicalPosition};

use crate::shell::NotifyWindow;

thread_local! {
    /// The next window created is the popup window (see [`select_backend`]).
    static NEXT_IS_POPUP: Cell<bool> = const { Cell::new(false) };
}

/// Pick the winit backend with a hook that creates the popup window
/// inactive and without a taskbar button. Call before any window exists;
/// fails quietly when another platform is already set (snapshots).
pub fn select_backend() {
    let selector = slint::BackendSelector::new();
    // Slint's built-in fallback skips OpenGL when WGPU is compiled in.
    #[cfg(any(windows, target_os = "linux"))]
    let selector = if std::env::var_os("SLINT_BACKEND").is_none() && !has_vulkan() {
        eprintln!("No Vulkan adapter; using OpenGL");
        selector.renderer_name("femtovg".into())
    } else {
        selector
    };
    let r = selector
        .with_winit_window_attributes_hook(|a| {
            if !NEXT_IS_POPUP.with(|n| n.replace(false)) {
                return a;
            }
            #[cfg(windows)]
            let a = winit::platform::windows::WindowAttributesExtWindows::with_skip_taskbar(a, true);
            a.with_active(false)
        })
        .select();
    if let Err(e) = r {
        eprintln!("window backend: {e}");
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn has_vulkan() -> bool {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN))
        .iter()
        .any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu)
}

/// Create the popup window (marks it for the backend hook).
pub fn create() -> Result<NotifyWindow, slint::PlatformError> {
    NEXT_IS_POPUP.with(|n| n.set(true));
    let w = NotifyWindow::new();
    NEXT_IS_POPUP.with(|n| n.set(false));
    w
}

/// Put the window of `size` in the corner.
pub fn corner(win: &NotifyWindow, size: LogicalSize) {
    let Some((x, y, w, h, scale)) = area(win) else { return };
    let (pw, ph) = ((size.width * scale) as i32, (size.height * scale) as i32);
    let pos = if cfg!(target_os = "macos") {
        PhysicalPosition::new(x + w - pw, y + (28.0 * scale) as i32)
    } else {
        PhysicalPosition::new(x + w - pw, y + h - ph)
    };
    win.window().set_position(pos);
}

/// Work area (x, y, width, height in physical pixels) and its scale.
#[cfg(windows)]
fn area(_win: &NotifyWindow) -> Option<(i32, i32, i32, i32, f32)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: SPI_GETWORKAREA writes one RECT into the pointed buffer.
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut r as *mut RECT).cast(), 0) } != 0;
    // SAFETY: no arguments, no preconditions.
    let dpi = unsafe { GetDpiForSystem() };
    ok.then(|| (r.left, r.top, r.right - r.left, r.bottom - r.top, dpi.max(96) as f32 / 96.0))
}

/// Monitor of the window (exists only after the first show), leaving
/// room for panels at the bottom.
#[cfg(not(windows))]
fn area(win: &NotifyWindow) -> Option<(i32, i32, i32, i32, f32)> {
    win.window()
        .with_winit_window(|w: &winit::window::Window| {
            let m = w.current_monitor().or_else(|| w.primary_monitor())?;
            let (p, s, scale) = (m.position(), m.size(), m.scale_factor() as f32);
            let panel = (40.0 * scale) as i32;
            Some((p.x, p.y, s.width as i32, s.height as i32 - panel, scale))
        })
        .flatten()
}

/// Show without activating. The first show goes through Slint (the
/// window is created inactive by the hook); later ones on Windows go
/// straight to the system, since winit activates windows it shows again.
pub fn show(win: &NotifyWindow, first: bool) {
    #[cfg(windows)]
    if !first && let Some(h) = hwnd(win) {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        style(win);
        // SAFETY: `h` is the live window of `win`.
        unsafe {
            ShowWindow(h, SW_SHOWNOACTIVATE);
            SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        return;
    }
    let _ = first;
    if let Err(e) = win.show() {
        eprintln!("notification window: {e}");
    }
    // the system window appears on the next event-loop turn
    #[cfg(windows)]
    {
        let weak = win.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(w) = weak.upgrade() {
                style(&w);
            }
        });
    }
}

/// No taskbar button, no activation by clicks.
#[cfg(windows)]
fn style(win: &NotifyWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let Some(h) = hwnd(win) else { return };
    // SAFETY: `h` is the live window of `win`; only extended style bits change.
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let want = (ex | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize) & !(WS_EX_APPWINDOW as isize);
        if want != ex {
            SetWindowLongPtrW(h, GWL_EXSTYLE, want);
        }
    }
}

pub fn hide(win: &NotifyWindow) {
    #[cfg(windows)]
    if let Some(h) = hwnd(win) {
        // SAFETY: `h` is the live window of `win`.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(h, windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE) };
        return;
    }
    let _ = win.hide();
}

#[cfg(windows)]
fn hwnd(win: &NotifyWindow) -> Option<windows_sys::Win32::Foundation::HWND> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    win.window()
        .with_winit_window(|w: &winit::window::Window| match w.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get() as windows_sys::Win32::Foundation::HWND),
            _ => None,
        })
        .flatten()
}
