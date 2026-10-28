//! Exercise cookie export without accessing an external account or its profile.
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires a native system WebView"]
fn javascript_solver_runs_offline_and_reports_exceptions() {
    fn evaluate(source: &str) -> serde_json::Value {
        struct Host(std::process::Child);
        impl Drop for Host {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_lauraboard-webview"));
        command.arg("javascript").stdin(Stdio::piped()).stdout(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut host = Host(command.spawn().unwrap());
        host.0.stdin.take().unwrap().write_all(source.as_bytes()).unwrap();
        let output = host.0.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let line = std::io::BufReader::new(output).lines().next().unwrap().unwrap();
            let _ = tx.send(line);
        });
        let line = rx.recv_timeout(Duration::from_secs(70)).expect("JavaScript helper did not reply");
        serde_json::from_str(&line).unwrap()
    }
    let result = evaluate(
        r#"fetch('https://example.invalid/').then(
        () => console.log(JSON.stringify({blocked:false})),
        () => console.log(JSON.stringify({blocked:true, answer:6*7}))
    );"#,
    );
    let value: serde_json::Value = serde_json::from_str(result["result"].as_str().unwrap()).unwrap();
    assert_eq!(value["answer"], 42);
    assert_eq!(value["blocked"], true);
    let error = evaluate("throw new Error('fixture exception');");
    assert!(error["error"].as_str().unwrap().contains("fixture exception"));
}

#[cfg(windows)]
#[test]
#[ignore = "requires Windows WebView2"]
fn capture_never_shows_a_native_window() {
    use windows::Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible},
    };
    use windows::core::BOOL;
    unsafe extern "system" fn inspect(hwnd: HWND, data: LPARAM) -> BOOL {
        // EnumWindows calls synchronously while this stack-owned state lives.
        let state = unsafe { &mut *(data.0 as *mut (u32, bool)) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        if pid == state.0 && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            use windows::Win32::UI::WindowsAndMessaging::{
                GWL_EXSTYLE, GetWindowLongPtrW, GetWindowTextLengthW, WS_EX_LAYERED, WS_EX_TRANSPARENT,
            };
            let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
            // Tao's untitled transparent layered message target sets WS_VISIBLE
            // for event delivery, but has no painted user-facing surface.
            if style & WS_EX_LAYERED.0 != 0 && style & WS_EX_TRANSPARENT.0 != 0 && unsafe { GetWindowTextLengthW(hwnd) } == 0 {
                return BOOL(1);
            }
            state.1 = true;
        }
        BOOL(1)
    }
    struct Host(std::process::Child);
    impl Drop for Host {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_lauraboard-webview"));
    command.args(["capture", "data:text/html,<body>capture</body>", "96", "64"]).stdin(Stdio::piped()).stdout(Stdio::piped());
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000);
    let mut host = Host(command.spawn().unwrap());
    let output = host.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(output).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let mut state = (host.0.id(), false);
    let start = Instant::now();
    let mut frame = false;
    while start.elapsed() < Duration::from_secs(15) {
        unsafe {
            EnumWindows(Some(inspect), LPARAM(&mut state as *mut _ as isize)).unwrap();
        }
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(10)) {
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert!(value["error"].is_null(), "{value}");
            if value["png"].is_string() {
                frame = true;
                break;
            }
        }
    }
    assert!(frame, "Hidden WebView did not produce a frame");
    assert!(!state.1, "Capture exposed a visible native window");
}

