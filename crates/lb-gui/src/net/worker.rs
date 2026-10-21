//! Background request execution. Each job runs on its own short-lived
//! thread and posts a [`Msg`] back; the UI is woken with a repaint.

use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::Context;

use super::Msg;
use super::api::Api;

#[derive(Clone)]
pub struct Worker {
    api: Api,
    tx: Sender<Msg>,
    ctx: Context,
}

impl Worker {
    pub fn new(api: Api, ctx: Context) -> (Self, Receiver<Msg>) {
        let (tx, rx) = channel();
        (Worker { api, tx, ctx }, rx)
    }

    pub fn api(&self) -> &Api {
        &self.api
    }

    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    /// Run `job` in the background; errors become [`Msg::Error`].
    pub fn spawn(&self, job: impl FnOnce(&Api) -> Result<Msg, String> + Send + 'static) {
        let (api, tx, ctx) = (self.api.clone(), self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let msg = job(&api).unwrap_or_else(Msg::Error);
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }
}
