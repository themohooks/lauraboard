//! A fresh, offline WebView for one yt-dlp EJS evaluation. No login profile.
use base64::Engine;
use std::io::Read;
use std::time::{Duration, Instant};

use tao::{
    event::Event,
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

#[derive(Debug)]
enum Message {
    Loaded,
    Result(serde_json::Value),
}

pub fn run() -> Result<(), String> {
    const MAX_SCRIPT: u64 = 32 * 1024 * 1024;
    let mut source = String::new();
    std::io::stdin().take(MAX_SCRIPT + 1).read_to_string(&mut source).map_err(|e| e.to_string())?;
    if source.len() as u64 > MAX_SCRIPT {
        return Err("JavaScript input is too large".into());
    }
    let event_loop = EventLoopBuilder::<Message>::with_user_event().build();
    #[cfg(target_os = "macos")]
    let event_loop = {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        let mut event_loop = event_loop;
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        event_loop
    };
    let window_builder = WindowBuilder::new().with_visible(false).with_focused(false);
    #[cfg(windows)]
    let window_builder = {
        use tao::platform::windows::WindowBuilderExtWindows;
        window_builder.with_skip_taskbar(true)
    };
    let window = window_builder.build(&event_loop).map_err(|e| e.to_string())?;
    let loaded = event_loop.create_proxy();
    let result = event_loop.create_proxy();
    let html = r#"<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'"><body></body>"#;
    let page_url = format!("data:text/html;base64,{}", base64::engine::general_purpose::STANDARD.encode(html));
    let allowed_url = page_url.clone();
    let builder = WebViewBuilder::new()
        .with_incognito(true)
        .with_url(&page_url)
        .with_navigation_handler(move |url| url == "about:blank" || url == allowed_url)
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_on_page_load_handler(move |event, _| {
            if matches!(event, wry::PageLoadEvent::Finished) {
                let _ = loaded.send_event(Message::Loaded);
            }
        })
        .with_ipc_handler(move |request| {
            if request.body().len() <= 32 * 1024 * 1024 {
                if let Ok(value) = serde_json::from_str(request.body()) {
                    let _ = result.send_event(Message::Result(value));
                }
            }
        });
    #[cfg(target_os = "macos")]
    let builder = builder.with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled);
    #[cfg(not(target_os = "linux"))]
    let view = builder.build_as_child(&window).map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    let (view, surface) = {
        use gtk::prelude::*;
        use wry::WebViewBuilderExtUnix;
        let surface = gtk::OffscreenWindow::new();
        let view = builder.build_gtk(&surface).map_err(|e| e.to_string())?;
        surface.show_all();
        (view, surface)
    };
    // Capture console output only from this disposable document. Scripts have
    // no access to cookies, application files, remote resources or native APIs.
    let script = format!(
        r#"(() => {{
        const send = window.ipc.postMessage.bind(window.ipc);
        let sent = false;
        const finish = value => {{ if (!sent) {{ sent = true; send(JSON.stringify(value)); }} }};
        console.log = value => finish({{result: String(value)}});
        console.warn = console.error = () => {{}};
        addEventListener('error', e => finish({{error: String(e.message)}}));
        addEventListener('unhandledrejection', e => finish({{error: String(e.reason)}}));
        try {{ {source}
        }} catch (error) {{ finish({{error: String(error)}}); }}
    }})()"#
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut evaluated = false;
    event_loop.run(move |event, _, control| {
        *control = ControlFlow::WaitUntil(deadline);
        match event {
            Event::UserEvent(Message::Loaded) if !evaluated => {
                evaluated = true;
                if let Err(error) = view.evaluate_script(&script) {
                    super::output(serde_json::json!({"error": error.to_string()}));
                    *control = ControlFlow::Exit;
                }
            }
            Event::UserEvent(Message::Result(value)) => {
                super::output(value);
                *control = ControlFlow::Exit;
            }
            Event::MainEventsCleared if Instant::now() >= deadline => {
                super::output(serde_json::json!({"error":"WebView JavaScript timed out"}));
                *control = ControlFlow::Exit;
            }
            _ => {}
        }
        let _ = &window;
        #[cfg(target_os = "linux")]
        let _ = &surface;
    });
}
