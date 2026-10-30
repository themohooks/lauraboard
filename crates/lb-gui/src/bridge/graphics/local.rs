//! Drafts drawn on this computer. When the server runs here (the project
//! folder is known), every change of the draft is drawn at once by the same
//! compositor the encoder uses, on a background thread — no request, PNG
//! and decoding per edit. The air's variables and clocks come from
//! `GET /graphics/live` every few seconds. Drafts with HTML / SWF need the
//! server's browser and are still rendered there.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, SharedString};

use lb_core::Ms;
use lb_core::graphics::{GraphicsConfig, ProgramClock};
use lb_gfx::{Compositor, FrameContext};

use super::super::{Ctx, with};
use crate::net::Msg;
use crate::shell::Gfx;

/// How old the air's variables may get before they are asked for again.
const LIVE_TTL: Duration = Duration::from_secs(3);

/// `GET /graphics/live`.
#[derive(Debug, Clone, Deserialize)]
struct Live {
    width: u32,
    height: u32,
    vars: HashMap<String, String>,
    remaining_ms: Ms,
    next_program_ms: Option<Ms>,
    program: ProgramClock,
}

struct Job {
    cfg: GraphicsConfig,
    live: Live,
    serial: u64,
}

#[derive(Default)]
pub struct LocalRender {
    /// Render thread (started on first use) and the project it reads.
    root: Option<PathBuf>,
    tx: Option<mpsc::Sender<Job>>,
    live: Option<(Live, Instant)>,
    fetching: bool,
    /// The server has no `/graphics/live` (older version): render there.
    unsupported: bool,
    /// Last draft sent to the thread and last frame shown.
    serial: u64,
    shown: u64,
}

impl LocalRender {
    pub fn new(project: Option<PathBuf>) -> Self {
        LocalRender { root: project, ..Default::default() }
    }
}

/// Draw `cfg` here if possible; `false`: the caller renders on the server.
pub fn render(c: &mut Ctx, cfg: &GraphicsConfig) -> bool {
    let l = &mut c.gfx.local;
    if l.root.is_none() || l.unsupported || lb_gfx::preview::has_web(cfg) {
        return false;
    }
    let stale = l.live.as_ref().is_none_or(|(_, at)| at.elapsed() > LIVE_TTL);
    if stale && !l.fetching {
        fetch_live(c);
    }
    let l = &mut c.gfx.local;
    let Some((live, _)) = l.live.clone() else {
        // first frame: from the server while the variables load
        return false;
    };
    let tx = l.tx.get_or_insert_with(|| spawn(l.root.clone().unwrap_or_default()));
    l.serial += 1;
    let job = Job { cfg: cfg.clone(), live, serial: l.serial };
    if tx.send(job).is_err() {
        l.tx = None;
        return false;
    }
    // a server render still on its way must not overwrite this frame
    c.gfx.render_ticket = 0;
    c.gfx.rendering = false;
    c.gfx.render_again = false;
    true
}

/// Drop the render thread with its image and font caches.
pub fn reset(c: &mut Ctx) {
    c.gfx.local.tx = None;
}

fn fetch_live(c: &mut Ctx) {
    c.gfx.local.fetching = true;
    let ch = c.model.ch();
    c.worker.spawn(move |api| {
        let r = api.get::<Live>(&format!("/channels/{ch}/graphics/live"));
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            let l = &mut c.gfx.local;
            l.fetching = false;
            let first = l.live.is_none();
            match r {
                Ok(live) if live.width > 0 && live.height > 0 && live.width <= 4096 && live.height <= 4096 => {
                    l.live = Some((live, Instant::now()));
                    if first && c.section() == "graphics" {
                        super::render(c);
                    }
                }
                Ok(_) => l.unsupported = true,
                Err(e) if e.contains("404") => l.unsupported = true,
                // keep the last variables; ask again on the next edit
                Err(_) => {}
            }
        })))
    });
}

fn spawn(root: PathBuf) -> mpsc::Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("gfx-preview".into())
        .spawn(move || {
            let mut comp: Option<Compositor> = None;
            while let Ok(mut job) = rx.recv() {
                // only the newest draft matters
                while let Ok(newer) = rx.try_recv() {
                    job = newer;
                }
                let (w, h) = (job.live.width, job.live.height);
                let comp = match &mut comp {
                    Some(c) if c.width() == w && c.height() == h => c,
                    slot => slot.insert(Compositor::new(w, h, job.cfg.clone(), &root)),
                };
                let rgba = draw(comp, &job);
                let serial = job.serial;
                let _ = slint::invoke_from_event_loop(move || with(move |c| show(c, serial, w, h, rgba)));
            }
        })
        .expect("preview thread");
    tx
}

fn draw(comp: &mut Compositor, job: &Job) -> Vec<u8> {
    comp.set_config(job.cfg.clone());
    comp.set_editing(true);
    let active = lb_gfx::preview::active_set(&job.cfg, None, None);
    let mut vars = job.live.vars.clone();
    vars.extend(job.cfg.vars.clone());
    let ctx = FrameContext {
        now: chrono::Local::now().naive_local(),
        active: &active,
        vars: &vars,
        item_remaining_ms: job.live.remaining_ms,
        next_program_ms: job.live.next_program_ms,
        program: Some(&job.live.program),
    };
    comp.render(&ctx).to_vec()
}

fn show(c: &mut Ctx, serial: u64, w: u32, h: u32, rgba: Vec<u8>) {
    // an older frame, or a server render started since (HTML / SWF added);
    // frames of a continuous edit are shown, not only the very last one
    if serial <= c.gfx.local.shown || c.gfx.rendering || c.section() != "graphics" {
        return;
    }
    c.gfx.local.shown = serial;
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Gfx>();
    if g.get_bg_mode() != 0 || rgba.len() != (w * h * 4) as usize {
        return;
    }
    g.set_background(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h)));
    g.set_has_background(true);
    g.set_rendering(false);
    g.set_bg_note(SharedString::new());
    c.gfx.pending_web = false;
    super::invalidate_layers(c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::graphics::{Anchor, Animation, Element, ElementKind, Placement};

    #[test]
    fn draft_and_its_variables_are_drawn_here() {
        let mut cfg = GraphicsConfig::default();
        cfg.elements.push(Element {
            id: "box".into(),
            name: "box".into(),
            kind: ElementKind::Rect { color: "#FF0000".into(), width: 10.0, height: 10.0, radius: 0.0, shape: Default::default() },
            placement: Placement { anchor: Anchor::TopLeft, x: 0.0, y: 0.0, width: None, height: None },
            opacity: 1.0,
            layer: 0,
            animation: Animation::default(),
            profiles: vec![],
        });
        let live = Live {
            width: 64,
            height: 36,
            vars: HashMap::new(),
            remaining_ms: 1000,
            next_program_ms: None,
            program: ProgramClock::default(),
        };
        let mut comp = Compositor::new(64, 36, cfg.clone(), &std::env::temp_dir());
        let rgba = draw(&mut comp, &Job { cfg, live, serial: 1 });
        assert_eq!(rgba.len(), 64 * 36 * 4);
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
        // outside the box: transparent
        let far = ((30 * 64 + 60) * 4) as usize;
        assert_eq!(rgba[far + 3], 0);
    }
}
