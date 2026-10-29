//! Graphics thread: renders overlay frames and streams them to the encoder
//! over a local TCP connection.
//!
//! Rendering and transport have independent clocks. A slow render only
//! freezes the overlay: the video pump repeats the latest complete frame,
//! and the audio pump reads browser PCM without waiting for the compositor.
//! There is no render backlog; a single latest-frame slot bounds memory.

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lb_core::graphics::{self, ActiveGraphic, GfxContext, GraphicsConfig, Manual, ProgramClock};
use lb_core::rundown::RundownItem;
use lb_core::{WallTime, diff_ms};
use lb_gfx::{Compositor, FrameContext};

/// State shared between the channel actor and the graphics thread.
#[derive(Default)]
pub struct GfxState {
    pub cfg: GraphicsConfig,
    pub cfg_version: u64,
    pub profile: Option<String>,
    pub on_air: Option<(RundownItem, WallTime)>,
    pub next_program_start: Option<WallTime>,
    /// The programme of the item on air.
    pub program: Option<ProgramClock>,
    pub vars: HashMap<String, String>,
    pub manual: HashMap<String, Manual>,
    /// Last evaluated active set (for status).
    pub active: Vec<ActiveGraphic>,
}

pub type SharedGfx = Arc<Mutex<GfxState>>;

pub struct GfxThread {
    pub port: u16,
    pub audio_port: u16,
    /// Frames the connected encoder has processed (from its progress
    /// reports); reset when an encoder starts.
    pub consumed: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// Frames are sent this much ahead of the wall clock, so the overlay never
/// holds the video back.
const LEAD_MS: i64 = 300;
/// Seconds of frames allowed beyond the encoder's reported progress
/// (reports come once a second).
const MAX_AHEAD_S: f64 = 2.5;

fn lock(shared: &SharedGfx) -> MutexGuard<'_, GfxState> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

fn now() -> WallTime {
    chrono::Local::now().naive_local()
}

/// Evaluate which elements are visible now.
pub fn evaluate(st: &GfxState, at: WallTime) -> Vec<ActiveGraphic> {
    let (item, elapsed, remaining) = match &st.on_air {
        Some((it, started)) => {
            let el = diff_ms(at, *started).max(0);
            (Some(it), el, (it.duration_ms - el).max(0))
        }
        None => (None, 0, 0),
    };
    let ctx = GfxContext {
        now: at,
        item,
        elapsed_ms: elapsed,
        remaining_ms: remaining,
        profile: st.profile.as_deref(),
        manual: &st.manual,
        vars: Some(&st.vars),
    };
    graphics::evaluate(&st.cfg, &ctx)
}

impl GfxThread {
    /// `fps` is the overlay frame rate (the video frame rate, so motion is
    /// smooth).
    pub fn spawn(shared: SharedGfx, width: u32, height: u32, fps: f64, root: PathBuf, channel: String) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let audio_listener = TcpListener::bind("127.0.0.1:0")?;
        audio_listener.set_nonblocking(true)?;
        let audio_port = audio_listener.local_addr()?.port();
        let latest = Arc::new(Mutex::new(LiveFrame { rgba: vec![0; width as usize * height as usize * 4].into(), sources: Vec::new() }));
        let consumed = Arc::new(AtomicU64::new(0));
        let consumed2 = consumed.clone();
        let (audio_latest, audio_stop, audio_consumed) = (latest.clone(), stop.clone(), consumed.clone());
        std::thread::Builder::new().name(format!("gfx-audio-{channel}")).spawn(move || {
            while !audio_stop.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = audio_listener.accept() else {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                };
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                let _ = stream.set_read_timeout(Some(Duration::from_millis(1)));
                let start = Instant::now();
                let mut blocks = 0u64;
                let mut cursors = HashMap::<usize, u64>::new();
                while !audio_stop.load(Ordering::Relaxed) {
                    // Bound audio just like video when the encoder is stalled.
                    if blocks as f64 * 0.01 > audio_consumed.load(Ordering::Relaxed) as f64 / fps.max(1.0) + MAX_AHEAD_S {
                        if peer_closed(&stream) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    let sources = audio_latest.lock().unwrap_or_else(|e| e.into_inner()).sources.clone();
                    cursors.retain(|key, _| sources.iter().any(|s| Arc::as_ptr(s) as usize == *key));
                    let mut mixed = [0.0f32; 960];
                    for source in &sources {
                        source.mix_audio(cursors.entry(Arc::as_ptr(source) as usize).or_default(), &mut mixed);
                    }
                    let mut bytes = Vec::with_capacity(3840);
                    for sample in mixed {
                        bytes.extend(sample.clamp(-1.0, 1.0).to_le_bytes());
                    }
                    if stream.write_all(&bytes).is_err() {
                        break;
                    }
                    blocks += 1;
                    std::thread::sleep((start + Duration::from_millis(blocks * 10)).saturating_duration_since(Instant::now()));
                }
            }
        })?;
        let handle = std::thread::Builder::new().name(format!("gfx-{channel}")).spawn(move || {
            let cfg = lock(&shared).cfg.clone();
            let mut comp = Compositor::new(width, height, cfg, &root);
            let mut version = 0;
            for conn in listener.incoming() {
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = conn else { continue };
                tracing::debug!(channel = %channel, "graphics consumer connected");
                let connected = AtomicBool::new(true);
                std::thread::scope(|scope| {
                    let renderer = std::thread::Builder::new().name(format!("gfx-render-{channel}")).spawn_scoped(scope, || {
                        render_loop(&shared, &mut comp, &mut version, &latest, fps, &stop2, &connected, &channel);
                    });
                    if let Err(e) = renderer {
                        tracing::error!(channel = %channel, "could not start graphics renderer: {e}");
                        return;
                    }
                    serve(&latest, stream, fps, &stop2, &consumed2, &channel);
                    connected.store(false, Ordering::Relaxed);
                });
                let mut frame = latest.lock().unwrap_or_else(|e| e.into_inner());
                frame.rgba = vec![0; width as usize * height as usize * 4].into();
                frame.sources.clear();
            }
        })?;
        Ok(GfxThread { port, audio_port, consumed, stop, handle: Some(handle) })
    }

