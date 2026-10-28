//! Exercise cookie export without accessing an external account or its profile.
use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

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
    let ended = stop.clone();
    let server = std::thread::spawn(move || {
        while !ended.load(Ordering::Relaxed) {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut request = [0; 4096];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\nContent-Type: text/html\r\nSet-Cookie: lb-test=session; HttpOnly; Path=/; SameSite=Lax\r\nConnection: close\r\n\r\n<p>WebView</p>\n");
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
    while start.elapsed() < Duration::from_secs(20) {
        if let Ok(value) = rx.recv_timeout(Duration::from_millis(100)) {
            assert!(value["error"].is_null(), "{value}");
            if let Some(cookies) = value["cookies"].as_array() {
                found = cookies.iter().any(|c| c["name"] == "lb-test" && c["value"] == "session" && c["httpOnly"] == true);
                if found {
                    break;
                }
            }
        }
    }
    host.0.stdin.as_mut().unwrap().write_all(b"done\n").unwrap();
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
    assert!(found, "HttpOnly cookie was not exported");
    assert!(closed, "WebView did not close after the parent sent done");
}