#[test]
#[ignore = "requires a native system WebView and display"]
fn exports_http_only_cookies_and_closes_on_done() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let requests = Arc::new(AtomicUsize::new(0));
    let served = requests.clone();
    let pending_resource = Arc::new(AtomicBool::new(false));
    let pending_seen = pending_resource.clone();
    let ended = stop.clone();
    let server = std::thread::spawn(move || {
        let mut pending_streams = Vec::new();
        while !ended.load(Ordering::Relaxed) {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                // Drain the complete request before closing the TCP socket;
                // unread headers can reset the response on Unix clients.
                let mut reader = std::io::BufReader::new(&mut stream);
                let mut complete = false;
                let mut slow_resource = false;
                let mut bytes = 0;
                for line_index in 0..64 {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => bytes += n,
                    }
                    if line_index == 0 {
                        slow_resource = line.starts_with("GET /pending ");
                    }
                    if bytes > 8192 {
                        break;
                    }
                    if line == "\r\n" || line == "\n" {
                        complete = true;
                        break;
                    }
                }
                drop(reader);
                if !complete {
                    continue;
                }
                served.fetch_add(1, Ordering::Relaxed);
                if slow_resource {
                    // Keep a subresource pending so PageLoadEvent::Finished
                    // cannot gate cookie export, even on a fast test machine.
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nContent-Type: image/png\r\n\r\n");
                    pending_streams.push(stream);
                    pending_seen.store(true, Ordering::Relaxed);
                    continue;
                }
                let body = b"<!doctype html><p>WebView</p><img src='/pending'>";
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nSet-Cookie: lb-test=session; HttpOnly; Path=/; SameSite=Lax\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes()).and_then(|_| stream.write_all(body));
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    });
    let profile = std::env::temp_dir().join(format!("lb-webview-native-{}", std::process::id()));
    struct Host(std::process::Child);
    impl Drop for Host {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_lauraboard-webview"));
    command.arg("login").arg(&url).arg(&profile).arg("-").stdin(Stdio::piped()).stdout(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut host = Host(command.spawn().unwrap());
    let output = host.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(output).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                let _ = tx.send(value);
            }
        }
    });
    let start = Instant::now();
    let mut found = false;
    let mut last_cookie_count = None;
    let mut premature_exit = None;
    // Cold WebKit/Xvfb initialization can take ~16 s even after receiving
    // the HTTP response. Keep startup budget separate from the 5 s close
    // requirement; the pending subresource still makes the old load gate fail.
    while start.elapsed() < Duration::from_secs(60) {
        if let Some(status) = host.0.try_wait().unwrap() {
            premature_exit = Some(status);
            break;
        }
        if let Ok(value) = rx.recv_timeout(Duration::from_millis(100)) {
            assert!(value["error"].is_null(), "{value}");
            if let Some(cookies) = value["cookies"].as_array() {
                last_cookie_count = Some(cookies.len());
                found = cookies.iter().any(|c| c["name"] == "lb-test" && c["value"] == "session" && c["httpOnly"] == true);
                if found && pending_resource.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }
    if premature_exit.is_none() {
        let _ = host.0.stdin.as_mut().unwrap().write_all(b"done\n");
    }
    let start = Instant::now();
    let closed = loop {
        if let Some(status) = host.0.try_wait().unwrap() {
            break status.success();
        }
        if start.elapsed() > Duration::from_secs(5) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    let _ = std::fs::remove_dir_all(profile);
    assert!(
        found,
        "HttpOnly cookie was not exported: requests={}, last_cookie_count={last_cookie_count:?}, premature_exit={premature_exit:?}, closed={closed}",
        requests.load(Ordering::Relaxed)
    );
    assert!(closed, "WebView did not close after the parent sent done");
    assert!(pending_resource.load(Ordering::Relaxed), "Slow subresource was not requested");
}

// Ruffle waits for animation frames before it can produce its first snapshot.
// A timer-only HTML test cannot detect WKWebView suspending rAF while hidden.
#[test]
#[ignore = "requires a native system WebView runtime"]
fn hidden_capture_runs_animation_frames() {
    struct Host(std::process::Child);
    impl Drop for Host {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let html = "data:text/html,<script>window.__lbReady=false;let n=0;const cancelled=requestAnimationFrame(()=>window.__lbError='cancelled callback ran');cancelAnimationFrame(cancelled);function tick(){if(++n===5)window.__lbReady=true;else requestAnimationFrame(tick)}requestAnimationFrame(tick)</script>";
    let mut command = Command::new(env!("CARGO_BIN_EXE_lauraboard-webview"));
    command.args(["capture", html, "64", "48"]).stdin(Stdio::piped()).stdout(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut host = Host(command.spawn().unwrap());
    let output = host.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(output).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let line = rx.recv_timeout(Duration::from_secs(40)).expect("Hidden animation did not produce a snapshot");
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert!(value["error"].is_null(), "{value}");
    assert!(value["png"].is_string(), "{value}");
    host.0.stdin.as_mut().unwrap().write_all(b"done\n").unwrap();
}