    /// The thread ended unexpectedly (it must be respawned).
    pub fn is_dead(&self) -> bool {
        !self.stop.load(Ordering::Relaxed) && self.handle.as_ref().is_none_or(|h| h.is_finished())
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

impl Drop for GfxThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Only completed frames cross this lock; it is never held during rendering,
/// browser IO, mixing or socket writes.
struct LiveFrame {
    rgba: Arc<[u8]>,
    sources: Vec<Arc<lb_gfx::web::WebSource>>,
}

#[allow(clippy::too_many_arguments)]
fn render_loop(
    shared: &SharedGfx,
    comp: &mut Compositor,
    version: &mut u64,
    latest: &Mutex<LiveFrame>,
    fps: f64,
    stop: &AtomicBool,
    connected: &AtomicBool,
    channel: &str,
) {
    let period = Duration::from_secs_f64(1.0 / fps.max(1.0));
    let mut failing = false;
    let mut slow_since = Instant::now();
    while !stop.load(Ordering::Relaxed) && connected.load(Ordering::Relaxed) {
        let started = Instant::now();
        let at = lb_core::add_ms(now(), LEAD_MS);
        let rendered = catch_unwind(AssertUnwindSafe(|| render_frame(shared, comp, version, at)));
        let sources = match rendered {
            Ok(()) => {
                failing = false;
                comp.audio_sources()
            }
            Err(e) => {
                if !failing {
                    let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()));
                    tracing::error!(channel = %channel, "graphics frame failed: {}", msg.unwrap_or_default());
                }
                failing = true;
                comp.clear();
                Vec::new()
            }
        };
        let rgba: Arc<[u8]> = comp.last_frame().into();
        *latest.lock().unwrap_or_else(|e| e.into_inner()) = LiveFrame { rgba, sources };
        let elapsed = started.elapsed();
        if elapsed > period && slow_since.elapsed() >= Duration::from_secs(5) {
            tracing::warn!(channel = %channel, render_ms = elapsed.as_millis() as u64,
                budget_ms = period.as_millis() as u64, "graphics rendering is slow; repeating latest overlay without blocking playout");
            slow_since = Instant::now();
        }
        // Drop missed render deadlines instead of queuing obsolete work.
        std::thread::sleep(period.saturating_sub(started.elapsed()));
    }
}

fn serve(latest: &Mutex<LiveFrame>, mut stream: TcpStream, fps: f64, stop: &AtomicBool, consumed: &AtomicU64, channel: &str) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1)));
    let fps = fps.max(1.0);
    let max_ahead = (fps * MAX_AHEAD_S).ceil() as u64;
    let start = Instant::now();
    let mut sent = 0u64;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if sent > consumed.load(Ordering::Relaxed).saturating_add(max_ahead) {
            if peer_closed(&stream) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        // Rawvideo timestamps are frame indices. Never reset this clock after
        // a delay: that leaves the overlay permanently behind the main input.
        let due = start + Duration::from_secs_f64(sent as f64 / fps);
        let wait = due.saturating_duration_since(Instant::now() + Duration::from_millis(LEAD_MS as u64));
        if !wait.is_zero() {
            std::thread::sleep(wait.min(Duration::from_millis(10)));
            continue;
        }
        let frame = latest.lock().unwrap_or_else(|e| e.into_inner()).rgba.clone();
        if let Err(e) = stream.write_all(&frame) {
            tracing::warn!(channel = %channel, "graphics consumer gone: {e}");
            return;
        }
        sent += 1;
    }
}

