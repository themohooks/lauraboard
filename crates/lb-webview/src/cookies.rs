#[cfg(any(windows, target_os = "macos"))]
use serde_json::{Value, json};

#[cfg(windows)]
pub fn read(view: &wry::WebView, done: impl FnOnce(Value) + Send + 'static) -> Result<(), String> {
    use windows::core::w;
    use wry::WebViewExtWindows;
    let handler = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, result| {
        done(match status {
            Ok(()) => serde_json::from_str(&result).unwrap_or_else(|e| json!({"error":e.to_string()})),
            Err(error) => json!({"error":error.to_string()}),
        });
        Ok(())
    }));
    unsafe { view.webview().CallDevToolsProtocolMethod(w!("Storage.getCookies"), w!("{}"), &handler) }.map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
#[path = "cookies_linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::read;

#[cfg(target_os = "macos")]
pub fn read(view: &wry::WebView, done: impl FnOnce(Value) + Send + 'static) -> Result<(), String> {
    let cookies = view.cookies().map_err(|e| e.to_string())?;
    done(json!({"cookies": cookies.iter().map(|c| json!({
        "domain": c.domain().unwrap_or_default(), "path": c.path().unwrap_or("/"),
        "name": c.name(), "value": c.value(), "secure": c.secure().unwrap_or(false),
        "httpOnly": c.http_only().unwrap_or(false),
        "expires": c.expires_datetime().map_or(0, |date| date.unix_timestamp())
    })).collect::<Vec<_>>()}));
    Ok(())
}
