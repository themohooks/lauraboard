//! Wizard window: fills the `Setup` global, runs the work on a thread and
//! reports progress back through the Slint event loop.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::options::{self, Options};
use crate::payload::Manifest;
use crate::receipt::Receipt;
use crate::{ffmpeg, install, platform, uninstall};

slint::include_modules!();

pub enum Mode {
    Install { manifest: Manifest, existing: Option<Receipt> },
    Uninstall(Option<Receipt>),
}

pub fn run(mode: Mode) -> Result<(), String> {
    let window = InstallerWindow::new().map_err(|e| e.to_string())?;
    let s = window.global::<Setup>();
    s.set_windows(cfg!(windows));
    s.set_ffmpeg_hint(ffmpeg::install_hint().into());
    // what "Готово" launches
    let launch_target: Arc<Mutex<Option<PathBuf>>> = Arc::default();
    let busy = Arc::new(AtomicBool::new(false));

    match &mode {
        Mode::Install { manifest, existing } => {
            let opts = Options::new(existing.as_ref());
            s.set_mode(SetupMode::Install);
            s.set_version(manifest.version.as_str().into());
            s.set_installed_version(existing.as_ref().map(|r| r.version.as_str()).unwrap_or("").into());
            s.set_size_text(format!("≈ {} МБ", manifest.size.div_ceil(1024 * 1024)).into());
            s.set_install_dir(opts.dir.display().to_string().into());
            s.set_menu_shortcut(opts.menu_shortcut);
            s.set_desktop_shortcut(opts.desktop_shortcut);
            s.set_add_to_path(opts.add_to_path);
            s.set_create_project(opts.project.is_some());
            s.set_project_dir(opts.project.unwrap_or_else(options::default_project).display().to_string().into());
            check_dir(&s, &manifest.binary);
            check_project(&s);
            check_ffmpeg(&window);
        }
        Mode::Uninstall(Some(r)) => {
            s.set_mode(SetupMode::Uninstall);
            s.set_installed_version(r.version.as_str().into());
            s.set_installed_dir(r.dir.display().to_string().into());
            if let Some(p) = crate::project::remembered() {
                s.set_data_dir(p.display().to_string().into());
            }
        }
        Mode::Uninstall(None) => s.set_mode(SetupMode::Missing),
    }

    let binary = match &mode {
        Mode::Install { manifest, .. } => manifest.binary.clone(),
        Mode::Uninstall(_) => String::new(),
    };
    s.on_dir_edited({
        let w = window.as_weak();
        let binary = binary.clone();
        move |_| {
            if let Some(w) = w.upgrade() {
                check_dir(&w.global::<Setup>(), &binary);
            }
        }
    });
    s.on_browse_dir({
        let w = window.as_weak();
        move || {
            let Some(w) = w.upgrade() else { return };
            let s = w.global::<Setup>();
            let current = PathBuf::from(s.get_install_dir().as_str());
            if let Some(dir) = pick_folder(current.parent()) {
                s.set_install_dir(options::with_app_folder(dir).display().to_string().into());
                check_dir(&s, &binary);
            }
        }
    });
    s.on_project_edited({
        let w = window.as_weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                check_project(&w.global::<Setup>());
            }
        }
    });
    s.on_browse_project({
        let w = window.as_weak();
        move || {
            let Some(w) = w.upgrade() else { return };
            let s = w.global::<Setup>();
            let current = PathBuf::from(s.get_project_dir().as_str());
            if let Some(dir) = pick_folder(current.parent()) {
                s.set_project_dir(dir.display().to_string().into());
                check_project(&s);
            }
        }
    });
    s.on_check_ffmpeg({
        let w = window.as_weak();
        move || {
            if let Some(w) = w.upgrade() {
                check_ffmpeg(&w);
            }
        }
    });

    let receipt = match &mode {
        Mode::Uninstall(r) => r.clone(),
        Mode::Install { .. } => None,
    };
    s.on_start({
        let w = window.as_weak();
        let busy = busy.clone();
        let launch_target = launch_target.clone();
        move || {
            let Some(win) = w.upgrade() else { return };
            let s = win.global::<Setup>();
            busy.store(true, Ordering::Relaxed);
            s.set_progress(0.0);
            s.set_indeterminate(false);
            s.set_step("".into());
            s.set_page(SetupPage::Progress);
            let w = w.clone();
            let busy = busy.clone();
            let launch_target = launch_target.clone();
            let job = match s.get_mode() {
                SetupMode::Install => Job::Install(read_options(&s)),
                _ => Job::Uninstall(
                    receipt.clone().unwrap_or_default(),
                    s.get_forget_settings(),
                    (s.get_remove_data() && !s.get_data_dir().is_empty()).then(|| PathBuf::from(s.get_data_dir().as_str())),
                ),
            };
            let w2 = w.clone();
            std::thread::spawn(move || {
                let mut report = |fraction: f32, step: &str| {
                    let step = SharedString::from(step);
                    let w = w2.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = w.upgrade() {
                            let s = w.global::<Setup>();
                            s.set_indeterminate(fraction < 0.0);
                            if fraction >= 0.0 {
                                s.set_progress(fraction);
                            }
                            s.set_step(step);
                        }
                    });
                };
                let result = match job {
                    Job::Install(opts) => install::install(&opts, &mut report).map(|o| (Some(o.binary), o.warnings)),
                    Job::Uninstall(r, forget, data) => uninstall::uninstall(&r, forget, data.as_deref(), &mut report).map(|w| (None, w)),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    busy.store(false, Ordering::Relaxed);
                    let Some(w) = w.upgrade() else { return };
                    let s = w.global::<Setup>();
                    match result {
                        Ok((binary, warnings)) => {
                            *launch_target.lock().unwrap() = binary;
                            s.set_success(true);
                            s.set_warnings(strings(warnings));
                        }
                        Err(e) => {
                            s.set_success(false);
                            s.set_error(e.into());
                            s.set_warnings(strings(Vec::new()));
                        }
                    }
                    s.set_page(SetupPage::Done);
                });
            });
        }
    });
    s.on_finish({
        let w = window.as_weak();
        move || {
            if let Some(w) = w.upgrade()
                && w.global::<Setup>().get_launch()
                && let Some(binary) = launch_target.lock().unwrap().as_ref()
            {
                let _ = platform::launch(binary);
            }
            let _ = slint::quit_event_loop();
        }
    });
    s.on_close(|| {
        let _ = slint::quit_event_loop();
    });
    window.window().on_close_requested(move || {
        if busy.load(Ordering::Relaxed) { slint::CloseRequestResponse::KeepWindowShown } else { slint::CloseRequestResponse::HideWindow }
    });

    window.run().map_err(|e| e.to_string())
}

