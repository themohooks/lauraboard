//! Parser for ffmpeg `-progress` key=value blocks.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EncoderStats {
    pub frame: u64,
    pub fps: f64,
    pub bitrate_kbps: f64,
    pub total_size: u64,
    pub out_time_ms: i64,
    pub speed: f64,
    pub dup_frames: u64,
    pub drop_frames: u64,
    /// Frames dropped/repeated by synchronization before the main encoder.
    pub sync_drop_frames: u64,
    pub sync_dup_frames: u64,
    /// Current delay relative to the first progress sample, excluding startup.
    pub lag_ms: u64,
    /// Bitrate is sampled from main encoded A/V packets (not container overhead).
    pub packet_telemetry: bool,
}

/// Incremental parser: feed lines, get a snapshot at each `progress=` line.
#[derive(Debug, Default)]
pub struct ProgressParser {
    cur: EncoderStats,
    packet_bytes: u64,
    sampled_bytes: u64,
    video_packets: u64,
    sampled_video: u64,
    input_index: Option<u64>,
    media_origin: Option<(std::time::Duration, i64)>,
}

impl ProgressParser {
    /// Returns `Some(stats)` when a block is complete. Lines that are not
    /// `key=value` progress lines return `None` and are ignored.
    pub fn feed(&mut self, line: &str) -> Option<EncoderStats> {
        let line = line.trim();
        if let Some(packet) = line.strip_prefix("LB_PACKET ") {
            let mut fields = packet.split_whitespace();
            if let (Some(stream), Some(size)) =
                (fields.next().and_then(|s| s.parse::<u32>().ok()), fields.next().and_then(|s| s.parse::<u64>().ok()))
            {
                self.cur.packet_telemetry = true;
                self.packet_bytes = self.packet_bytes.saturating_add(size);
                if stream == 0 {
                    self.video_packets += 1;
                }
            }
            return None;
        }
        if let Some(index) = line.strip_prefix("LB_FRAME ").and_then(|s| s.trim().parse::<u64>().ok()) {
            if let Some(previous) = self.input_index {
                if index == previous {
                    self.cur.sync_dup_frames += 1;
                } else if index > previous {
                    self.cur.sync_drop_frames += index - previous - 1;
                }
            } else {
                self.cur.sync_drop_frames = index;
            }
            self.input_index = Some(index);
            return None;
        }
        let (k, v) = line.split_once('=')?;
        let v = v.trim();
        match k {
            "frame" => self.cur.frame = v.parse().unwrap_or(0),
            "fps" => self.cur.fps = v.parse().unwrap_or(0.0),
            "bitrate" => self.cur.bitrate_kbps = v.trim_end_matches("kbits/s").trim().parse().unwrap_or(0.0),
            "total_size" => self.cur.total_size = v.parse().unwrap_or(0),
            "out_time_us" | "out_time_ms" => self.cur.out_time_ms = v.parse::<i64>().map(|x| x / 1000).unwrap_or(0),
            "speed" => self.cur.speed = v.trim_end_matches('x').trim().parse().unwrap_or(0.0),
            "dup_frames" => self.cur.dup_frames = v.parse().unwrap_or(0),
            "drop_frames" => self.cur.drop_frames = v.parse().unwrap_or(0),
            "progress" => return Some(self.cur.clone()),
            _ => {}
        }
        None
    }

    /// One-second live rates, including zero during a stalled encoder. Lifetime
    /// averages from -progress and bytes from the preview must not leak in.
    pub fn sample(&mut self, elapsed: std::time::Duration, period: std::time::Duration) -> EncoderStats {
        let secs = period.as_secs_f64().max(0.001);
        let mut stats = self.cur.clone();
        if stats.packet_telemetry {
            stats.bitrate_kbps = (self.packet_bytes - self.sampled_bytes) as f64 * 8.0 / secs / 1000.0;
            stats.total_size = self.packet_bytes;
            stats.fps = (self.video_packets - self.sampled_video) as f64 / secs;
        }
        self.sampled_bytes = self.packet_bytes;
        self.sampled_video = self.video_packets;
        if self.cur.out_time_ms > 0 {
            let (origin, media) = *self.media_origin.get_or_insert((elapsed, self.cur.out_time_ms));
            let wall_ms = elapsed.saturating_sub(origin).as_millis() as i64;
            stats.lag_ms = wall_ms.saturating_sub(self.cur.out_time_ms.saturating_sub(media)).max(0) as u64;
        }
        stats
    }

