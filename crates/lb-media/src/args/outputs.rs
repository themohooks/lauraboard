//! Output targets → `tee` muxer slave specifications.

use lb_core::config::{OutputTarget, RecordConfig};

/// Guess the container from the URL when not configured.
pub fn guess_format(url: &str) -> &'static str {
    let l = url.to_ascii_lowercase();
    if l.starts_with("rtmp://") || l.starts_with("rtmps://") || l.ends_with(".flv") {
        "flv"
    } else if l.ends_with(".m3u8") {
        "hls"
    } else if l.ends_with(".mkv") {
        "matroska"
    } else if l.ends_with(".mp4") {
        "mp4"
    } else {
        // srt://, udp://, rtp://, tcp://, .ts and unknown
        "mpegts"
    }
}

/// Escape characters with special meaning in tee slave URLs.
fn escape_tee(url: &str) -> String {
    let mut s = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            '\\' => s.push('/'),
            '|' | '[' | ']' => {
                s.push('\\');
                s.push(c);
            }
            _ => s.push(c),
        }
    }
    s
}

/// One tee slave for an output target.
pub fn tee_slave(o: &OutputTarget) -> String {
    let fmt = o.format.clone().unwrap_or_else(|| guess_format(&o.url).to_string());
    let opts = match fmt.as_str() {
        "flv" => "f=flv:onfail=abort".to_string(),
        "hls" => "f=hls:hls_time=4:hls_list_size=8:hls_flags=delete_segments+independent_segments".to_string(),
        "mpegts" => "f=mpegts:bsfs/v=dump_extra".to_string(),
        "mp4" => "f=mp4:movflags=+frag_keyframe+empty_moov".to_string(),
        other => format!("f={other}"),
    };
    format!("[{opts}]{}", escape_tee(&o.url))
}

/// Tee slave for time-segmented local recording into `dir` (resolved).
pub fn record_slave(rec: &RecordConfig, dir: &str, channel: &str) -> String {
    let seg = (rec.segment.ms() / 1000).max(60);
    let pattern = format!("{}/{channel}-%Y%m%d-%H%M%S.ts", dir.trim_end_matches(['/', '\\']));
    format!("[f=segment:segment_time={seg}:segment_format=mpegts:strftime=1:reset_timestamps=1:onfail=ignore]{}", escape_tee(&pattern))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::Dur;

    fn out(url: &str) -> OutputTarget {
        OutputTarget { name: "x".into(), url: url.into(), enabled: true, format: None }
    }

    #[test]
    fn formats() {
        assert_eq!(guess_format("rtmp://a/live/key"), "flv");
        assert_eq!(guess_format("srt://1.2.3.4:9000"), "mpegts");
        assert_eq!(guess_format("C:/out/stream.m3u8"), "hls");
        assert_eq!(tee_slave(&out("rtmp://a/live/k")), "[f=flv:onfail=abort]rtmp://a/live/k");
        assert_eq!(tee_slave(&out("udp://239.0.0.1:1234")), "[f=mpegts:bsfs/v=dump_extra]udp://239.0.0.1:1234");
    }

    #[test]
    fn record_pattern() {
        let rec = RecordConfig { enabled: true, dir: "rec".into(), segment: Dur::mins(30) };
        let s = record_slave(&rec, "D:\\tv\\rec\\", "main");
        assert!(s.contains("segment_time=1800"));
        assert!(s.ends_with("]D:/tv/rec/main-%Y%m%d-%H%M%S.ts"));
    }
}
