//! HTML / SWF sources captured in a separate headless Chromium process.
//! The compositor reads the latest transparent frame without waiting for IO.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Value, json};
use tiny_skia::Pixmap;

type Result<T> = std::result::Result<T, String>;
type Frame = Arc<Mutex<Option<Arc<Pixmap>>>>;

pub fn is_web(path: &str) -> bool {
    Path::new(path).extension().and_then(|x| x.to_str()).is_some_and(|x| ["html", "htm", "swf"].iter().any(|e| x.eq_ignore_ascii_case(e)))
}

pub struct WebSource {
    frame: Frame,
    error: Arc<Mutex<Option<String>>>,
    used: Arc<Mutex<Instant>>,
    stop: Arc<AtomicBool>,
    created: Instant,
    finished: Arc<AtomicBool>,
}

impl Drop for WebSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

type Key = (PathBuf, u32, u32);
static SOURCES: OnceLock<Mutex<HashMap<Key, Arc<WebSource>>>> = OnceLock::new();

impl WebSource {
    pub fn get(path: &Path, w: u32, h: u32) -> Arc<Self> {
        let key = (path.to_path_buf(), w.clamp(1, 4096), h.clamp(1, 4096));
        let mut sources = SOURCES.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        sources.retain(|_, s| {
            !s.stop.load(Ordering::Relaxed)
                || (s.error.lock().unwrap_or_else(|e| e.into_inner()).is_some() && s.created.elapsed() < Duration::from_secs(30))
        });
        if let Some(source) = sources.get(&key) {
            *source.used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
            return source.clone();
        }
        let source = Arc::new(Self {
            frame: Arc::new(Mutex::new(None)),
            error: Arc::new(Mutex::new(None)),
            used: Arc::new(Mutex::new(Instant::now())),
            stop: Arc::new(AtomicBool::new(false)),
            created: Instant::now(),
            finished: Arc::new(AtomicBool::new(false)),
        });
        let (frame, error, used, stop) = (source.frame.clone(), source.error.clone(), source.used.clone(), source.stop.clone());
        let (path, w, h) = key.clone();
        let finished = source.finished.clone();
        std::thread::spawn(move || {
            if let Err(message) = capture(&path, w, h, &frame, &used, &stop) {
                tracing::warn!(path = %path.display(), %message, "browser graphics failed");
                *error.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
            }
            stop.store(true, Ordering::Relaxed);
            finished.store(true, Ordering::Relaxed);
        });
        sources.insert(key, source.clone());
        source
    }

