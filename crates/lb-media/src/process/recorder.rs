//! Recorder process: files from the encoder's local feed. Stopped with
//! `q` on stdin, so the file being written gets closed properly.

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin};

use super::command;
use super::stderr::{LastError, spawn_reader};
use crate::error::{MediaError, Result};

pub struct Recorder {
    child: Child,
    stdin: Option<ChildStdin>,
    pub last_error: LastError,
}

impl Recorder {
    pub fn spawn(ffmpeg: &str, args: &[String]) -> Result<Self> {
        let mut child = command(ffmpeg)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| MediaError::Spawn { program: ffmpeg.to_string(), source: e })?;
        let stdin = child.stdin.take();
        let last_error: LastError = Arc::new(Mutex::new(None));
        if let Some(err) = child.stderr.take() {
            spawn_reader(err, "recorder".into(), None, last_error.clone(), None);
        }
        Ok(Recorder { child, stdin, last_error })
    }

    /// Non-blocking exit check.
    pub fn try_exited(&mut self) -> Option<bool> {
        self.child.try_wait().ok().flatten().map(|s| s.success())
    }

    /// Ask ffmpeg to finish the file and quit; killed after `grace`.
    pub async fn shutdown(&mut self, grace: Duration) {
        if let Some(mut i) = self.stdin.take() {
            let _ = i.write_all(b"q").await;
            let _ = i.flush().await;
        }
        if tokio::time::timeout(grace, self.child.wait()).await.is_err() {
            let _ = self.child.kill().await;
        }
    }

    pub fn error_text(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|g| g.clone())
    }
}
