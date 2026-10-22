//! Project automation, run through cargo aliases (`.cargo/config.toml`):
//!
//! * `cargo dist [--linux]` — release build of the application and the installer
//!   with the application packed inside → `target/dist/`;
//! * `cargo xtask icons` — render `laurabrod.svg` into `laurabrod.ico`
//!   (Windows executable icon, committed to the repository).

mod dist;
mod icons;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <dist | icons>\n  dist   build target/dist/laurabrod-<version>-<os>-<arch>-setup[.exe]\n  icons  regenerate crates/lb-gui/assets/app/laurabrod.ico";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().cloned().unwrap_or_default();
    let result = match task.as_str() {
        "dist" => dist::run(&args[1..]),
        "icons" => icons::run(),
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
