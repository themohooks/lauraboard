//! `lauraboard-setup` — installer and uninstaller of LauraBoard.
//!
//! * with the application packed inside (`cargo dist`): installs or updates;
//! * without it (the copy shipped as `uninstall`) or with `--uninstall`:
//!   removes what the installation created.
//!
//! The wizard uses the operator GUI's Material 3 components; `--silent`
//! does the same without a window (for scripts and deployment).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod ffmpeg;
mod install;
mod options;
mod payload;
mod platform;
mod project;
mod receipt;
#[cfg(feature = "snapshot")]
mod snapshot;
mod ui;
mod uninstall;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use options::Options;
use receipt::Receipt;

#[derive(Parser, Debug)]
#[command(name = "lauraboard-setup", version, about = "Установка и удаление LauraBoard")]
struct Args {
    /// Remove the installed application.
    #[arg(long)]
    uninstall: bool,
    /// No window: use the defaults and the options below.
    #[arg(long)]
    silent: bool,
    /// Install folder (default: per-user programs folder).
    #[arg(long)]
    dir: Option<PathBuf>,
    /// No Start menu / applications menu entry.
    #[arg(long)]
    no_menu: bool,
    /// No desktop shortcut.
    #[arg(long)]
    no_desktop: bool,
    /// Do not put `lauraboard` on the PATH.
    #[arg(long)]
    no_path: bool,
    /// Project to create (if missing) and open from shortcuts.
    #[arg(long, conflicts_with = "no_project")]
    project: Option<PathBuf>,
    /// Do not create a project.
    #[arg(long)]
    no_project: bool,
    /// Fill a new project with the demo channel (needs FFmpeg).
    #[arg(long)]
    demo: bool,
    /// With --uninstall: also delete interface settings.
    #[arg(long)]
    forget_settings: bool,
    /// With --uninstall: also delete channel and broadcast data of the
    /// project opened by the shortcuts (settings, playlists, schedules,
    /// as-run and program logs; media and recordings stay).
    #[arg(long)]
    remove_data: bool,
    /// Render every page to PNG files in this folder and exit.
    #[cfg(feature = "snapshot")]
    #[arg(long)]
    snapshot: Option<PathBuf>,
}

fn main() -> ExitCode {
    platform::attach_console();
    let args = Args::parse();
    #[cfg(feature = "snapshot")]
    if let Some(dir) = &args.snapshot {
        return match snapshot::run(dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("snapshot: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let result = if args.silent { silent(&args) } else { wizard(&args) };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ошибка: {e}");
            if !args.silent {
                rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("LauraBoard").set_description(&e).show();
            }
            ExitCode::FAILURE
        }
    }
}

fn wizard(args: &Args) -> Result<(), String> {
    let mode = if args.uninstall || !payload::present() {
        ui::Mode::Uninstall(Receipt::load())
    } else {
        ui::Mode::Install { manifest: payload::manifest()?, existing: Receipt::load() }
    };
    ui::run(mode)
}

fn silent(args: &Args) -> Result<(), String> {
    let mut last = String::new();
    let mut report = |_: f32, step: &str| {
        if step != last {
            println!("{step}");
            last = step.to_string();
        }
    };
    let warnings = if args.uninstall || !payload::present() {
        let r = Receipt::load().ok_or("LauraBoard не установлен")?;
        let data = if args.remove_data { project::remembered() } else { None };
        uninstall::uninstall(&r, args.forget_settings, data.as_deref(), &mut report)?
    } else {
        let mut opts = Options::new(Receipt::load().as_ref());
        if let Some(dir) = &args.dir {
            opts.dir = dir.clone();
        }
        opts.menu_shortcut &= !args.no_menu;
        opts.desktop_shortcut &= !args.no_desktop;
        opts.add_to_path &= !args.no_path;
        if args.project.is_some() {
            opts.project = args.project.clone();
        }
        if args.no_project {
            opts.project = None;
        }
        opts.demo = args.demo;
        install::install(&opts, &mut report)?.warnings
    };
    for w in warnings {
        eprintln!("предупреждение: {w}");
    }
    Ok(())
}
