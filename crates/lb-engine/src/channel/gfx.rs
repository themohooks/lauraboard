//! Graphics thread: renders overlay frames and streams them to the encoder
//! over a local TCP connection.
//!
//! The thread paces itself: frames go out at the wall-clock rate, a little
//! ahead, and never further ahead of the encoder's progress than a couple of
//! seconds. Socket back-pressure cannot be relied on: Windows loopback
//! accepts writes without limit and aborts the connection once gigabytes
//! pile up (the overlay then vanished after ~20 s).
//!
//! Frames are timed by their index (`start + n / fps`), not by the moment
//! they happen to be rendered, so motion (tickers, slides) advances by the
//! same amount every frame. A panic while rendering one frame is caught:
//! the thread keeps serving (a dead thread would leave the encoder unable to
//! connect and restarting forever).

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

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

/// A frame clock drifting further than this from the wall clock is resynced
/// (encoder stalled or restarted).
const MAX_DRIFT_MS: i64 = 750;
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
        let audio = Arc::new(Mutex::new(std::collections::VecDeque::<f32>::new()));
        let audio_out = audio.clone();
        let audio_stop = stop.clone();
        std::thread::Builder::new().name(format!("gfx-audio-{channel}")).spawn(move || {
            while !audio_stop.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = audio_listener.accept() else {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                };
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                audio_out.lock().unwrap_or_else(|e| e.into_inner()).clear();
                let start = std::time::Instant::now();
                let mut blocks = 0;
                while !audio_stop.load(Ordering::Relaxed) {
                    let mut bytes = Vec::with_capacity(3840);
                    {
                        let mut queue = audio_out.lock().unwrap_or_else(|e| e.into_inner());
                        for _ in 0..960 {
                            bytes.extend(queue.pop_front().unwrap_or(0.0).clamp(-1.0, 1.0).to_le_bytes());
                        }
                    }
                    if stream.write_all(&bytes).is_err() {
                        break;
                    }
                    blocks += 1;
                    std::thread::sleep((start + Duration::from_millis(blocks * 10)).saturating_duration_since(std::time::Instant::now()));
                }
            }
        })?;
        let consumed = Arc::new(AtomicU64::new(0));
        let consumed2 = consumed.clone();
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
                serve(&shared, &mut comp, &mut version, stream, fps, &stop2, &consumed2, &channel, &audio);
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

#[allow(clippy::too_many_arguments)]
fn serve(
    shared: &SharedGfx,
    comp: &mut Compositor,
    version: &mut u64,
    mut stream: TcpStream,
    fps: f64,
    stop: &AtomicBool,
    consumed: &AtomicU64,
    channel: &str,
    audio: &Mutex<std::collections::VecDeque<f32>>,
) {
    let _ = stream.set_nodelay(true);
    let frame_ms = 1000.0 / fps.max(1.0);
    let max_ahead = (fps * MAX_AHEAD_S).ceil() as u64;
    let mut start = now();
    let mut n: u64 = 0;
    // frames written on this connection (`n` restarts on a resync)
    let mut sent: u64 = 0;
    let mut failing = false;
    loop {
        // the encoder is behind (slower than real time, or stalled): wait
        // rather than queue frames in the socket
        while sent > consumed.load(Ordering::Relaxed) + max_ahead {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let mut at = lb_core::add_ms(start, (n as f64 * frame_ms).round() as i64);
        let ahead = diff_ms(at, now());
        if ahead > LEAD_MS {
            std::thread::sleep(Duration::from_millis((ahead - LEAD_MS) as u64));
        } else if ahead < -MAX_DRIFT_MS {
            start = now();
            n = 0;
            at = start;
        }
        n += 1;
        sent += 1;
        let rendered = catch_unwind(AssertUnwindSafe(|| render_frame(shared, comp, version, at)));
        let frame: &[u8] = match rendered {
            Ok(()) => {
                failing = false;
                comp.last_frame()
            }
            Err(e) => {
                if !failing {
                    let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()));
                    tracing::error!(channel = %channel, "graphics frame failed: {}", msg.unwrap_or_default());
                }
                failing = true;
                comp.clear()
            }
        };
        if let Err(e) = stream.write_all(frame) {
            tracing::warn!(channel = %channel, "graphics consumer gone: {e}");
            return;
        }
        let samples = (((sent as f64 * 48000.0 / fps).round() - ((sent - 1) as f64 * 48000.0 / fps).round()) as usize) * 2;
        let mut mixed = vec![0.0; samples];
        if !failing {
            comp.mix_audio(&mut mixed);
        }
        let mut queue = audio.lock().unwrap_or_else(|e| e.into_inner());
        queue.extend(mixed);
        while queue.len() > 48000 {
            queue.pop_front();
        }
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
