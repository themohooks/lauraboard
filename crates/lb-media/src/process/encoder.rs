//! Long-running encoder process: stdin input, stats via `-progress`,
//! MJPEG preview frames on stdout.

use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStdin};
use tokio::sync::watch;

use super::command;
use super::stderr::{LastError, OutputLog, spawn_reader};
use crate::error::{MediaError, Result};
use crate::jpeg::JpegSplitter;
use crate::progress::EncoderStats;

/// Latest preview frame (JPEG bytes).
pub type PreviewFrame = Option<Arc<Vec<u8>>>;

pub struct Encoder {
    child: Child,
    /// Intermediate stream input. Taken by the feeder.
    pub stdin: Option<ChildStdin>,
    pub stats: watch::Receiver<EncoderStats>,
    pub preview: watch::Receiver<PreviewFrame>,
    pub last_error: LastError,
    /// Recent problems of network outputs.
    pub outputs: OutputLog,
}

impl Encoder {
    pub fn spawn(ffmpeg: &str, args: &[String]) -> Result<Self> {
        let mut child = command(ffmpeg)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| MediaError::Spawn { program: ffmpeg.to_string(), source: e })?;
        let stdin = child.stdin.take();
        let (stats_tx, stats) = watch::channel(EncoderStats::default());
        let (prev_tx, preview) = watch::channel(None);
        let last_error: LastError = Arc::new(Mutex::new(None));
        let outputs: OutputLog = Arc::new(Mutex::new(Vec::new()));
        if let Some(err) = child.stderr.take() {
            spawn_reader(err, "encoder".into(), Some(stats_tx), last_error.clone(), Some(outputs.clone()));
        }
        if let Some(mut out) = child.stdout.take() {
            tokio::spawn(async move {
                let mut split = JpegSplitter::default();
                let mut buf = vec![0u8; 64 * 1024];
                while let Ok(n) = out.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if let Some(f) = split.push(&buf[..n]).pop() {
                        let _ = prev_tx.send(Some(Arc::new(f)));
                    }
                }
            });
        }
        Ok(Encoder { child, stdin, stats, preview, last_error, outputs })
    }

    /// Non-blocking exit check.
    pub fn try_exited(&mut self) -> Option<bool> {
        self.child.try_wait().ok().flatten().map(|s| s.success())
    }

    pub async fn wait(&mut self) -> Result<bool> {
        Ok(self.child.wait().await?.success())
    }

    /// Close stdin (lets ffmpeg flush outputs) and wait up to `grace`.
    pub async fn shutdown(&mut self, grace: std::time::Duration) {
        self.stdin.take();
        if tokio::time::timeout(grace, self.child.wait()).await.is_err() {
            let _ = self.child.kill().await;
        }
    }

    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }

    pub fn error_text(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|g| g.clone())
    }
}
