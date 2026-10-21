//! Parser for ffmpeg `-progress` key=value blocks.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EncoderStats {
    pub frame: u64,
    pub fps: f64,
    pub bitrate_kbps: f64,
    pub total_size: u64,
    pub out_time_ms: i64,
    pub speed: f64,
    pub dup_frames: u64,
    pub drop_frames: u64,
}

/// Incremental parser: feed lines, get a snapshot at each `progress=` line.
#[derive(Debug, Default)]
pub struct ProgressParser {
    cur: EncoderStats,
}

impl ProgressParser {
    /// Returns `Some(stats)` when a block is complete. Lines that are not
    /// `key=value` progress lines return `None` and are ignored.
    pub fn feed(&mut self, line: &str) -> Option<EncoderStats> {
        let (k, v) = line.trim().split_once('=')?;
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
}
