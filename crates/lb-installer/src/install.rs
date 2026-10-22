//! Install / update: unpack, integrate with the OS, write the receipt,
//! prepare the first project. Runs on a worker thread; `report` gets the
//! overall fraction (negative = indeterminate) and the current step.

use std::collections::HashSet;
use std::path::Path;

use crate::options::Options;
use crate::payload;
use crate::platform::{self, Registration};
use crate::receipt::Receipt;

pub type Report<'a> = &'a mut dyn FnMut(f32, &str);

/// Done; `warnings` lists optional steps that failed.
pub struct Outcome {
    pub binary: std::path::PathBuf,
    pub warnings: Vec<String>,
}

pub fn install(opts: &Options, report: Report) -> Result<Outcome, String> {
    let manifest = payload::manifest()?;
    let previous = Receipt::load();
    let dir = std::path::absolute(&opts.dir).map_err(|e| format!("{}: {e}", opts.dir.display()))?;

    report(0.0, "Подготовка папки");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if let Some(prev) = &previous {
        crate::uninstall::remove_integration(prev);
        remove_old_copies(prev);
    }

    report(0.02, "Копирование файлов");
    let total = manifest.size.max(1) as f32;
    let files = payload::extract(&dir, &mut |done| report(0.02 + 0.68 * (done as f32 / total).min(1.0), "Копирование файлов"))?;
    if let Some(prev) = &previous {
        drop_stale_files(prev, &dir, &files);
    }

    let binary = dir.join(&manifest.binary);
    let icon = dir.join(&manifest.icon);
    let mut receipt = Receipt { version: manifest.version.clone(), dir: dir.clone(), files, ..Receipt::default() };
    let mut warnings = Vec::new();
    let mut note = |r: Result<Vec<std::path::PathBuf>, String>, extras: &mut Vec<_>| match r {
        Ok(paths) => extras.extend(paths),
        Err(e) => warnings.push(e),
    };

    report(0.72, "Ярлыки");
    if opts.menu_shortcut {
        note(platform::menu_shortcut(&binary, &icon), &mut receipt.extras);
    }
    if opts.desktop_shortcut {
        note(platform::desktop_shortcut(&binary, &icon), &mut receipt.extras);
    }
    if opts.add_to_path {
        let change = platform::add_to_path(&dir, &binary).map(|c| {
            receipt.path_entry = c.entry;
            c.extras
        });
        note(change, &mut receipt.extras);
    }

    report(0.78, "Регистрация в системе");
    let reg = Registration {
        version: &manifest.version,
        dir: &dir,
        binary: &binary,
        uninstaller: &dir.join(&manifest.uninstaller),
        size: manifest.size,
    };
    receipt.registered = platform::register(&reg).unwrap_or_else(|e| {
        warnings.push(e);
        false
    });
    // saved before the project step: whatever happens next, it can be uninstalled
    receipt.save()?;

    if let Some(project) = &opts.project {
        report(
            -1.0,
            if opts.demo {
                "Создание демо-канала (генерация роликов)"
            } else {
                "Создание проекта"
            },
        );
        if let Err(e) = crate::project::prepare(&binary, project, opts.demo) {
            warnings.push(e);
        }
    }
    report(1.0, "Готово");
    Ok(Outcome { binary, warnings })
}

/// `*.old` files left by an update that replaced busy executables.
fn remove_old_copies(prev: &Receipt) {
    for f in &prev.files {
        let _ = std::fs::remove_file(payload::old_name(&prev.dir.join(f)));
    }
}

/// Files of the previous version that the new one no longer ships
/// (or all of them when the new version goes into another folder).
fn drop_stale_files(prev: &Receipt, dir: &Path, files: &[String]) {
    let same_dir = prev.dir == dir;
    let keep: HashSet<&str> = if same_dir { files.iter().map(String::as_str).collect() } else { HashSet::new() };
    let stale: Vec<String> = prev.files.iter().filter(|f| !keep.contains(f.as_str())).cloned().collect();
    crate::uninstall::remove_files(&prev.dir, &stale);
    if !same_dir {
        let _ = std::fs::remove_dir(&prev.dir);
    }
}
