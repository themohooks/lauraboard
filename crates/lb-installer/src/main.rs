//! `lauraboard-setup` — installer and uninstaller of LauraBoard.
//!
//! * with the application packed inside (`cargo dist`): installs or updates;
//! * without it (the copy shipped as `uninstall`) or with `--uninstall`:
//!   removes what the installation created.
//!
//! The wizard uses the operator GUI's Material 3 components; `--silent`
//! does the same without a window (for scripts and deployment). `--update`
//! is how the application updates itself: it starts the new installer and
//! quits, the installer waits for it, installs with the same choices and
//! opens the new version.
//!
//! An installation for all users needs administrator rights: this program
//! then starts itself once more elevated and lets that copy do the work.

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

use std::ffi::OsString;
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
    /// Install folder (default: per-user programs folder, or Program Files
    /// with --all-users).
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Install for every user of the computer (needs administrator rights).
    #[arg(long)]
    all_users: bool,
    /// Update the installed copy with the same choices: progress only, then
    /// the new version opens.
    #[arg(long)]
    update: bool,
    /// Wait for this process (the application being updated) to exit.
    #[arg(long, value_name = "PID")]
    wait: Option<u32>,
    /// Wizard: start installing at once with the options given (used by the
    /// copy started with administrator rights).
    #[arg(long, hide = true)]
    resume: bool,
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

/// How long an update waits for the application to quit.
pub const WAIT_FOR_APP: std::time::Duration = std::time::Duration::from_secs(60);

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
    let result = if needs_elevation(&args) && !platform::elevated() {
        elevated(&args)
    } else if args.silent {
        silent(&args)
    } else {
        wizard(&args)
    };
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

fn removing(args: &Args) -> bool {
    args.uninstall || !payload::present()
}

/// The wizard asks for rights itself once "for all users" is chosen; this
/// is for work that starts right away.
fn needs_elevation(args: &Args) -> bool {
    if removing(args) {
        return Receipt::load().is_some_and(|r| r.all_users);
    }
    (args.silent || args.update || args.resume) && options(args).all_users
}

/// Run the same command elevated. Silent mode waits for it (scripts get its
/// exit code); a failed update brings the old version back.
fn elevated(args: &Args) -> Result<(), String> {
    let own: Vec<OsString> = std::env::args_os().skip(1).collect();
    match platform::elevate(&own, args.silent) {
        Ok(Some(0) | None) => Ok(()),
        Ok(Some(code)) => Err(format!("установка завершилась с кодом {code}")),
        Err(e) => {
            if args.update
                && let Some(r) = Receipt::load()
                && let Ok(m) = payload::manifest()
            {
                let _ = platform::launch(&r.dir.join(m.binary));
            }
            Err(e)
        }
    }
}

/// Defaults (or the installed copy's choices) plus the command line.
fn options(args: &Args) -> Options {
    let mut opts = Options::new(Receipt::load().as_ref());
    if args.all_users && !opts.all_users {
        opts.all_users = true;
        opts.dir = options::default_dir(true);
    }
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
    opts
}

fn wizard(args: &Args) -> Result<(), String> {
    let mode = if removing(args) {
        ui::Mode::Uninstall(Receipt::load())
    } else {
        let run = if args.update {
            ui::Run::Update(args.wait)
        } else if args.resume {
            ui::Run::Now
        } else {
            ui::Run::Wizard
        };
        ui::Mode::Install { manifest: payload::manifest()?, existing: Receipt::load(), opts: options(args), run }
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
    let warnings = if removing(args) {
        let r = Receipt::load().ok_or("LauraBoard не установлен")?;
        let data = if args.remove_data { project::remembered() } else { None };
        uninstall::uninstall(&r, args.forget_settings, data.as_deref(), &mut report)?
    } else {
        if let Some(pid) = args.wait {
            platform::wait_exit(pid, WAIT_FOR_APP);
        }
        let outcome = install::install(&options(args), &mut report)?;
        if args.update {
            let _ = platform::launch(&outcome.binary);
        }
        outcome.warnings
    };
    for w in warnings {
        eprintln!("предупреждение: {w}");
    }
    Ok(())
}
