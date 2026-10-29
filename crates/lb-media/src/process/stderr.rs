//! Background reader for ffmpeg stderr: forwards log lines to `tracing`,
//! parses `-progress` blocks and remembers the last error line, and the
//! recent problems of network outputs (they fail without stopping ffmpeg).
//! Stream keys are masked in everything that leaves the reader.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::watch;

use crate::progress::{EncoderStats, ProgressParser};

/// Shared "last error line" slot.
pub type LastError = Arc<Mutex<Option<String>>>;

/// Recent problems of queued (network) outputs: when, and the raw line
/// (it names the target URL; match it against the configured outputs).
pub type OutputLog = Arc<Mutex<Vec<(Instant, String)>>>;
const OUTPUT_LOG_LEN: usize = 32;

pub fn spawn_reader<R>(
    stderr: R,
    label: String,
    stats: Option<watch::Sender<EncoderStats>>,
    last_error: LastError,
    outputs: Option<OutputLog>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut parser = ProgressParser::default();
        let started = Instant::now();
        let mut previous = started;
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + Duration::from_secs(1), Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let line = tokio::select! {
                line = lines.next_line() => match line { Ok(Some(line)) => line, _ => break },
                _ = tick.tick(), if stats.is_some() => {
                    let now = Instant::now();
                    let sample = parser.sample(started.elapsed(), now.duration_since(previous));
                    previous = now;
                    if let Some(tx) = &stats { let _ = tx.send(sample); }
                    continue;
                }
            };
            if stats.is_some() && ProgressParser::is_telemetry_line(&line) {
                parser.feed(&line);
                continue;
            }
            if stats.is_some() && ProgressParser::is_progress_line(&line) {
                // Rates are published by the clock above, even if progress
                // stops. A terminal block is still delivered before EOF.
                if line.trim() == "progress=end" {
                    parser.feed(&line);
                    if let Some(tx) = &stats {
                        let _ = tx.send(parser.sample(started.elapsed(), previous.elapsed()));
                    }
                } else {
                    parser.feed(&line);
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
            let shown = crate::redact::urls(l);
            tracing::warn!(target: "ffmpeg", process = %label, "{shown}");
            if let Some(log) = &outputs
                && is_output_problem(l)
            {
                if let Ok(mut g) = log.lock() {
                    if g.len() >= OUTPUT_LOG_LEN {
                        g.remove(0);
                    }
                    g.push((Instant::now(), l.to_string()));
                }
                // the encoder keeps running: not its error
                continue;
            }
            if let Ok(mut g) = last_error.lock() {
                // a generic closing line must not hide the real cause
                if g.is_none() || !is_trailer(l) {
                    *g = Some(shown);
                }
            }
        }
    });
}

/// Expected messages of the concatenated pipeline that are not problems.
fn is_benign(l: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "timestamp discontinuity",
        "is muxed as a private data stream",
        "deprecated pixel format used",
        // a network output is down; its own error line says why
        "FIFO queue full",
    ];
    PATTERNS.iter().any(|p| l.contains(p))
}

/// Lines ffmpeg prints after the actual error: summaries on the way out
/// and the knock-on failures of other parts of the pipeline (an output that
/// cannot open makes the preview encoder report "Invalid argument", …).
fn is_trailer(l: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "Nothing was written into output file",
        "Conversion failed",
        "Terminating thread with return code",
        "Task finished with error code",
        "Could not open encoder before EOF",
        "Error sending frames to consumers",
        "All tee outputs failed",
        "Could not write header",
        "Error muxing a packet",
        "Error submitting a packet to the muxer",
        "Error closing file",
        "Error writing trailer",
        "Exiting normally",
    ];
    PATTERNS.iter().any(|p| l.contains(p))
}

/// A queued output failed or is retrying (ffmpeg keeps running).
fn is_output_problem(l: &str) -> bool {
    l.starts_with("[fifo @") || (l.starts_with("[tee @") && l.contains("Slave muxer"))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn publishes_zero_rates_even_when_ffmpeg_stops_reporting() {
        use super::*;
        use tokio::io::AsyncWriteExt;
        let (mut input, output) = tokio::io::duplex(4096);
        let (tx, mut rx) = watch::channel(EncoderStats::default());
        spawn_reader(output, "test".into(), Some(tx), Arc::new(Mutex::new(None)), None);
        input.write_all(b"LB_PACKET 0 1000\nLB_FRAME 0\nframe=1\nout_time_us=40000\nprogress=continue\n").await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), rx.changed()).await.unwrap().unwrap();
        assert!(rx.borrow().bitrate_kbps > 0.0);
        tokio::time::timeout(Duration::from_secs(3), rx.changed()).await.unwrap().unwrap();
        let stalled = rx.borrow().clone();
        assert_eq!(stalled.bitrate_kbps, 0.0);
        assert_eq!(stalled.fps, 0.0);
        assert!(stalled.lag_ms >= 900);
    }

    #[test]
    fn benign_patterns() {
        assert!(super::is_benign("[in#0/mpegts] timestamp discontinuity (stream id=257)"));
        assert!(!super::is_benign("Connection refused"));
        assert!(super::is_trailer(
            "[out#1/image2pipe @ 0000] Nothing was written into output file, because at least one of its streams received no packets."
        ));
        assert!(!super::is_trailer("tcp://127.0.0.1:5000: Connection refused"));
        assert!(super::is_trailer("[vost#1:0/mjpeg @ 0000] Task finished with error code: -22 (Invalid argument)"));
        assert!(!super::is_trailer("[tee @ 0000] Slave '[f=flv]rtmps://h/s/k': error opening: I/O error"));
        assert!(super::is_output_problem("[fifo @ 0000] Error opening rtmps://h/s/k: I/O error"));
        assert!(!super::is_output_problem("[in#0/mpegts @ 0000] Packet corrupt"));
    }
}