    pub fn frame(&self) -> Option<Arc<Pixmap>> {
        *self.used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
        self.frame.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The editor can wait for its first frame; live playout never waits.
    pub fn wait(&self, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        loop {
            if self.frame().is_some() {
                return Ok(());
            }
            if let Some(error) = self.error.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                return Err(error);
            }
            if start.elapsed() >= timeout {
                return Err("browser source did not produce a frame".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let start = Instant::now();
        while !self.finished.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(6) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Close isolated browser processes before the application exits.
pub fn shutdown_all() {
    if let Some(sources) = SOURCES.get() {
        let sources = std::mem::take(&mut *sources.lock().unwrap_or_else(|e| e.into_inner()));
        for source in sources.values() {
            source.stop.store(true, Ordering::Relaxed);
        }
        for source in sources.values() {
            source.shutdown();
        }
    }
}

pub use lb_core::browser::path as browser_path;

struct Browser {
    child: Child,
    profile: PathBuf,
    #[cfg(windows)]
    _job: Option<BrowserJob>,
}

// Closing the application must also close Chromium's renderer processes,
// including when the embedded engine is terminated with its GUI process.
#[cfg(windows)]
struct BrowserJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl BrowserJob {
    fn attach(child: &Child) -> Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let guard = Self(handle);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
            || unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(guard)
    }
}
#[cfg(windows)]
impl Drop for BrowserJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(windows)]
        {
            self._job.take();
        }
        // Only the unique directory created for this browser is removed.
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

struct Cdp {
    socket: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    next: u64,
}
impl Cdp {
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        self.socket
            .send(tungstenite::Message::Text(json!({"id": id, "method": method, "params": params}).to_string().into()))
            .map_err(|e| e.to_string())?;
        loop {
            let message = self.socket.read().map_err(|e| e.to_string())?;
            if let tungstenite::Message::Text(text) = message {
                let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                if v["id"] == id {
                    if !v["error"].is_null() {
                        return Err(format!("{method}: {}", v["error"]));
                    }
                    return Ok(v["result"].clone());
                }
            }
        }
    }
}

fn capture(path: &Path, w: u32, h: u32, frame: &Frame, used: &Mutex<Instant>, stop: &AtomicBool) -> Result<()> {
    let executable = browser_path().ok_or(lb_core::browser::MISSING)?;
    let server = FileServer::start(path)?;
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // A previous crashed instance may have left this PID's directory behind.
    // Reserve a fresh directory atomically instead of reusing its profile.
    let profile = loop {
        let path =
            std::env::temp_dir().join(format!("lauraboard-browser-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        match std::fs::create_dir(&path) {
            Ok(()) => break path,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    };
    let mut command = Command::new(executable);
    command
        .args([
            "--headless=new",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding",
            "--hide-scrollbars",
            "--mute-audio",
            "--autoplay-policy=no-user-gesture-required",
            "--remote-debugging-port=0",
            "--remote-debugging-address=127.0.0.1",
            "--use-angle=swiftshader",
            "--enable-unsafe-swiftshader",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_dir(&profile);
            return Err(e.to_string());
        }
    };
    let mut browser = Browser {
        child,
        profile,
        #[cfg(windows)]
        _job: None,
    };
    #[cfg(windows)]
    {
        browser._job = Some(BrowserJob::attach(&browser.child)?);
    }
    let start = Instant::now();
    let port = loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if let Ok(contents) = std::fs::read_to_string(browser.profile.join("DevToolsActivePort")) {
            if let Some(port) = contents.lines().next().and_then(|p| p.parse::<u16>().ok()) {
                break port;
            }
        }
        if let Some(status) = browser.child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("LauraBoard web runtime exited before capture started: {status}"));
        }
        if start.elapsed() > Duration::from_secs(10) {
            return Err("browser debugging endpoint timed out".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let cfg = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(3))).build();
    let text = ureq::Agent::new_with_config(cfg)
        .get(format!("http://127.0.0.1:{port}/json/list"))
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let targets: Vec<Value> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let ws = targets
        .iter()
        .find(|t| t["type"] == "page")
        .and_then(|t| t["webSocketDebuggerUrl"].as_str())
        .ok_or("browser has no page target")?;
    let (socket, _) = tungstenite::connect(ws).map_err(|e| e.to_string())?;
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_ref() {
        stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    }
    let mut cdp = Cdp { socket, next: 0 };
    cdp.call("Page.enable", json!({}))?;
    cdp.call("Emulation.setDeviceMetricsOverride", json!({"width": w, "height": h, "deviceScaleFactor": 1, "mobile": false}))?;
    cdp.call("Emulation.setDefaultBackgroundColorOverride", json!({"color": {"r": 0, "g": 0, "b": 0, "a": 0}}))?;
    let nav = cdp.call("Page.navigate", json!({"url": server.url}))?;
    if let Some(error) = nav["errorText"].as_str() {
        return Err(error.into());
    }
    let loading = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let expression = if server.swf {
            "({ready: window.__lbReady === true, error: window.__lbError || null})"
        } else {
            "({ready: document.readyState === 'complete'})"
        };
        let ready = cdp.call("Runtime.evaluate", json!({"expression": expression, "returnByValue": true}))?;
        if let Some(error) = ready["result"]["value"]["error"].as_str() {
            return Err(format!("Ruffle: {error}"));
        }
        if ready["result"]["value"]["ready"] == true {
            break;
        }
        if loading.elapsed() > Duration::from_secs(20) {
            return Err("HTML / Ruffle loading timed out".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    while !stop.load(Ordering::Relaxed) && used.lock().unwrap_or_else(|e| e.into_inner()).elapsed() < Duration::from_secs(15) {
        let start = Instant::now();
        let result =
            cdp.call("Page.captureScreenshot", json!({"format": "png", "captureBeyondViewport": false, "optimizeForSpeed": true}))?;
        let png = result["data"].as_str().ok_or("browser returned no image")?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(png).map_err(|e| e.to_string())?;
        let pixmap = Pixmap::decode_png(&bytes).map_err(|e| e.to_string())?;
        *frame.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(pixmap));
        std::thread::sleep(Duration::from_millis(33).saturating_sub(start.elapsed()));
    }
    let _ = cdp.call("Browser.close", json!({}));
    Ok(())
}

struct FileServer {
    url: String,
    swf: bool,
    stop: Arc<AtomicBool>,
}
impl Drop for FileServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl FileServer {
    fn start(file: &Path) -> Result<Self> {
        let file = file.canonicalize().map_err(|e| e.to_string())?;
        let root = file.parent().ok_or("source has no parent directory")?.to_path_buf();
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let base = format!("http://{}/", listener.local_addr().map_err(|e| e.to_string())?);
        let swf = file.extension().is_some_and(|x| x.eq_ignore_ascii_case("swf"));
        let mut movie = url::Url::parse(&base).map_err(|e| e.to_string())?;
        movie.set_path(file.file_name().ok_or("source has no filename")?.to_str().ok_or("source filename is not UTF-8")?);
        let ruffle = std::env::var_os("LAURABOARD_RUFFLE")
            .map(PathBuf::from)
            .or_else(lb_core::browser::ruffle_path)
            .filter(|p| p.join("ruffle.js").is_file());
        if swf && ruffle.is_none() {
            return Err("Не найден SWF-плеер LauraBoard в runtime/ruffle".into());
        }
        let script = "/__ruffle/ruffle.js";
        let movie_json = serde_json::to_string(movie.as_str()).unwrap().replace('<', "\\u003c");
        let wrapper = format!(
            r#"<!doctype html><meta charset="utf-8"><style>html,body{{margin:0;width:100%;height:100%;overflow:hidden;background:transparent}}ruffle-player{{display:block;width:100%;height:100%}}</style><script>window.RufflePlayer={{config:{{autoplay:"on",unmuteOverlay:"hidden",wmode:"transparent",allowScriptAccess:false}}}};</script><script src="{script}"></script><script>addEventListener('load',async()=>{{try{{const p=RufflePlayer.newest().createPlayer();document.body.appendChild(p);const loaded=new Promise(resolve=>p.addEventListener('loadeddata',resolve,{{once:true}}));await p.ruffle().load({{url:{movie_json},autoplay:"on",wmode:"transparent"}});await loaded;await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);window.__lbReady=true;}}catch(e){{window.__lbError=String(e);console.error(e)}}}});</script><body></body>"#
        );
        let stop = Arc::new(AtomicBool::new(false));
        let ended = stop.clone();
        std::thread::spawn(move || {
            while !ended.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve_file(stream, &root, ruffle.as_deref(), &wrapper),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
                    Err(_) => break,
                }
            }
        });
        Ok(Self { url: if swf { format!("{base}__lb_swf.html") } else { movie.to_string() }, swf, stop })
    }
}

fn serve_file(mut stream: TcpStream, root: &Path, ruffle: Option<&Path>, wrapper: &str) {
    // Windows can inherit the listener's nonblocking mode on accepted
    // sockets. Individual requests need blocking reads with a timeout.
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let mut reader = BufReader::new(&mut stream).take(64 * 1024);
    let mut request = String::new();
    if reader.by_ref().take(8193).read_line(&mut request).is_err() || request.len() > 8192 || !request.ends_with('\n') {
        return;
    }
    // Dropping a TCP socket with unread request headers can reset the
    // connection and discard the response on the client (notably macOS).
    // Wait for the empty line, while bounding the entire request head.
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header).unwrap_or(0) == 0 {
            return;
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
    }
    drop(reader);
    let raw = request.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/");
    let Ok(path) = percent_encoding::percent_decode_str(raw).decode_utf8() else { return };
    let (body, content_type) = if path == "/__lb_swf.html" {
        (Some(wrapper.as_bytes().to_vec()), "text/html")
    } else {
        let (base, relative) =
            if let Some(p) = path.strip_prefix("/__ruffle/") { (ruffle.unwrap_or(root), p) } else { (root, path.trim_start_matches('/')) };
        let full = base.join(relative).canonicalize().ok();
        let body = full.as_ref().filter(|p| base.canonicalize().ok().is_some_and(|b| p.starts_with(b))).and_then(|p| std::fs::read(p).ok());
        let mime = match Path::new(relative).extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
            "html" | "htm" => "text/html; charset=utf-8",
            "js" | "mjs" => "application/javascript",
            "wasm" => "application/wasm",
            "css" => "text/css",
            "json" => "application/json",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "swf" => "application/x-shockwave-flash",
            _ => "application/octet-stream",
        };
        (body, mime)
    };
    let status = if body.is_some() { "200 OK" } else { "404 Not Found" };
    let body = body.unwrap_or_default();
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes()).and_then(|_| stream.write_all(&body));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the bundled runtime in target/debug/runtime"]
    fn html_animation_preserves_alpha_and_updates_frames() {
        let dir = std::env::temp_dir().join(format!("lb-web-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let html = dir.join("animated графика.html");
        std::fs::write(&html, r#"<!doctype html><style>html,body{margin:0;background:transparent}div{width:32px;height:32px;background:rgba(255,0,0,.5)}</style><div></div><script>setInterval(()=>document.querySelector('div').style.background=`rgba(${Math.floor(Date.now()/30)%255},0,0,.5)`,30)</script>"#).unwrap();
        let source = WebSource::get(&html, 64, 48);
        source.wait(Duration::from_secs(15)).unwrap();
        let first = source.frame().unwrap();
        assert_eq!((first.width(), first.height()), (64, 48));
        assert_eq!(first.pixel(50, 40).unwrap().alpha(), 0);
        assert!(first.pixel(5, 5).unwrap().alpha().abs_diff(128) <= 1);
        std::thread::sleep(Duration::from_millis(300));
        let second = source.frame().unwrap();
        assert_ne!(first.data(), second.data(), "HTML animation must keep advancing");
        source.shutdown();
        let _ = std::fs::remove_file(html);
        let _ = std::fs::remove_dir(dir);
    }

    #[test]
    #[ignore = "requires the bundled browser and Ruffle in target/debug/runtime"]
    fn swf_is_loaded_by_ruffle() {
        let dir = std::env::temp_dir().join(format!("lb-swf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.swf");
        // Uncompressed one-frame SWF: 64×48 stage, red background.
        // Transparent wmode intentionally suppresses the stage background.
        let bits = format!("{:05b}{:012b}{:012b}{:012b}{:012b}", 12, 0, 1280, 0, 960);
        let padded = format!("{bits}000");
        let mut body: Vec<u8> =
            padded.as_bytes().chunks(8).map(|chunk| u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 2).unwrap()).collect();
        body.extend([0, 24, 1, 0, 0x43, 0x02, 255, 0, 0, 0x40, 0, 0, 0]);
        let mut swf = b"FWS\x09".to_vec();
        swf.extend(((body.len() + 8) as u32).to_le_bytes());
        swf.extend(body);
        std::fs::write(&path, swf).unwrap();
        let external = std::env::var_os("LB_TEST_SWF").map(PathBuf::from);
        let source = WebSource::get(external.as_deref().unwrap_or(&path), 64, 48);
        source.wait(Duration::from_secs(30)).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let frame = source.frame().unwrap();
        assert_eq!((frame.width(), frame.height()), (64, 48));
        if external.is_none() {
            assert!(frame.data().chunks_exact(4).all(|p| p[3] == 0), "transparent SWF must preserve alpha");
        }
        if let Some(output) = std::env::var_os("LB_TEST_FRAME") {
            frame.save_png(output).unwrap();
        }
        source.shutdown();
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(dir);
    }

    #[test]
    fn local_server_keeps_sources_inside_their_directory() {
        use std::io::Read;
        let dir = std::env::temp_dir().join(format!("lb-web-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("source");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.join("outside.txt");
        std::fs::write(&outside, "not a source asset").unwrap();
        let html = root.join("test.html");
        std::fs::write(&html, "<b>ok</b>").unwrap();
        let server = FileServer::start(&html).unwrap();
        let url = url::Url::parse(&server.url).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", url.port().unwrap())).unwrap();
        write!(stream, "GET /%2e%2e/outside.txt HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 404"));
        let _ = std::fs::remove_file(html);
        let _ = std::fs::remove_file(outside);
        let _ = std::fs::remove_dir(root);
        let _ = std::fs::remove_dir(dir);
    }

    #[test]
    fn local_server_waits_for_complete_request_headers() {
        use std::io::{ErrorKind, Read};
        let dir = std::env::temp_dir().join(format!("lb-web-headers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let html = dir.join("test.html");
        std::fs::write(&html, "<b>complete response</b>").unwrap();
        let server = FileServer::start(&html).unwrap();
        let url = url::Url::parse(&server.url).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", url.port().unwrap())).unwrap();
        stream.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        // Separate TCP writes make the premature response reproducible, even
        // on platforms that usually buffer the entire request in one read.
        for part in ["GET /test.html HTTP/1.1\r\n", "Host: localhost\r\nX-Test: fragmented\r\n"] {
            stream.write_all(part.as_bytes()).unwrap();
            let mut byte = [0];
            let error = stream.read(&mut byte).expect_err("server replied before the end of the request headers");
            assert!(matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut), "{error}");
        }
        stream.write_all(b"\r\n").unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        assert!(response.ends_with("\r\n\r\n<b>complete response</b>"), "{response}");
        let _ = std::fs::remove_file(html);
        let _ = std::fs::remove_dir(dir);
    }
}
