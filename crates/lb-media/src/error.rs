#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("failed to start '{program}': {source} (is FFmpeg installed and configured in lauraboard.toml?)")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("ffprobe failed for '{input}': {message}")]
    Probe { input: String, message: String },
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = MediaError> = std::result::Result<T, E>;