enum Job {
    Install(Options),
    // receipt, forget settings, project whose data goes too
    Uninstall(Receipt, bool, Option<PathBuf>),
}

fn read_options(s: &Setup) -> Options {
    Options {
        dir: PathBuf::from(s.get_install_dir().trim()),
        menu_shortcut: s.get_menu_shortcut(),
        desktop_shortcut: s.get_desktop_shortcut(),
        add_to_path: s.get_add_to_path(),
        project: s.get_create_project().then(|| PathBuf::from(s.get_project_dir().trim())),
        demo: s.get_demo() && s.get_ffmpeg_found() && !s.get_project_exists(),
    }
}

fn strings(v: Vec<String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(v.into_iter().map(SharedString::from).collect::<Vec<_>>()))
}

fn check_dir(s: &Setup, binary: &str) {
    match options::check_dir(&s.get_install_dir(), binary) {
        Ok(note) => {
            s.set_dir_error("".into());
            s.set_dir_note(note.into());
        }
        Err(e) => {
            s.set_dir_error(e.into());
            s.set_dir_note("".into());
        }
    }
}

fn check_project(s: &Setup) {
    let exists = Path::new(s.get_project_dir().trim()).join("laurabrod.toml").exists();
    s.set_project_exists(exists);
    if exists {
        s.set_demo(false);
    }
}

fn check_ffmpeg(window: &InstallerWindow) {
    window.global::<Setup>().set_ffmpeg_checking(true);
    let w = window.as_weak();
    std::thread::spawn(move || {
        let found = ffmpeg::detect();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                let s = w.global::<Setup>();
                s.set_ffmpeg_checking(false);
                s.set_ffmpeg_found(found.is_some());
                s.set_ffmpeg_version(found.unwrap_or_default().into());
            }
        });
    });
}

fn pick_folder(start: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(dir) = start.filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_folder()
}
