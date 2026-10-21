//! Exit codes and user-facing error type.

use std::fmt;

/// Documented process exit codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Ok = 0,
    Error = 1,
    Invalid = 2,
    NotFound = 3,
    Connection = 4,
    AlreadyRunning = 5,
    Cancelled = 6,
}

#[derive(Debug)]
pub struct CliError {
    pub code: Code,
    pub message: String,
}

impl CliError {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        CliError { code, message: message.into() }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

/// Map any error to an exit code (by type where known).
pub fn code_of(e: &anyhow::Error) -> Code {
    if let Some(c) = e.downcast_ref::<CliError>() {
        return c.code;
    }
    if let Some(e) = e.downcast_ref::<lb_engine::EngineError>() {
        return match e {
            lb_engine::EngineError::AlreadyRunning(_) => Code::AlreadyRunning,
            lb_engine::EngineError::UnknownChannel(_) => Code::NotFound,
            lb_engine::EngineError::InvalidConfig(_) => Code::Invalid,
            _ => Code::Error,
        };
    }
    if let Some(e) = e.downcast_ref::<lb_core::CoreError>() {
        return match e {
            lb_core::CoreError::NotFound(_) => Code::NotFound,
            lb_core::CoreError::Invalid(_) | lb_core::CoreError::Parse { .. } => Code::Invalid,
            _ => Code::Error,
        };
    }
    Code::Error
}

pub fn bail(code: Code, msg: impl Into<String>) -> anyhow::Error {
    CliError::new(code, msg).into()
}
