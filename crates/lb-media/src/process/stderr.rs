//! Background reader for ffmpeg stderr: forwards log lines to `tracing`,
//! parses `-progress` blocks and remembers the last error line.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::watch;

use crate::progress::{EncoderStats, ProgressParser};

/// Shared "last error line" slot.
pub type LastError = Arc<Mutex<Option<String>>>;

pub fn spawn_reader<R>(stderr: R, label: String, stats: Option<watch::Sender<EncoderStats>>, last_error: LastError)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut parser = ProgressParser::default();
        while let Ok(Some(line)) = lines.next_line().await {
            if stats.is_some() && ProgressParser::is_progress_line(&line) {
                if let (Some(s), Some(tx)) = (parser.feed(&line), &stats) {
                    let _ = tx.send(s);
                }
                continue;
            }
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            tracing::warn!(target: "ffmpeg", process = %label, "{l}");
            if let Ok(mut g) = last_error.lock() {
                *g = Some(l.to_string());
            }
        }
    });
}
