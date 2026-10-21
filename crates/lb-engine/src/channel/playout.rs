//! Playing one item: run its decoder and pump the output into the encoder.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::sync::{Mutex, mpsc, watch};

use lb_media::process::Decoder;

/// Encoder stdin shared between consecutive items.
pub type SharedStdin = Arc<Mutex<Option<ChildStdin>>>;

/// MPEG-TS packet size: interruptions are aligned to it so the encoder
/// never receives a truncated packet.
const TS_PACKET: u64 = 188;

#[derive(Debug, Clone, PartialEq)]
pub enum PlayOutcome {
    /// Decoder finished normally.
    Completed,
    /// Decoder failed; `bytes` tells whether anything aired.
    Failed { error: String, bytes: u64 },
    /// The encoder went away while feeding.
    EncoderGone,
    /// Stopped on request (skip, interrupt).
    Stopped,
}

/// Handle to a running play task.
#[derive(Clone)]
pub struct PlayHandle {
    stop: Arc<watch::Sender<bool>>,
}

impl PlayHandle {
    /// Ask the task to stop at the next packet boundary (kills the decoder).
    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }
}

impl std::fmt::Debug for PlayHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PlayHandle")
    }
}

/// Spawn the decoder and feed its output to the shared encoder stdin. The
/// outcome is sent to `done` tagged with `generation`.
pub fn spawn(
    ffmpeg: String,
    args: Vec<String>,
    label: String,
    sink: SharedStdin,
    generation: u64,
    done: mpsc::Sender<(u64, PlayOutcome)>,
) -> PlayHandle {
    let (stop, stop_rx) = watch::channel(false);
    tokio::spawn(async move {
        let outcome = feed(&ffmpeg, &args, &label, &sink, stop_rx).await;
        let _ = done.send((generation, outcome)).await;
    });
    PlayHandle { stop: Arc::new(stop) }
}

async fn feed(ffmpeg: &str, args: &[String], label: &str, sink: &SharedStdin, mut stop: watch::Receiver<bool>) -> PlayOutcome {
    let mut dec = match Decoder::spawn(ffmpeg, args, label) {
        Ok(d) => d,
        Err(e) => return PlayOutcome::Failed { error: e.to_string(), bytes: 0 },
    };
    let mut guard = sink.lock().await;
    let Some(stdin) = guard.as_mut() else {
        dec.kill().await;
        return PlayOutcome::EncoderGone;
    };
    let mut buf = vec![0u8; 188 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = tokio::select! {
            r = dec.stdout.read(&mut buf) => match r {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return PlayOutcome::Failed { error: e.to_string(), bytes: total },
            },
            _ = stop.changed() => {
                // complete the current TS packet, then stop
                let missing = ((TS_PACKET - total % TS_PACKET) % TS_PACKET) as usize;
                if missing > 0 {
                    let mut tail = vec![0u8; missing];
                    let read = tokio::time::timeout(Duration::from_millis(500), dec.stdout.read_exact(&mut tail)).await;
                    if matches!(read, Ok(Ok(_))) {
                        let _ = stdin.write_all(&tail).await;
                    }
                }
                let _ = stdin.flush().await;
                dec.kill().await;
                return PlayOutcome::Stopped;
            }
        };
        if stdin.write_all(&buf[..n]).await.is_err() {
            dec.kill().await;
            *guard = None;
            return PlayOutcome::EncoderGone;
        }
        total += n as u64;
    }
    let _ = stdin.flush().await;
    match dec.wait_code().await {
        Ok(0) if total > 0 => PlayOutcome::Completed,
        status => {
            let code = status.map(|c| c.to_string()).unwrap_or_else(|e| e.to_string());
            let detail = dec.error_text().map(|e| format!(": {e}")).unwrap_or_default();
            PlayOutcome::Failed { error: format!("decoder exited with code {code} after {total} bytes{detail}"), bytes: total }
        }
    }
}
