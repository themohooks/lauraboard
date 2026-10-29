//! Connection form validation shared with host-side tests.
use crate::GuiOptions;

pub fn options(url: &str, token: &str) -> Result<GuiOptions, String> {
    let base = url.trim().trim_end_matches('/');
    let valid = !base.contains('#') && ureq::http::Uri::try_from(base).ok().is_some_and(|u| {
        matches!(u.scheme_str(), Some("http" | "https")) && u.host().is_some()
            && u.authority().is_some_and(|a| !a.as_str().contains('@')) && u.query().is_none()
    });
    if !valid { return Err("Укажите URL сервера: http://192.168.1.10:8787".into()); }
    let token = token.trim();
    Ok(GuiOptions { base_url: base.into(), token: (!token.is_empty()).then(|| token.into()), channel: None, embedded_engine: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_remote_connection() {
        let opts = options(" http://192.168.1.10:8787/ ", " secret ").unwrap();
        assert_eq!(opts.base_url, "http://192.168.1.10:8787");
        assert_eq!(opts.token.as_deref(), Some("secret"));
        assert!(!opts.embedded_engine);
        assert!(options("https://server.example", " ").unwrap().token.is_none());
        assert!(options("http://[::1]:8787", "").is_ok());
        for invalid in ["", "server:8787", "ftp://server", "http://", "http://user:pass@server", "http://server?token=secret", "http://server#fragment"] {
            assert!(options(invalid, "").is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn connection_probe_handles_auth_and_failure() {
        use std::io::{Read, Write};
        for (status, body, success) in [(200, "[]", true), (401, r#"{"error":"unauthorized"}"#, false), (200, "invalid json", false)] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") { stream.read_exact(&mut byte).unwrap(); request.push(byte[0]); }
                let request = String::from_utf8(request).unwrap().to_lowercase();
                assert!(request.starts_with("get /api/v1/channels "));
                assert!(request.contains("authorization: bearer secret"));
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let result = crate::net::api::Api::new(&base, Some("secret".into())).get::<Vec<serde_json::Value>>("/channels");
            assert_eq!(result.is_ok(), success);
            server.join().unwrap();
        }
    }
}
