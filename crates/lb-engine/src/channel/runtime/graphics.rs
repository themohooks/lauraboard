//! Lifetime of the graphics overlay thread: started with the broadcast when
//! graphics are switched on, respawned if it ever dies.
//!
//! Attaching or detaching the overlay restarts the encoder, and that drops
//! the connection to every network output (the remote server sees the
//! stream end). So the overlay stays attached while the channel runs: it
//! is attached even with no elements yet (new graphics, a scene's own
//! graphics just go on air), and switching graphics off only empties it.

use super::super::gfx::GfxThread;
use super::Runtime;

impl Runtime {
    /// Graphics are switched on.
    fn graphics_wanted(&self) -> bool {
        self.shared.data.read().unwrap_or_else(|e| e.into_inner()).config.graphics.enabled
    }

    /// Graphics are switched off: the overlay draws nothing.
    pub(super) fn graphics_muted(&self) -> bool {
        !self.graphics_wanted()
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
            // keep the overlay input (it draws nothing now): no restart
            (Some(t), false) if !t.is_dead() => false,
            (Some(_), false) => {
                self.gfx_thread = None;
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
