//! Opt-in LAN playback on a separate port; never exposes the control API.
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
    routing::get,
};
use lb_engine::Engine;
use std::{collections::HashMap, sync::Arc, time::Duration};

pub fn directory(h: &lb_engine::ChannelHandle) -> std::path::PathBuf {
    h.read(|d| d.dir.join("runtime/hls"))
}

async fn file(State(root): State<std::path::PathBuf>, Path(name): Path<String>) -> Result<Response, StatusCode> {
    // Only encoder-owned HLS files, never arbitrary channel media or config.
    if name.contains(['/', '\\']) || name.starts_with('.') || !(name.ends_with(".m3u8") || name.ends_with(".ts")) {
        return Err(StatusCode::NOT_FOUND);
    }
    let path = root.join(&name);
    let resolved = tokio::fs::canonicalize(&path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let root = tokio::fs::canonicalize(&root).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if !resolved.starts_with(&root) {
        return Err(StatusCode::NOT_FOUND);
    }
    let file = tokio::fs::File::open(resolved).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let size = file.metadata().await.map_err(|_| StatusCode::NOT_FOUND)?.len();
    let stream = futures_util::stream::try_unfold(file, |mut file| async move {
        use tokio::io::AsyncReadExt;
        let mut data = vec![0u8; 64 * 1024];
        let n = file.read(&mut data).await?;
        if n == 0 {
            return Ok::<_, std::io::Error>(None);
        }
        data.truncate(n);
        lb_core::traffic::sent(lb_core::traffic::HLS, n as u64);
        Ok(Some((data, file)))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, if name.ends_with(".m3u8") { "application/vnd.apple.mpegurl" } else { "video/mp2t" })
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::CONTENT_LENGTH, size)
        .body(Body::from_stream(stream))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub fn router(root: std::path::PathBuf) -> Router {
    Router::new().route("/{name}", get(file)).with_state(root)
}

pub async fn monitor(engine: Arc<Engine>) {
    let mut running: HashMap<String, (u16, tokio::sync::oneshot::Sender<()>)> = HashMap::new();
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        let channels = engine.channels();
        running.retain(|id, (port, _)| {
            channels.iter().any(|h| h.id() == id && h.read(|d| d.config.local_hls.enabled && d.config.local_hls.port == *port))
        });
        for h in channels {
            let cfg = h.read(|d| d.config.local_hls.clone());
            if !cfg.enabled || running.contains_key(h.id()) {
                continue;
            }
            match tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, cfg.port)).await {
                Ok(listener) => {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let app = router(directory(&h));
                    tasks.spawn(async move {
                        let _ = axum::serve(listener, app)
                            .with_graceful_shutdown(async {
                                let _ = rx.await;
                            })
                            .await;
                    });
                    running.insert(h.id().to_string(), (cfg.port, tx));
                }
                Err(e) => tracing::warn!(channel = h.id(), port = cfg.port, "local HLS: {e}"),
            }
        }
        while tasks.try_join_next().is_some() {}
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn serves_only_hls_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.m3u8"), "#EXTM3U\n").unwrap();
        std::fs::write(dir.path().join("secret.toml"), "secret").unwrap();
        let app = router(dir.path().to_path_buf());
        for (url, code) in [("/index.m3u8", 200), ("/secret.toml", 404), ("/..%2fsecret.ts", 404)] {
            let response = app.clone().oneshot(axum::http::Request::builder().uri(url).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status().as_u16(), code);
            if code == 200 {
                let bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
                assert_eq!(bytes.as_ref(), b"#EXTM3U\n");
            }
        }
    }
}
