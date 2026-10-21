//! Reading / writing TOML and JSON files with consistent errors and
//! atomic replacement (write to temp file, then rename).

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::path::Path;

use crate::error::{CoreError, Result};

pub fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
    toml::from_str(&text).map_err(|e| CoreError::Parse { path: path.into(), message: e.to_string() })
}

pub fn read_toml_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    if path.exists() { read_toml(path) } else { Ok(T::default()) }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
    serde_json::from_str(&text).map_err(|e| CoreError::Parse { path: path.into(), message: e.to_string() })
}

pub fn read_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    if path.exists() { read_json(path) } else { Ok(T::default()) }
}

pub fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text =
        toml::to_string_pretty(value).map_err(|e| CoreError::Serialize { what: path.display().to_string(), message: e.to_string() })?;
    write_atomic(path, text.as_bytes())
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| CoreError::Serialize { what: path.display().to_string(), message: e.to_string() })?;
    write_atomic(path, text.as_bytes())
}

/// Write via a temporary file and rename, so a crash never leaves a
/// half-written configuration.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    }
    let tmp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    fs::write(&tmp, data).map_err(|e| CoreError::io(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| CoreError::io(path, e))
}