/// A dead encoder cannot advance progress. Detect EOF even while the
/// progress limit prevents further writes, so its replacement can connect.
fn peer_closed(stream: &TcpStream) -> bool {
    match stream.peek(&mut [0u8; 1]) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => !matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted),
    }
}

/// Evaluate the active set and render one frame at `at`.
fn render_frame(shared: &SharedGfx, comp: &mut Compositor, version: &mut u64, at: WallTime) {
    let (active, vars, remaining, next_ms, program) = {
        let mut st = lock(shared);
        if st.cfg_version != *version {
            comp.set_config(st.cfg.clone());
            *version = st.cfg_version;
        }
        let active = evaluate(&st, at);
        st.active = active.clone();
        let remaining = st.on_air.as_ref().map(|(it, s)| (it.duration_ms - diff_ms(at, *s)).max(0)).unwrap_or(0);
        let next_ms = st.next_program_start.map(|t| diff_ms(t, at));
        (active, st.vars.clone(), remaining, next_ms, st.program.clone())
    };
    comp.render_live(&FrameContext {
        now: at,
        active: &active,
        vars: &vars,
        item_remaining_ms: remaining,
        next_program_ms: next_ms,
        program: program.as_ref(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    struct StopOnDrop<'a>(&'a AtomicBool);
    impl Drop for StopOnDrop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    #[tokio::test]
    #[ignore = "requires FFmpeg with libx264"]
    async fn ffmpeg_keeps_encoding_when_graphics_rendering_blocks() {
        use lb_core::config::{AudioFormat, EncoderConfig, VideoFormat};
        use lb_media::args::encoder::{EncodeJob, GfxInput, encoder_args};
        use tokio::io::{AsyncBufReadExt, BufReader};

        let shared = Arc::new(Mutex::new(GfxState::default()));
        let video = VideoFormat::default();
        let audio = AudioFormat::default();
        let encoder = EncoderConfig::default();
        let mut gfx =
            GfxThread::spawn(shared.clone(), video.width, video.height, video.fps, PathBuf::from("."), "test-ffmpeg".into()).unwrap();
        let mut args = encoder_args(&EncodeJob {
            channel: "test",
            video: &video,
            audio: &audio,
            encoder: &encoder,
            video_codec: "libx264",
            outputs: &[],
            record_feed: None,
            preview: None,
            gfx: Some(GfxInput { port: gfx.port, audio_port: gfx.audio_port, fps: video.fps }),
            loglevel: "error",
        });
        // Exercise the production overlay/audio graph and codec settings with
        // paced synthetic main A/V instead of the MPEG-TS stdin decoder.
        let begin = args.iter().position(|a| a == "-fflags").unwrap();
        let end = args.iter().position(|a| a == "pipe:0").unwrap() + 1;
        args.splice(
            begin..end,
            ["-re", "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=25[out0];sine=frequency=440:sample_rate=48000[out1]"]
                .map(str::to_string),
        );
        let output = args.iter().position(|a| a == "-flags").unwrap();
        args.truncate(output);
        args.extend(["-t", "6", "-f", "null", "-"].map(str::to_string));
        let mut child = lb_media::process::command("ffmpeg")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let consumed = gfx.consumed.clone();
        let (ready, started) = tokio::sync::oneshot::channel();
        let monitor = tokio::spawn(async move {
            let mut ready = Some(ready);
            let mut lines = BufReader::new(stderr).lines();
            let mut parser = lb_media::progress::ProgressParser::default();
            let mut frames = 0;
            let mut updates = Vec::new();
            let mut errors = Vec::new();
            while let Some(line) = lines.next_line().await.unwrap() {
                if let Some(stats) = parser.feed(&line) {
                    frames = stats.frame;
                    consumed.store(frames, Ordering::Relaxed);
                    if frames > 5 {
                        updates.push(Instant::now());
                        if let Some(ready) = ready.take() {
                            let _ = ready.send(());
                        }
                    }
                } else if !lb_media::progress::ProgressParser::is_progress_line(&line) {
                    errors.push(line);
                }
            }
            (frames, updates, errors)
        });
        tokio::time::timeout(Duration::from_secs(10), started).await.unwrap().unwrap();
        let blocked = tokio::task::spawn_blocking(move || {
            let _guard = shared.lock().unwrap();
            std::thread::sleep(Duration::from_secs(3));
        });
        let status = tokio::time::timeout(Duration::from_secs(20), child.wait()).await.unwrap().unwrap();
        blocked.await.unwrap();
        let (frames, updates, errors) = monitor.await.unwrap();
        gfx.shutdown();
        gfx.handle.take().unwrap().join().unwrap();
        assert!(status.success(), "{errors:?}");
        assert!(frames >= 145, "only {frames} frames in six seconds");
        assert!(
            updates.windows(2).all(|pair| pair[1].duration_since(pair[0]) < Duration::from_secs(2)),
            "encoder progress stalled while rendering was blocked"
        );
    }

    #[test]
    fn blocked_renderer_does_not_starve_video_or_audio_transport() {
        let shared = Arc::new(Mutex::new(GfxState::default()));
        let mut gfx = GfxThread::spawn(shared.clone(), 4, 4, 25.0, PathBuf::from("."), "test-blocked".into()).unwrap();
        let mut video = TcpStream::connect(("127.0.0.1", gfx.port)).unwrap();
        let mut audio = TcpStream::connect(("127.0.0.1", gfx.audio_port)).unwrap();
        video.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        audio.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        video.read_exact(&mut [0u8; 64]).unwrap();
        // Simulate a render that cannot complete for over one second. The
        // old shared render/write loop could not deliver either stream here.
        let blocked = shared.lock().unwrap();
        let start = Instant::now();
        for i in 1..36 {
            video.read_exact(&mut [0u8; 64]).unwrap();
            gfx.consumed.store(i + 1, Ordering::Relaxed);
            audio.read_exact(&mut [0u8; 3840]).unwrap();
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        drop(blocked);
        video.read_exact(&mut [0u8; 64]).unwrap();
        gfx.shutdown();
        gfx.handle.take().unwrap().join().unwrap();
    }

    #[test]
    fn video_keeps_its_cadence_without_new_renders_and_picks_up_latest_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut reader = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        reader.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let latest = Arc::new(Mutex::new(LiveFrame { rgba: vec![1; 16].into(), sources: Vec::new() }));
        let stop = Arc::new(AtomicBool::new(false));
        let consumed = Arc::new(AtomicU64::new(0));
        std::thread::scope(|scope| {
            let _halt = StopOnDrop(&stop);
            scope.spawn(|| serve(&latest, stream, 25.0, &stop, &consumed, "test"));
            let start = Instant::now();
            // No renderer publishes for more than a second: transport must
            // still supply 25 fps, repeating the last completed frame.
            for i in 0..40 {
                let mut frame = [0u8; 16];
                reader.read_exact(&mut frame).unwrap();
                assert_eq!(frame, [1; 16]);
                consumed.store(i + 1, Ordering::Relaxed);
            }
            assert!(start.elapsed() < Duration::from_secs(3));
            latest.lock().unwrap().rgba = vec![2; 16].into();
            let mut updated = false;
            for i in 40..55 {
                let mut frame = [0u8; 16];
                reader.read_exact(&mut frame).unwrap();
                consumed.store(i + 1, Ordering::Relaxed);
                if frame == [2; 16] {
                    updated = true;
                    break;
                }
            }
            stop.store(true, Ordering::Relaxed);
            assert!(updated, "a new render must replace the repeated frame without restarting the encoder");
        });
    }

    #[test]
    fn stalled_consumer_has_bounded_backlog_and_recovers_without_resetting_timestamps() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut reader = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        reader.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let latest = Mutex::new(LiveFrame { rgba: vec![7; 4].into(), sources: Vec::new() });
        let stop = AtomicBool::new(false);
        let consumed = AtomicU64::new(0);
        std::thread::scope(|scope| {
            let _halt = StopOnDrop(&stop);
            scope.spawn(|| serve(&latest, stream, 10.0, &stop, &consumed, "test"));
            for _ in 0..26 {
                reader.read_exact(&mut [0u8; 4]).unwrap();
            }
            reader.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            let err = reader.read_exact(&mut [0u8; 4]).unwrap_err();
            assert!(matches!(err.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut));
            consumed.store(26, Ordering::Relaxed);
            reader.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut frame = [0u8; 4];
            reader.read_exact(&mut frame).unwrap();
            stop.store(true, Ordering::Relaxed);
            assert_eq!(frame, [7; 4]);
        });
    }

    #[test]
    fn disconnected_encoder_is_detected_while_waiting_for_progress() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut reader = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        reader.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let latest = Mutex::new(LiveFrame { rgba: vec![0; 4].into(), sources: Vec::new() });
        let stop = AtomicBool::new(false);
        let consumed = AtomicU64::new(0);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let _halt = StopOnDrop(&stop);
            scope.spawn(|| {
                serve(&latest, stream, 10.0, &stop, &consumed, "test");
                tx.send(()).unwrap();
            });
            for _ in 0..26 {
                reader.read_exact(&mut [0u8; 4]).unwrap();
            }
            drop(reader);
            rx.recv_timeout(Duration::from_secs(1)).expect("closed encoder must not leave the listener stuck behind its progress limit");
        });
    }
}
