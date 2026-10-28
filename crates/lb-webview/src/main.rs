//! Small native WebView host shared by sign-in and HTML/SWF capture.
//! No browser is distributed or discovered. stdout is a private JSON-lines pipe.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod cookies;
mod snapshot;

use base64::Engine;
use serde_json::json;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::{WebContext, WebViewBuilder};

#[derive(Debug)]
enum Message {
    Stop,
    Loaded,
    Ready(String),
    Frame(Result<Vec<u8>, String>),
    Navigate(String),
    Cookies(serde_json::Value),
}

fn output(value: serde_json::Value) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{value}").and_then(|_| out.flush()).is_err() {
        std::process::exit(0);
    }
}

fn main() {
    if let Err(error) = run() {
        output(json!({"error": error}));
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let login = args.first().is_some_and(|v| v == "login");
    if args.len() != 4 || !matches!(args[0].as_str(), "login" | "capture") {
        return Err("usage: lauraboard-webview login URL PROFILE - | capture URL WIDTH HEIGHT".into());
    }
    let url = args[1].clone();
    let (width, height) = if login {
        (960, 720)
    } else {
        (
            args[2].parse::<u32>().map_err(|e| e.to_string())?.clamp(1, 4096),
            args[3].parse::<u32>().map_err(|e| e.to_string())?.clamp(1, 4096),
        )
    };
    let event_loop = EventLoopBuilder::<Message>::with_user_event().build();
    #[cfg(target_os = "macos")]
    let event_loop = {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        let mut event_loop = event_loop;
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        event_loop
    };
    let mut window_builder = WindowBuilder::new()
        .with_title("LauraBoard — вход в YouTube")
        .with_inner_size(tao::dpi::PhysicalSize::new(width, height))
        .with_visible(true)
        .with_focused(login)
        .with_decorations(login)
        .with_resizable(login);
    #[cfg(target_os = "linux")]
    {
        window_builder = window_builder.with_visible(login);
    }
    if !login {
        window_builder = window_builder.with_position(tao::dpi::PhysicalPosition::new(-30000, -30000));
    }
    #[cfg(windows)]
    {
        use tao::platform::windows::WindowBuilderExtWindows;
        window_builder = window_builder.with_skip_taskbar(!login);
    }
    let window = window_builder.build(&event_loop).map_err(|e| e.to_string())?;
    let profile = if login { Some(PathBuf::from(&args[2])) } else { None };
    if let Some(dir) = &profile {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut context = WebContext::new(profile);
    let nav = event_loop.create_proxy();
    let loaded = event_loop.create_proxy();
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(&url)
        .with_incognito(!login)
        .with_bounds(wry::Rect {
            position: wry::dpi::PhysicalPosition::new(0, 0).into(),
            size: wry::dpi::PhysicalSize::new(width, height).into(),
        })
        .with_on_page_load_handler(move |event, _| {
            if matches!(event, wry::PageLoadEvent::Finished) {
                let _ = loaded.send_event(Message::Loaded);
            }
        })
        .with_transparent(!login)
        .with_autoplay(!login)
        .with_new_window_req_handler(move |url, _| {
            // Keep sign-in popups in this app-owned WebView.
            let _ = nav.send_event(Message::Navigate(url));
            wry::NewWindowResponse::Deny
        });
    #[cfg(target_os = "macos")]
    let builder = {
        use wry::WebViewBuilderExtDarwin;
        // Stable app-specific sign-in store; capture views remain ephemeral.
        builder.with_data_store_identifier(*b"LauraBoard-login")
    };
    #[cfg(not(target_os = "linux"))]
    let view = if login { builder.build(&window) } else { builder.build_as_child(&window) }.map_err(|e| format!("System WebView: {e}"))?;
    #[cfg(target_os = "linux")]
    let offscreen = if login {
        None
    } else {
        use gtk::prelude::*;
        let surface = gtk::OffscreenWindow::new();
        surface.set_default_size(width as i32, height as i32);
        Some(surface)
    };
    #[cfg(target_os = "linux")]
    let view = {
        use gtk::prelude::*;
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        window.gtk_window().set_skip_taskbar_hint(!login);
        let result = if let Some(surface) = &offscreen {
            let result = builder.build_gtk(surface);
            surface.show_all();
            result
        } else {
            builder.build_gtk(window.gtk_window())
        };
        result.map_err(|e| format!("WebKitGTK: {e}"))?
    };
    if !login {
        // Physical output pixels correspond to CSS pixels, regardless of desktop DPI.
        let _ = view.zoom(1.0 / window.scale_factor());
        snapshot::mute(&view);
    }
    let stop = event_loop.create_proxy();
    std::thread::spawn(move || {
        // Closing the parent pipe also ends the helper, including a crashed parent.
        let _ = std::io::stdin().lock().lines().next();
        let _ = stop.send_event(Message::Stop);
    });
    let proxy = event_loop.create_proxy();
    let start = Instant::now();
    let mut next = Instant::now();
    let mut ready = login;
    let mut loaded = false;
    let mut pending = false;
    let mut cookie_pending = false;
    let mut closing: Option<Instant> = None;
    event_loop.run(move |event, _, control| {
        *control = ControlFlow::WaitUntil(next);
        match event {
            Event::UserEvent(Message::Stop) | Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if login {
                    closing = Some(Instant::now());
                    next = Instant::now();
                } else {
                    *control = ControlFlow::Exit;
                }
            }
            Event::UserEvent(Message::Navigate(url)) => {
                let _ = view.load_url(&url);
            }
            Event::UserEvent(Message::Loaded) => {
                loaded = true;
                if !login {
                    snapshot::mute(&view);
                }
            }
            Event::UserEvent(Message::Cookies(value)) => {
                cookie_pending = false;
                output(value);
                if closing.is_some() {
                    *control = ControlFlow::Exit;
                }
            }
            Event::UserEvent(Message::Ready(result)) => {
                pending = false;
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result) {
                    if let Some(error) = v["error"].as_str() {
                        output(json!({"error": error}));
                        *control = ControlFlow::Exit;
                    }
                    ready = v["ready"] == true;
                }
            }
            Event::UserEvent(Message::Frame(result)) => {
                pending = false;
                match result {
                    Ok(png) => output(json!({"png": base64::engine::general_purpose::STANDARD.encode(png)})),
                    Err(error) => {
                        output(json!({"error": error}));
                        *control = ControlFlow::Exit;
                    }
                }
            }
            Event::MainEventsCleared if Instant::now() >= next => {
                next = Instant::now() + if login { Duration::from_secs(2) } else { Duration::from_millis(33) };
                *control = ControlFlow::WaitUntil(next);
                if login && loaded && !cookie_pending {
                    let proxy = proxy.clone();
                    cookie_pending = true;
                    if let Err(error) = cookies::read(&view, move |value| {
                        let _ = proxy.send_event(Message::Cookies(value));
                    }) {
                        cookie_pending = false;
                        output(json!({"error":error}));
                    }
                } else if !login && loaded && !pending {
                    pending = true;
                    let proxy = proxy.clone();
                    if ready {
                        snapshot::capture(&view, move |frame| {
                            let _ = proxy.send_event(Message::Frame(frame));
                        });
                    } else {
                        if snapshot::readiness(&view, move |v| {
                            let _ = proxy.send_event(Message::Ready(v));
                        })
                        .is_err()
                        {
                            pending = false;
                        }
                    }
                }
                if !login && !ready && start.elapsed() > Duration::from_secs(30) {
                    output(json!({"error":"HTML / Ruffle loading timed out"}));
                    *control = ControlFlow::Exit;
                }
                if closing.is_some_and(|t| t.elapsed() > Duration::from_secs(3)) {
                    *control = ControlFlow::Exit;
                }
            }
            _ => {}
        }
        // Retain the native parent and shared WebContext throughout the loop.
        let _ = (&window, &context);
        #[cfg(target_os = "linux")]
        let _ = &offscreen;
    });
}
