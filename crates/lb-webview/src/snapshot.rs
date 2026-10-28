#[cfg(windows)]
pub fn capture(view: &wry::WebView, done: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
    use base64::Engine;
    use windows::core::w;
    use wry::WebViewExtWindows;
    let done = std::rc::Rc::new(std::cell::RefCell::new(Some(done)));
    let callback = done.clone();
    let handler = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, json| {
        let result = status.map_err(|e| e.to_string()).and_then(|_| {
            let value: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            base64::engine::general_purpose::STANDARD
                .decode(value["data"].as_str().ok_or("WebView returned no PNG")?)
                .map_err(|e| e.to_string())
        });
        if let Some(done) = callback.borrow_mut().take() {
            done(result);
        }
        Ok(())
    }));
    let result = unsafe {
        view.webview().CallDevToolsProtocolMethod(
            w!("Page.captureScreenshot"),
            w!("{\"format\":\"png\",\"captureBeyondViewport\":false}"),
            &handler,
        )
    };
    if let Err(error) = result
        && let Some(done) = done.borrow_mut().take()
    {
        done(Err(error.to_string()));
    }
}

#[cfg(target_os = "linux")]
pub fn capture(view: &wry::WebView, done: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};
    use wry::WebViewExtUnix;
    view.webview().snapshot(
        SnapshotRegion::Visible,
        SnapshotOptions::TRANSPARENT_BACKGROUND,
        None::<&gtk::gio::Cancellable>,
        move |result| {
            let result = result.map_err(|e| e.to_string()).and_then(|surface| {
                let mut png = Vec::new();
                surface.write_to_png(&mut png).map_err(|e| e.to_string())?;
                Ok(png)
            });
            done(result);
        },
    );
}

#[cfg(target_os = "macos")]
pub fn capture(view: &wry::WebView, done: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
    use objc2::AnyThread;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use wry::WebViewExtMacOS;
    let done = std::cell::RefCell::new(Some(done));
    let callback = block2::RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        let result = (|| {
            if !error.is_null() {
                return Err("WKWebView snapshot failed".into());
            }
            let image = unsafe { image.as_ref() }.ok_or("WKWebView returned no image")?;
            let tiff = image.TIFFRepresentation().ok_or("WKWebView returned no bitmap")?;
            let bitmap = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &tiff).ok_or("Invalid snapshot bitmap")?;
            let data = unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
                .ok_or("PNG encoding failed")?;
            Ok(data.to_vec())
        })();
        if let Some(done) = done.borrow_mut().take() {
            done(result);
        }
    });
    unsafe {
        view.webview().takeSnapshotWithConfiguration_completionHandler(None, &callback);
    }
}
pub fn mute(view: &wry::WebView) {
    #[cfg(windows)]
    {
        use windows::core::Interface;
        use wry::WebViewExtWindows;
        if let Ok(view) = view.webview().cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8>() {
            let _ = unsafe { view.SetIsMuted(true) };
        }
    }
    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::WebViewExt;
        use wry::WebViewExtUnix;
        view.webview().set_is_muted(true);
    }
    // WKWebView has no public per-view mute API. Mute media elements on
    // insertion. The Web Audio capture tap outputs silence to the device.
    #[cfg(target_os = "macos")]
    {
        let _ = view.evaluate_script("(()=>{const mute=()=>document.querySelectorAll('audio,video').forEach(e=>{e.muted=true});mute();new MutationObserver(mute).observe(document,{childList:true,subtree:true})})()");
    }
}
pub const READY: &str = "({ready: document.readyState === 'complete' && (typeof window.__lbReady === 'undefined' || window.__lbReady === true), error: window.__lbError || null, stage: window.__lbStage || document.readyState})";

#[cfg(not(windows))]
pub fn readiness(view: &wry::WebView, done: impl Fn(String) + Send + 'static) -> Result<(), String> {
    view.evaluate_script_with_callback(READY, done).map_err(|e| e.to_string())
}

#[cfg(windows)]
pub fn readiness(view: &wry::WebView, done: impl Fn(String) + Send + 'static) -> Result<(), String> {
    use windows::core::{HSTRING, w};
    use wry::WebViewExtWindows;
    let params = HSTRING::from(serde_json::json!({"expression":READY,"returnByValue":true}).to_string());
    let handler = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, result| {
        let value = if let Err(error) = status {
            serde_json::json!({"error":error.to_string()})
        } else {
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
            if !parsed["exceptionDetails"].is_null() {
                serde_json::json!({"error":parsed["exceptionDetails"].to_string()})
            } else {
                parsed["result"]["value"].clone()
            }
        };
        done(value.to_string());
        Ok(())
    }));
    unsafe { view.webview().CallDevToolsProtocolMethod(w!("Runtime.evaluate"), &params, &handler) }.map_err(|e| e.to_string())
}
