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
            if is_benign(l) {
                tracing::debug!(target: "ffmpeg", process = %label, "{l}");
                continue;
            }
            tracing::warn!(target: "ffmpeg", process = %label, "{l}");
            if let Ok(mut g) = last_error.lock() {
                // a generic closing line must not hide the real cause
                if g.is_none() || !is_trailer(l) {
                    *g = Some(l.to_string());
                }
            }
        }
    });
}

/// Expected messages of the concatenated pipeline that are not problems.
fn is_benign(l: &str) -> bool {
    const PATTERNS: &[&str] = &["timestamp discontinuity", "is muxed as a private data stream", "deprecated pixel format used"];
    PATTERNS.iter().any(|p| l.contains(p))
}

/// Summary lines ffmpeg prints on the way out after the actual error.
fn is_trailer(l: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "Nothing was written into output file",
        "Conversion failed",
        "Terminating thread with return code",
        "Error closing file",
        "Error writing trailer",
        "Exiting normally",
    ];
    PATTERNS.iter().any(|p| l.contains(p))
}

#[cfg(test)]
mod tests {
    #[test]
    fn benign_patterns() {
        assert!(super::is_benign("[in#0/mpegts] timestamp discontinuity (stream id=257)"));
        assert!(!super::is_benign("Connection refused"));
        assert!(super::is_trailer("[out#1/image2pipe @ 0000] Nothing was written into output file, because at least one of its streams received no packets."));
        assert!(!super::is_trailer("tcp://127.0.0.1:5000: Connection refused"));
    }
}
