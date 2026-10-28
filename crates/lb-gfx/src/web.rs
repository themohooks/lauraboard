//! HTML / SWF sources captured by the native system WebView host.
//! The compositor reads the latest transparent frame without waiting for IO.

use std::collections::{HashMap, VecDeque};
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
use serde_json::Value;
use tiny_skia::Pixmap;

type Result<T> = std::result::Result<T, String>;
#[derive(Default)]
struct AudioBuffer {
    samples: VecDeque<f32>,
    end: u64,
}

type Frame = Arc<Mutex<Option<Arc<Pixmap>>>>;

pub fn is_web(path: &str) -> bool {
    Path::new(path).extension().and_then(|x| x.to_str()).is_some_and(|x| ["html", "htm", "swf"].iter().any(|e| x.eq_ignore_ascii_case(e)))
}

pub struct WebSource {
    frame: Frame,
    audio: Arc<Mutex<AudioBuffer>>,
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
            audio: Arc::new(Mutex::new(AudioBuffer::default())),
            error: Arc::new(Mutex::new(None)),
            used: Arc::new(Mutex::new(Instant::now())),
            stop: Arc::new(AtomicBool::new(false)),
            created: Instant::now(),
            finished: Arc::new(AtomicBool::new(false)),
        });
        let (frame, error, used, stop) = (source.frame.clone(), source.error.clone(), source.used.clone(), source.stop.clone());
        let (path, w, h) = key.clone();
        let finished = source.finished.clone();
        let audio = source.audio.clone();
        std::thread::spawn(move || {
            if let Err(message) = capture(&path, w, h, &frame, &used, &stop, &audio) {
                tracing::warn!(path = %path.display(), %message, "browser graphics failed");
                *error.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
            }
            stop.store(true, Ordering::Relaxed);
            finished.store(true, Ordering::Relaxed);
        });
        sources.insert(key, source.clone());
        source
    }

    pub fn mix_audio(&self, cursor: &mut u64, out: &mut [f32]) {
        let queue = self.audio.lock().unwrap_or_else(|e| e.into_inner());
        let start = queue.end - queue.samples.len() as u64;
        *cursor = (*cursor).clamp(start, queue.end);
        for sample in out {
            if let Some(value) = queue.samples.get((*cursor - start) as usize) {
                *sample += value;
                *cursor += 1;
            }
        }
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

struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn capture(path: &Path, w: u32, h: u32, frame: &Frame, used: &Mutex<Instant>, stop: &AtomicBool, audio: &Mutex<AudioBuffer>) -> Result<()> {
    let executable = lb_core::browser::path().ok_or(lb_core::browser::MISSING)?;
    let server = FileServer::start(path)?;
    let mut command = Command::new(executable);
    command
        .args(["capture", &server.url, &w.to_string(), &h.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut host = Host(command.spawn().map_err(|e| format!("WebView: {e}"))?);
    let output = host.0.stdout.take().ok_or("WebView output pipe unavailable")?;
    let (tx, rx) = std::sync::mpsc::sync_channel(2);
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let start = Instant::now();
    let mut received = false;
    let mut last_frame = start;
    while !stop.load(Ordering::Relaxed) && used.lock().unwrap_or_else(|e| e.into_inner()).elapsed() < Duration::from_secs(15) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                let value: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                if let Some(error) = value["error"].as_str() {
                    return Err(format!("WebView: {error}"));
                }
                if let Some(pcm) = value["pcm"].as_str() {
                    let bytes = base64::engine::general_purpose::STANDARD.decode(pcm).map_err(|e| e.to_string())?;
                    let mut queue = audio.lock().unwrap_or_else(|e| e.into_inner());
                    queue.end += (bytes.len() / 4) as u64;
                    queue.samples.extend(bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())));
                    while queue.samples.len() > 19200 {
                        queue.samples.pop_front();
                    }
                }
                if let Some(png) = value["png"].as_str() {
                    let bytes = base64::engine::general_purpose::STANDARD.decode(png).map_err(|e| e.to_string())?;
                    let pixmap = Pixmap::decode_png(&bytes).map_err(|e| e.to_string())?;
                    *frame.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(pixmap));
                    received = true;
                    last_frame = Instant::now();
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err("WebView exited before capture completed".into()),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if !received && start.elapsed() > Duration::from_secs(35) {
            return Err("WebView capture timed out".into());
        }
        if received && last_frame.elapsed() > Duration::from_secs(10) {
            return Err("WebView stopped producing frames".into());
        }
    }
    Ok(())
}

