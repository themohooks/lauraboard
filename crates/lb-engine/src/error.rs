#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("channel '{0}' is already running in another process")]
    AlreadyRunning(String),
    #[error("channel '{0}' not found")]
    UnknownChannel(String),
    #[error("configuration has errors: {0}")]
    InvalidConfig(String),
    #[error(transparent)]
    Core(#[from] lb_core::CoreError),
    #[error(transparent)]
    Media(#[from] lb_media::MediaError),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("channel task stopped")]
    ChannelGone,
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = EngineError> = std::result::Result<T, E>;
