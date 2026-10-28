//! Read the whole WebKit cookie store without Wry's nested gtk::main_iteration
//! loop. GTK/Tao must keep servicing navigation and shutdown while IPC is pending.
use glib::translate::{FromGlibPtrContainer, ToGlibPtr, from_glib_full};
use gtk::{gio, glib};
use serde_json::{Value, json};
use webkit2gtk::{WebViewExt, WebsiteDataManagerExt};
use wry::WebViewExtUnix;

type Completion = Box<dyn FnOnce(Value)>;

// WebKitGTK >= 2.42. The 2.0 gtk-rs bindings do not expose these yet;
// Wry 0.57 uses the same native API for its synchronous cookies() method.
unsafe extern "C" {
    fn webkit_cookie_manager_get_all_cookies(
        manager: *mut webkit2gtk::ffi::WebKitCookieManager,
        cancellable: *mut gio::ffi::GCancellable,
        callback: gio::ffi::GAsyncReadyCallback,
        data: glib::ffi::gpointer,
    );
    fn webkit_cookie_manager_get_all_cookies_finish(
        manager: *mut webkit2gtk::ffi::WebKitCookieManager,
        result: *mut gio::ffi::GAsyncResult,
        error: *mut *mut glib::ffi::GError,
    ) -> *mut glib::ffi::GList;
}

unsafe extern "C" fn completed(source: *mut glib::gobject_ffi::GObject, result: *mut gio::ffi::GAsyncResult, data: glib::ffi::gpointer) {
    // The native async operation calls once on the GTK main context. Recover
    // the outer box (a thin pointer) and release the owned GLib result/list.
    let done = unsafe { Box::from_raw(data.cast::<Completion>()) };
    let mut error = std::ptr::null_mut();
    let list = unsafe { webkit_cookie_manager_get_all_cookies_finish(source.cast(), result, &mut error) };
    let value = if !error.is_null() {
        let error: glib::Error = unsafe { from_glib_full(error) };
        json!({"error": error.to_string()})
    } else {
        let cookies: Vec<soup::Cookie> = unsafe { FromGlibPtrContainer::from_glib_full(list) };
        let string = |s: Option<glib::GString>| s.map(|s| s.to_string()).unwrap_or_default();
        json!({"cookies": cookies.into_iter().map(|mut c| json!({
            "domain": string(c.domain()), "path": string(c.path()),
            "name": string(c.name()), "value": string(c.value()),
            "secure": c.is_secure(), "httpOnly": c.is_http_only(),
            "expires": c.expires().map_or(0, |date| date.to_unix())
        })).collect::<Vec<_>>()})
    };
    done(value);
}

pub fn read(view: &wry::WebView, done: impl FnOnce(Value) + Send + 'static) -> Result<(), String> {
    let manager = view.webview().website_data_manager().and_then(|m| m.cookie_manager()).ok_or("WebKit cookie manager unavailable")?;
    let done: Box<Completion> = Box::new(Box::new(done));
    // GIO retains the manager for the operation; completed owns the callback.
    unsafe {
        webkit_cookie_manager_get_all_cookies(manager.to_glib_none().0, std::ptr::null_mut(), Some(completed), Box::into_raw(done).cast());
    }
    Ok(())
}
