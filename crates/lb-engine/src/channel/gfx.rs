//! Graphics thread: renders overlay frames and streams them to the encoder
//! over a local TCP connection. Writing blocks when ffmpeg has enough
//! frames, which paces rendering to the encoder's consumption.

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
    pub fn spawn(shared: SharedGfx, width: u32, height: u32, root: PathBuf, channel: String) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        std::thread::Builder::new().name(format!("gfx-{channel}")).spawn(move || {
            let cfg = shared.lock().map(|s| s.cfg.clone()).unwrap_or_default();
            let mut comp = Compositor::new(width, height, cfg, &root);
            let mut version = 0;
            for conn in listener.incoming() {
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = conn else { continue };
                tracing::debug!(channel = %channel, "graphics consumer connected");
                serve(&shared, &mut comp, &mut version, stream, &stop2);
            }
        })?;
        Ok(GfxThread { port, stop })
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

fn serve(shared: &SharedGfx, comp: &mut Compositor, version: &mut u64, mut stream: TcpStream, stop: &AtomicBool) {
    let _ = stream.set_nodelay(true);
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let at = now();
        let (active, vars, remaining, next_ms) = {
            let Ok(mut st) = shared.lock() else { return };
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
        let frame =
            comp.render(&FrameContext { now: at, active: &active, vars: &vars, item_remaining_ms: remaining, next_program_ms: next_ms });
        if stream.write_all(frame).is_err() {
            return;
        }
    }
}
