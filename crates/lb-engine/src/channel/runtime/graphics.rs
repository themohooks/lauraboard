//! Lifetime of the graphics overlay thread: started with the broadcast when
//! the channel has graphics, started / stopped when graphics are added or
//! switched off, respawned if it ever dies.

use super::super::gfx::GfxThread;
use super::Runtime;

impl Runtime {
    /// Graphics are switched on and there is something to draw.
    fn graphics_wanted(&self) -> bool {
        let d = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
        d.config.graphics.enabled && !d.graphics.elements.is_empty()
    }

    /// Bring the overlay thread in line with the configuration. Returns
    /// `true` when the overlay input changed (the encoder must be restarted
    /// to connect to it).
    pub(super) fn ensure_graphics(&mut self) -> bool {
        let wanted = self.graphics_wanted();
        match (&self.gfx_thread, wanted) {
            (Some(t), true) if !t.is_dead() => false,
            (Some(_), true) => {
                tracing::warn!(channel = %self.shared.id, "graphics thread died, restarting it");
                self.gfx_thread = None;
                self.spawn_graphics();
                true
            }
            (Some(_), false) => {
                if let Some(t) = self.gfx_thread.take() {
                    t.shutdown();
                }
                true
            }
            (None, true) => {
                self.spawn_graphics();
                self.gfx_thread.is_some()
            }
            (None, false) => false,
        }
    }

    fn spawn_graphics(&mut self) {
        let (w, h, fps) = {
            let d = self.shared.data.read().unwrap_or_else(|e| e.into_inner());
            (d.config.video.width, d.config.video.height, d.config.video.fps)
        };
        match GfxThread::spawn(self.gfx.clone(), w, h, fps, self.shared.root.clone(), self.shared.id.clone()) {
            Ok(t) => self.gfx_thread = Some(t),
            Err(e) => tracing::warn!("graphics disabled: {e}"),
        }
    }
}
