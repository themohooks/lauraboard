//! Graphics thread: renders overlay frames and streams them to the encoder
//! over a local TCP connection. Writing blocks when ffmpeg has enough
//! frames, which paces rendering to the encoder's consumption.
//!
//! Frames are timed by their index (`start + n / fps`), not by the moment
//! they happen to be rendered, so motion (tickers, slides) advances by the
//! same amount every frame. A panic while rendering one frame is caught:
//! the thread keeps serving (a dead thread would leave the encoder unable to
//! connect and restarting forever).

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use lb_core::graphics::{self, ActiveGraphic, GfxContext, GraphicsConfig, Manual};
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
    pub vars: HashMap<String, String>,
    pub manual: HashMap<String, Manual>,
    /// Last evaluated active set (for status).
    pub active: Vec<ActiveGraphic>,
}

pub type SharedGfx = Arc<Mutex<GfxState>>;

pub struct GfxThread {
    pub port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// A frame clock drifting further than this from the wall clock is resynced
/// (encoder stalled or restarted).
const MAX_DRIFT_MS: i64 = 750;

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
    let ctx =
        GfxContext { now: at, item, elapsed_ms: elapsed, remaining_ms: remaining, profile: st.profile.as_deref(), manual: &st.manual };
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
                serve(&shared, &mut comp, &mut version, stream, fps, &stop2, &channel);
            }
        })?;
        Ok(GfxThread { port, stop, handle: Some(handle) })
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
    channel: &str,
) {
    let _ = stream.set_nodelay(true);
    let frame_ms = 1000.0 / fps.max(1.0);
    let mut start = now();
    let mut n: u64 = 0;
    let mut failing = false;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let wall = now();
        let mut at = lb_core::add_ms(start, (n as f64 * frame_ms).round() as i64);
        if diff_ms(at, wall).abs() > MAX_DRIFT_MS {
            start = wall;
            n = 0;
            at = wall;
        }
        n += 1;
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
        if stream.write_all(frame).is_err() {
            return;
        }
    }
}

/// Evaluate the active set and render one frame at `at`.
fn render_frame(shared: &SharedGfx, comp: &mut Compositor, version: &mut u64, at: WallTime) {
    let (active, vars, remaining, next_ms) = {
        let mut st = lock(shared);
        if st.cfg_version != *version {
            comp.set_config(st.cfg.clone());
            *version = st.cfg_version;
        }
        let active = evaluate(&st, at);
        st.active = active.clone();
        let remaining = st.on_air.as_ref().map(|(it, s)| (it.duration_ms - diff_ms(at, *s)).max(0)).unwrap_or(0);
        let next_ms = st.next_program_start.map(|t| diff_ms(t, at));
        (active, st.vars.clone(), remaining, next_ms)
    };
    comp.render(&FrameContext { now: at, active: &active, vars: &vars, item_remaining_ms: remaining, next_program_ms: next_ms });
}