struct FileServer {
    url: String,
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
        let init = include_str!("ruffle_init.js");
        let wrapper = format!(
            r#"<!doctype html><meta charset="utf-8"><style>html,body{{margin:0;width:100%;height:100%;overflow:hidden;background:transparent}}ruffle-player{{display:block;width:100%;height:100%}}</style><script>{init}window.__lbReady=false;window.__lbStage="loading Ruffle script";window.RufflePlayer={{config:{{autoplay:"on",volume:1,unmuteOverlay:"hidden",wmode:"transparent",allowScriptAccess:false}}}};</script><script src="{script}"></script><script>addEventListener('load',async()=>{{try{{window.__lbStage="creating Ruffle player";const p=RufflePlayer.newest().createPlayer();document.body.appendChild(p);const loaded=new Promise(resolve=>p.addEventListener('loadeddata',resolve,{{once:true}}));window.__lbStage="loading SWF";await p.ruffle().load({{url:{movie_json},autoplay:"on",wmode:"transparent"}});await loaded;window.__lbStage="waiting for animation frames";await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);window.__lbReady=true;}}catch(e){{window.__lbError=String(e);console.error(e)}}}});</script><body></body>"#
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
        Ok(Self { url: if swf { format!("{base}__lb_swf.html") } else { movie.to_string() }, stop })
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
    fn audio_readers_are_independent_and_underruns_are_silent() {
        let source = WebSource {
            frame: Default::default(),
            audio: Arc::new(Mutex::new(AudioBuffer { samples: VecDeque::from([0.25, -0.25]), end: 12 })),
            error: Default::default(),
            used: Arc::new(Mutex::new(Instant::now())),
            stop: Default::default(),
            created: Instant::now(),
            finished: Default::default(),
        };
        for _ in 0..2 {
            let mut cursor = 0;
            let mut out = [0.0; 4];
            source.mix_audio(&mut cursor, &mut out);
            assert_eq!(out, [0.25, -0.25, 0.0, 0.0]);
            assert_eq!(cursor, 12);
        }
    }

    #[test]
    #[ignore = "requires lauraboard-webview and a system WebView runtime"]
    fn web_audio_is_captured_without_desktop_playback() {
        let dir = std::env::temp_dir().join(format!("lb-audio-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.html");
        std::fs::write(
            &path,
            "<script>const ctx=new AudioContext();const tone=ctx.createOscillator();tone.connect(ctx.destination);tone.start();</script>",
        )
        .unwrap();
        let source = WebSource::get(&path, 64, 48);
        source.wait(Duration::from_secs(35)).unwrap();
        let mut cursor = 0;
        let start = Instant::now();
        loop {
            let mut samples = [0.0; 4096];
            source.mix_audio(&mut cursor, &mut samples);
            if samples.iter().any(|s| s.abs() > 0.01) {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "Web Audio never reached the capture pipe");
            std::thread::sleep(Duration::from_millis(50));
        }
        source.shutdown();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[ignore = "requires lauraboard-webview and a system WebView runtime"]
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
    #[ignore = "requires lauraboard-webview and Ruffle in target/debug/runtime"]
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
        body.extend([0, 24, 1, 0, 0x43, 0x02, 255, 0, 0]);
        // One second of 440 Hz, 44.1 kHz mono PCM, started on frame one.
        let mut sound = vec![1, 0, 0x3e];
        sound.extend(44100u32.to_le_bytes());
        for i in 0..44100 {
            let sample = ((i as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 8000.0) as i16;
            sound.extend(sample.to_le_bytes());
        }
        body.extend(((14u16 << 6) | 63).to_le_bytes());
        body.extend((sound.len() as u32).to_le_bytes());
        body.extend(sound);
        body.extend(((15u16 << 6) | 3).to_le_bytes());
        body.extend([1, 0, 0]); // StartSound, default SOUNDINFO
        body.extend(((12u16 << 6) | 2).to_le_bytes());
        body.extend([0x07, 0]); // Stop timeline, keep sound playing
        body.extend([0x40, 0, 0, 0]);
        let mut swf = b"FWS\x09".to_vec();
        swf.extend(((body.len() + 8) as u32).to_le_bytes());
        swf.extend(body);
        std::fs::write(&path, swf).unwrap();
        let external = std::env::var_os("LB_TEST_SWF").map(PathBuf::from);
        let source = WebSource::get(external.as_deref().unwrap_or(&path), 64, 48);
        source.wait(Duration::from_secs(40)).unwrap();
        if external.is_none() {
            let mut cursor = 0;
            let start = Instant::now();
            loop {
                let mut samples = [0.0; 4096];
                source.mix_audio(&mut cursor, &mut samples);
                if samples.iter().any(|s| s.abs() > 0.01) {
                    break;
                }
                assert!(start.elapsed() < Duration::from_secs(5), "SWF soundtrack was not captured");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
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
