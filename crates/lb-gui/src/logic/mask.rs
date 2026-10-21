//! Hiding secrets in output URLs (stream keys, passwords, tokens) before
//! they are shown on screen.

/// `rtmp://host/app/KEY` → `rtmp://host/app/••••1234`; `user:pass@` and
/// `key=`/`token=`/`passphrase=` query values are masked as well.
pub fn mask_url(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (format!("{s}://"), r),
        None => (String::new(), url),
    };
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    // credentials in the authority part
    let path = match path.split_once('@') {
        Some((cred, host)) if !cred.contains('/') => {
            let user = cred.split(':').next().unwrap_or("");
            format!("{user}:••••@{host}")
        }
        _ => path.to_string(),
    };
    let lower = scheme.to_lowercase();
    let keyed = lower.starts_with("rtmp") || lower.starts_with("rtsp");
    let path = match path.rsplit_once('/') {
        // the last path segment of RTMP-like URLs is the stream key
        Some((head, key)) if keyed && head.contains('/') && !key.is_empty() => format!("{head}/{}", hide(key)),
        _ => path,
    };
    let query = query.map(|q| {
        q.split('&')
            .map(|kv| match kv.split_once('=') {
                Some((k, v)) if is_secret(k) => format!("{k}={}", hide(v)),
                _ => kv.to_string(),
            })
            .collect::<Vec<_>>()
            .join("&")
    });
    match query {
        Some(q) => format!("{scheme}{path}?{q}"),
        None => format!("{scheme}{path}"),
    }
}

fn is_secret(k: &str) -> bool {
    let k = k.to_lowercase();
    ["key", "token", "pass", "passphrase", "password", "secret", "streamid", "auth"].iter().any(|s| k.contains(s))
}

fn hide(v: &str) -> String {
    let n = v.chars().count();
    if n <= 4 {
        return "••••".into();
    }
    let tail: String = v.chars().skip(n - 4).collect();
    format!("••••{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_keys_and_credentials_are_hidden() {
        assert_eq!(mask_url("rtmp://a.rtmp.youtube.com/live2/abcd-efgh-1234"), "rtmp://a.rtmp.youtube.com/live2/••••1234");
        assert_eq!(mask_url("srt://host:9000?streamid=secret99&latency=200"), "srt://host:9000?streamid=••••et99&latency=200");
        assert_eq!(mask_url("rtmp://user:hunter2@host/app/key"), "rtmp://user:••••@host/app/••••");
        assert_eq!(mask_url("udp://239.0.0.1:1234"), "udp://239.0.0.1:1234");
        assert_eq!(mask_url("recordings/out.ts"), "recordings/out.ts");
    }
}
