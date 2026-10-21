//! Playing one item: run its decoder and pump the output into the encoder.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::sync::{Mutex, mpsc};
use tokio::task::AbortHandle;

use lb_media::process::Decoder;

/// Encoder stdin shared between consecutive items.
pub type SharedStdin = Arc<Mutex<Option<ChildStdin>>>;

#[derive(Debug, Clone, PartialEq)]
pub enum PlayOutcome {
    /// Decoder finished normally.
    Completed,
    /// Decoder failed; `bytes` tells whether anything aired.
    Failed { error: String, bytes: u64 },
    /// The encoder went away while feeding.
    EncoderGone,
}

/// Spawn the decoder and feed its output to the shared encoder stdin. The
/// outcome is sent to `done` tagged with `generation`; aborting the returned
/// handle kills the decoder and sends nothing.
pub fn spawn(
    ffmpeg: String,
    args: Vec<String>,
    label: String,
    sink: SharedStdin,
    generation: u64,
    done: mpsc::Sender<(u64, PlayOutcome)>,
) -> AbortHandle {
    let h = tokio::spawn(async move {
        let outcome = feed(&ffmpeg, &args, &label, &sink).await;
        let _ = done.send((generation, outcome)).await;
    });
    h.abort_handle()
}

async fn feed(ffmpeg: &str, args: &[String], label: &str, sink: &SharedStdin) -> PlayOutcome {
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
        let n = match dec.stdout.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return PlayOutcome::Failed { error: e.to_string(), bytes: total },
        };
        if stdin.write_all(&buf[..n]).await.is_err() {
            dec.kill().await;
            *guard = None;
            return PlayOutcome::EncoderGone;
        }
        total += n as u64;
    }
    let _ = stdin.flush().await;
    match dec.wait().await {
        Ok(true) if total > 0 => PlayOutcome::Completed,
        _ => PlayOutcome::Failed { error: dec.error_text().unwrap_or_else(|| "decoder failed".into()), bytes: total },
    }
}
