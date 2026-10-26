//! Background request execution. Each job runs on its own short-lived
//! thread and posts a [`Msg`] back; the UI is woken through [`Wake`].

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use super::api::Api;
use super::{Msg, Wake};

#[derive(Clone)]
pub struct Worker {
    api: Api,
    tx: Sender<Msg>,
    wake: Wake,
    epoch: Arc<AtomicU64>,
}

impl Worker {
    pub fn new(api: Api, wake: Wake) -> (Self, Receiver<Msg>) {
        let (tx, rx) = channel();
        (Worker { api, tx, wake, epoch: Arc::new(AtomicU64::new(0)) }, rx)
    }

    pub fn api(&self) -> &Api {
        &self.api
    }

    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }

    pub fn invalidate(&self) {
        self.epoch.fetch_add(1, Ordering::Relaxed);
    }

    /// Run `job` in the background; errors become [`Msg::Error`].
    pub fn spawn(&self, job: impl FnOnce(&Api) -> Result<Msg, String> + Send + 'static) {
        let (api, tx, wake) = (self.api.clone(), self.tx.clone(), self.wake.clone());
        let epoch = self.epoch();
        std::thread::spawn(move || {
            let msg = job(&api).unwrap_or_else(Msg::Error);
            let _ = tx.send(Msg::Scoped(epoch, Box::new(msg)));
            wake();
        });
    }
}
