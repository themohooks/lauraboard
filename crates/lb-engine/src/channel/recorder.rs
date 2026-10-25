//! Recorder supervision. The recorder is its own ffmpeg, fed by the
//! encoder over a local port that stays the same for the life of the
//! channel: recording settings (even switching recording on and off) only
//! restart the recorder, never the encoder and the broadcast.

use std::net::TcpListener;
use std::time::{Duration, Instant};

use lb_media::process::Recorder;

/// A recorder that quit sooner than this after starting is restarted with
/// a growing delay.
const SHORT_RUN: Duration = Duration::from_secs(10);
const BACKOFF: &[u64] = &[0, 1, 2, 5, 10, 30];
/// Time to close the current file on a settings change or stop.
const GRACE: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct RecorderSup {
    port: Option<u16>,
    proc: Option<Recorder>,
    ffmpeg: String,
    /// Command line of the recorder that runs (or should).
    args: Vec<String>,
    started: Option<Instant>,
    retry_at: Option<Instant>,
    failures: usize,
    last_error: Option<String>,
}

impl RecorderSup {
    /// Local port of the feed: picked once, kept across restarts (the
    /// encoder sends there).
    pub fn port(&mut self) -> u16 {
        *self.port.get_or_insert_with(|| TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).unwrap_or(41_877))
    }

    /// Run with `args`: restarted only when they changed (or it is down).
    pub async fn apply(&mut self, ffmpeg: &str, args: Vec<String>) {
        if self.proc.is_some() && self.args == args && self.ffmpeg == ffmpeg {
            return;
        }
        self.stop().await;
        self.ffmpeg = ffmpeg.to_string();
        self.args = args;
        self.failures = 0;
        self.last_error = None;
        self.spawn();
    }

    fn spawn(&mut self) {
        tracing::info!("starting recorder: {} {}", self.ffmpeg, self.args.join(" "));
        match Recorder::spawn(&self.ffmpeg, &self.args) {
            Ok(r) => {
                self.proc = Some(r);
                self.started = Some(Instant::now());
                self.retry_at = None;
            }
            Err(e) => self.failed(e.to_string()),
        }
    }

    fn failed(&mut self, err: String) {
        self.last_error = Some(err);
        let delay = BACKOFF[self.failures.min(BACKOFF.len() - 1)];
        self.failures += 1;
        self.retry_at = Some(Instant::now() + Duration::from_secs(delay));
    }

    /// Restart a recorder that quit. It quits by itself when the encoder
    /// goes away (end of the feed) and listens again for the next one.
    pub fn check(&mut self) {
        if let Some(r) = self.proc.as_mut() {
            let Some(ok) = r.try_exited() else { return };
            let err = r.error_text();
            self.proc = None;
            let short = self.started.is_none_or(|t| t.elapsed() < SHORT_RUN);
            match (ok && !short, err) {
                (true, _) => self.failures = 0,
                (false, e) => self.failed(e.unwrap_or_else(|| "recorder exited".into())),
            }
        }
        if self.proc.is_none() && !self.args.is_empty() && self.retry_at.is_none_or(|t| Instant::now() >= t) {
            self.spawn();
        }
    }

    pub async fn stop(&mut self) {
        if let Some(mut r) = self.proc.take() {
            r.shutdown(GRACE).await;
        }
        self.args.clear();
        self.retry_at = None;
    }

    /// The recorder runs; it does not keep failing.
    pub fn healthy(&self) -> bool {
        self.proc.is_some() && self.started.is_some_and(|t| t.elapsed() >= SHORT_RUN)
    }

    /// Why recording does not work right now.
    pub fn error(&self) -> Option<String> {
        if self.healthy() { None } else { self.last_error.clone() }
    }
}
