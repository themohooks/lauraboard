//! Spawning and supervising ffmpeg processes.
//!
//! * [`decoder`] — per-item decoder producing the intermediate stream
//! * [`encoder`] — long-running encoder with stats and preview
//! * [`recorder`] — recording to files from the encoder's local feed
//! * [`stderr`] — stderr reader: logging, progress parsing, last error

pub mod decoder;
pub mod encoder;
pub mod recorder;
pub mod stderr;

pub use decoder::Decoder;
pub use encoder::Encoder;
pub use recorder::Recorder;

use tokio::process::Command;

/// A `Command` configured for background ffmpeg use: killed when dropped,
/// no console window on Windows.
pub fn command(program: &str) -> Command {
    let mut c = Command::new(bundled_program(program));
    c.kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Prefer shipped media tools for default names; explicit paths still win.
pub fn bundled_program(program: &str) -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return bundled_in_directory(program, dir);
        }
    }
    program.into()
}

fn bundled_in_directory(program: &str, dir: &std::path::Path) -> std::path::PathBuf {
    if matches!(program, "ffmpeg" | "ffprobe" | "ffmpeg.exe" | "ffprobe.exe") {
        let name = format!("{}{}", program.trim_end_matches(".exe"), std::env::consts::EXE_SUFFIX);
        let bundled = dir.join(name);
        if bundled.is_file() {
            return bundled;
        }
    }
    program.into()
}

#[cfg(test)]
mod tests {
    use super::bundled_in_directory;
    use std::path::PathBuf;

    #[test]
    fn bundled_tools_win_but_explicit_paths_and_fallback_are_preserved() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("lauraboard-media-{}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        for tool in ["ffmpeg", "ffprobe"] {
            let binary = dir.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX));
            assert_eq!(bundled_in_directory(tool, &dir), PathBuf::from(tool));
            std::fs::write(&binary, b"test").unwrap();
            assert_eq!(bundled_in_directory(tool, &dir), binary);
            let explicit = format!("custom/{tool}");
            assert_eq!(bundled_in_directory(&explicit, &dir), PathBuf::from(explicit));
            std::fs::remove_file(binary).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
