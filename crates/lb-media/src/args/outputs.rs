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

/// Queue in front of a network target: its own thread, reconnecting every
/// few seconds after a failure and dropping packets meanwhile (escaped for
/// the tee slave option parser).
const NETWORK_FIFO: &str = r"use_fifo=1:fifo_options=attempt_recovery=1\\:recover_any_error=1\\:recovery_wait_time=5\\:drop_pkts_on_overflow=1\\:restart_with_keyframe=1\\:queue_size=300";

/// One tee slave for an output target.
///
/// A network target that cannot be reached (wrong key, server down) must
/// not stop the encoder: that would take every other output and the
/// preview with it and restart the whole pipeline over and over. It is
/// queued and retried on its own instead.
pub fn tee_slave(o: &OutputTarget) -> String {
    let fmt = o.format.clone().unwrap_or_else(|| guess_format(&o.url).to_string());
    let mut opts = match fmt.as_str() {
        "flv" => "f=flv".to_string(),
        "hls" => "f=hls:hls_time=4:hls_list_size=8:hls_flags=delete_segments+independent_segments".to_string(),
        "mpegts" => "f=mpegts:bsfs/v=dump_extra".to_string(),
        "mp4" => "f=mp4:movflags=+frag_keyframe+empty_moov".to_string(),
        other => format!("f={other}"),
    };
    if lb_core::media::is_url(&o.url) {
        opts.push_str(":onfail=ignore:");
        opts.push_str(NETWORK_FIFO);
    }
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
        let rtmp = tee_slave(&out("rtmp://a/live/k"));
        assert!(rtmp.starts_with(r"[f=flv:onfail=ignore:use_fifo=1:fifo_options=attempt_recovery=1\\:"), "{rtmp}");
        assert!(rtmp.ends_with("]rtmp://a/live/k"));
        assert!(tee_slave(&out("udp://239.0.0.1:1234")).starts_with("[f=mpegts:bsfs/v=dump_extra:onfail=ignore:use_fifo=1:"));
        // local files are written directly
        assert_eq!(tee_slave(&out("D:/tv/out.ts")), "[f=mpegts:bsfs/v=dump_extra]D:/tv/out.ts");
    }

    #[test]
    fn record_pattern() {
        let rec = RecordConfig { enabled: true, dir: "rec".into(), segment: Dur::mins(30) };
        let s = record_slave(&rec, "D:\\tv\\rec\\", "main");
        assert!(s.contains("segment_time=1800"));
        assert!(s.ends_with("]D:/tv/rec/main-%Y%m%d-%H%M%S.ts"));
    }
}
