//! Hide stream keys in text shown to people (logs, errors in the UI).
//!
//! `rtmps://dc4-1.rtmp.t.me/s/3951560039:secret` → `rtmps://dc4-1.rtmp.t.me/s/***`:
//! the scheme, host and first path segment stay (enough to recognise the
//! target), the rest of the path and any query are masked.

const SCHEMES: &[&str] = &["rtmp://", "rtmps://", "srt://", "rist://", "http://", "https://", "rtsp://"];

/// `text` with the secret part of every stream URL masked.
pub fn urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((at, scheme)) = find_scheme(rest) {
        out.push_str(&rest[..at]);
        let url_start = &rest[at..];
        let len = url_start.find(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | '|' | ']')).unwrap_or(url_start.len());
        let mut url = &url_start[..len];
        // "…/key: I/O error" — the colon belongs to the message
        let mut tail = "";
        if let Some(u) = url.strip_suffix(':') {
            url = u;
            tail = ":";
        }
        out.push_str(&mask(url, scheme.len()));
        out.push_str(tail);
        rest = &url_start[len..];
    }
    out.push_str(rest);
    out
}

fn find_scheme(s: &str) -> Option<(usize, &'static str)> {
    let lower = s.to_ascii_lowercase();
    SCHEMES.iter().filter_map(|sch| lower.find(sch).map(|i| (i, *sch))).min_by_key(|(i, _)| *i)
}

fn mask(url: &str, scheme_len: usize) -> String {
    let (base, query) = match url.find('?') {
        Some(q) => (&url[..q], Some(())),
        None => (url, None),
    };
    let after = &base[scheme_len..];
    let mut parts = after.splitn(3, '/');
    let host = parts.next().unwrap_or_default();
    let first = parts.next();
    let more = parts.next();
    let mut s = format!("{}{host}", &base[..scheme_len]);
    match (first, more) {
        (Some(f), Some(m)) if !m.is_empty() => s.push_str(&format!("/{f}/***")),
        (Some(f), _) if !f.is_empty() => s.push_str("/***"),
        _ => {}
    }
    if query.is_some() {
        s.push_str("?***");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::urls;

    #[test]
    fn masks_keys() {
        assert_eq!(
            urls("[fifo @ 0x1] Error opening rtmps://dc4-1.rtmp.t.me/s/3951560039:nxyxoq7: I/O error"),
            "[fifo @ 0x1] Error opening rtmps://dc4-1.rtmp.t.me/s/***: I/O error"
        );
        assert_eq!(urls("rtmp://a.rtmp.youtube.com/live2/abcd-efgh"), "rtmp://a.rtmp.youtube.com/live2/***");
        assert_eq!(urls("srt://1.2.3.4:9000?streamid=x&passphrase=y"), "srt://1.2.3.4:9000?***");
        assert_eq!(urls("[f=flv]rtmp://h/app/key|[f=hls]out.m3u8"), "[f=flv]rtmp://h/app/***|[f=hls]out.m3u8");
        assert_eq!(urls("no urls ./output/hls/live.m3u8"), "no urls ./output/hls/live.m3u8");
        assert_eq!(urls("rtmp://h/key"), "rtmp://h/***");
    }
}
