//! Per-item decoder process.

use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::process::{Child, ChildStdout};

use super::command;
use super::stderr::{LastError, spawn_reader};
use crate::error::{MediaError, Result};

pub struct Decoder {
    child: Child,
    /// Normalised MPEG-TS output.
    pub stdout: ChildStdout,
    pub last_error: LastError,
}

impl Decoder {
    pub fn spawn(ffmpeg: &str, args: &[String], label: &str) -> Result<Self> {
        let mut child = command(ffmpeg)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| MediaError::Spawn { program: ffmpeg.to_string(), source: e })?;
        let stdout = child.stdout.take().ok_or_else(|| MediaError::Other("decoder stdout missing".into()))?;
        let last_error: LastError = Arc::new(Mutex::new(None));
        if let Some(err) = child.stderr.take() {
            spawn_reader(err, label.to_string(), None, last_error.clone());
        }
        Ok(Decoder { child, stdout, last_error })
    }

    /// Wait for exit; returns the exit code (-1 when killed by a signal).
    pub async fn wait_code(&mut self) -> Result<i32> {
        Ok(self.child.wait().await?.code().unwrap_or(-1))
    }

    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }

    pub fn error_text(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|g| g.clone())
    }
}
