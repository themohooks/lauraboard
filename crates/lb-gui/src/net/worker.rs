//! Background request execution. Each job runs on its own short-lived
//! thread and posts a [`Msg`] back; the UI is woken through [`Wake`].

use std::sync::mpsc::{Receiver, Sender, channel};

use super::{Msg, Wake};
use super::api::Api;

#[derive(Clone)]
pub struct Worker {
    api: Api,
    tx: Sender<Msg>,
    wake: Wake,
}

impl Worker {
    pub fn new(api: Api, wake: Wake) -> (Self, Receiver<Msg>) {
        let (tx, rx) = channel();
        (Worker { api, tx, wake }, rx)
    }

    pub fn api(&self) -> &Api {
        &self.api
    }

    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    /// Run `job` in the background; errors become [`Msg::Error`].
    pub fn spawn(&self, job: impl FnOnce(&Api) -> Result<Msg, String> + Send + 'static) {
        let (api, tx, wake) = (self.api.clone(), self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let msg = job(&api).unwrap_or_else(Msg::Error);
            let _ = tx.send(msg);
            wake();
        });
    }
}
