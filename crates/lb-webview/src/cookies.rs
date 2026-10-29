#[cfg(any(windows, target_os = "macos"))]
use serde_json::{Value, json};

#[cfg(windows)]
pub fn read(view: &wry::WebView, done: impl FnOnce(Value) + Send + 'static) -> Result<(), String> {
    use webview2_com::{GetCookiesCompletedHandler, Microsoft::Web::WebView2::Win32::ICoreWebView2_2, take_pwstr};
    use windows::core::{BOOL, Interface, PCWSTR, PWSTR};
    use wry::WebViewExtWindows;
    // Read the native store asynchronously, including while navigation is pending.
    let handler = GetCookiesCompletedHandler::create(Box::new(move |status, cookies| {
        let result = (|| -> windows::core::Result<Value> {
            status?;
            let mut exported = Vec::new();
            if let Some(cookies) = cookies {
                unsafe {
                    let mut count = 0;
                    cookies.Count(&mut count)?;
                    for index in 0..count {
                        let cookie = cookies.GetValueAtIndex(index)?;
                        let mut name = PWSTR::null();
                        cookie.Name(&mut name)?;
                        let name = take_pwstr(name);
                        let mut value = PWSTR::null();
                        cookie.Value(&mut value)?;
                        let value = take_pwstr(value);
                        let mut domain = PWSTR::null();
                        cookie.Domain(&mut domain)?;
                        let domain = take_pwstr(domain);
                        let mut path = PWSTR::null();
                        cookie.Path(&mut path)?;
                        let path = take_pwstr(path);
                        let mut http_only = BOOL::default();
                        cookie.IsHttpOnly(&mut http_only)?;
                        let mut secure = BOOL::default();
                        cookie.IsSecure(&mut secure)?;
                        let mut expires = 0.0;
                        cookie.Expires(&mut expires)?;
                        exported.push(json!({
                            "name": name, "value": value, "domain": domain, "path": path,
                            "httpOnly": http_only.as_bool(), "secure": secure.as_bool(),
                            "expires": expires
                        }));
                    }
                }
            }
            Ok(json!({"cookies": exported}))
        })();
        done(result.unwrap_or_else(|error| json!({"error": error.to_string()})));
        Ok(())
    }));
    unsafe {
        view.webview()
            .cast::<ICoreWebView2_2>()
            .and_then(|view| view.CookieManager())
            .and_then(|manager| manager.GetCookies(PCWSTR::null(), &handler))
    }
    .map_err(|error| error.to_string())
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