    pub fn is_telemetry_line(line: &str) -> bool {
        line.starts_with("LB_PACKET ") || line.starts_with("LB_FRAME ")
    }

    /// Whether a stderr line belongs to the progress output.
    pub fn is_progress_line(line: &str) -> bool {
        let Some((k, _)) = line.split_once('=') else { return false };
        !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_block() {
        let mut p = ProgressParser::default();
        let lines = ["frame=250", "fps=25.00", "bitrate=3012.4kbits/s", "out_time_us=10000000", "speed=1.01x", "drop_frames=2"];
        for l in lines {
            assert!(p.feed(l).is_none());
        }
        let s = p.feed("progress=continue").unwrap();
        assert_eq!(s.frame, 250);
        assert_eq!(s.out_time_ms, 10_000);
        assert!((s.speed - 1.01).abs() < 1e-9);
        assert!((s.bitrate_kbps - 3012.4).abs() < 1e-9);
        assert_eq!(s.drop_frames, 2);
        assert!(ProgressParser::is_progress_line("fps=25"));
        assert!(!ProgressParser::is_progress_line("[tee @ 0x1] Slave failed"));
    }

    #[test]
    fn packet_rates_measure_each_interval_and_zero_when_stalled() {
        use std::time::Duration;
        let mut p = ProgressParser::default();
        p.feed("LB_PACKET 0 1000");
        p.feed("LB_PACKET 0 2000");
        p.feed("LB_PACKET 1 1000");
        p.feed("bitrate=N/A");
        let first = p.sample(Duration::from_secs(1), Duration::from_secs(1));
        assert_eq!(first.bitrate_kbps, 32.0);
        assert_eq!(first.fps, 2.0);
        assert_eq!(first.total_size, 4000);
        let idle = p.sample(Duration::from_secs(2), Duration::from_secs(1));
        assert_eq!(idle.bitrate_kbps, 0.0);
        assert_eq!(idle.fps, 0.0);
        p.feed("LB_PACKET 0 500");
        let recovered = p.sample(Duration::from_secs(4), Duration::from_secs(2));
        assert_eq!(recovered.bitrate_kbps, 2.0);
        assert_eq!(recovered.fps, 0.5);
    }

    #[test]
    fn detects_filter_drops_and_duplicates_including_initial_skips_without_counting_index_resets() {
        let mut p = ProgressParser::default();
        for index in [2, 3, 5, 5, 8, 0, 1] {
            p.feed(&format!("LB_FRAME {index}"));
        }
        p.feed("LB_FRAME N/A");
        let stats = p.sample(std::time::Duration::from_secs(1), std::time::Duration::from_secs(1));
        assert_eq!(stats.sync_drop_frames, 5);
        assert_eq!(stats.sync_dup_frames, 1);
        assert_eq!(stats.drop_frames, 0);
    }

    #[test]
    fn stalled_output_accumulates_delay_and_catchup_clears_it() {
        use std::time::Duration;
        let mut p = ProgressParser::default();
        p.feed("out_time_us=1000000");
        assert_eq!(p.sample(Duration::from_secs(5), Duration::from_secs(1)).lag_ms, 0);
        assert_eq!(p.sample(Duration::from_secs(7), Duration::from_secs(1)).lag_ms, 2000);
        p.feed("out_time_us=4000000");
        assert_eq!(p.sample(Duration::from_secs(8), Duration::from_secs(1)).lag_ms, 0);
    }
}
