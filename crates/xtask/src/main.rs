//! Project automation, run through cargo aliases (`.cargo/config.toml`):
//!
//! * `cargo dist [--linux]` — release build of the application and the installer
//!   with the application packed inside → `target/dist/`;
//! * `cargo xtask icons` — render `lauraboard.svg` into `lauraboard.ico`
//!   (Windows executable icon, committed to the repository);
//! * `cargo xtask backgrounds [--force]` — render the ready-made backgrounds
//!   the engine embeds (`crates/lb-engine/assets/backgrounds/`).

mod backgrounds;
mod dist;
mod icons;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <dist | icons | backgrounds>\n  dist   build target/dist/lauraboard-<version>-<os>-<arch>-setup[.exe]\n  icons  regenerate crates/lb-gui/assets/app/lauraboard.ico
  backgrounds  render crates/lb-engine/assets/backgrounds (needs ffmpeg)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().cloned().unwrap_or_default();
    let result = match task.as_str() {
        "dist" => dist::run(&args[1..]),
        "icons" => icons::run(),
        "backgrounds" => backgrounds::run(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {task}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (two levels above this crate).
pub fn root() -> PathBuf {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_dir.parent().and_then(|p| p.parent()).expect("workspace root").to_path_buf()
}

pub fn app_assets() -> PathBuf {
    root().join("crates/lb-gui/assets/app")
}
