//! First project: created by the installed application itself
//! (`laurabrod -p <dir> init [--demo]`) and remembered as the project its
//! shortcuts open (`<config dir>/laurabrod/desktop.json`, same format the
//! application writes).

use std::path::Path;
use std::process::{Command, Stdio};

use crate::receipt::{config_dir, write_json};

/// Create the project if it does not exist yet, then make it the default.
pub fn prepare(binary: &Path, project: &Path, demo: bool) -> Result<(), String> {
    if !project.join("laurabrod.toml").exists() {
        std::fs::create_dir_all(project).map_err(|e| format!("{}: {e}", project.display()))?;
        let mut cmd = Command::new(binary);
        cmd.arg("-p").arg(project).arg("init").stdin(Stdio::null());
        if demo {
            cmd.arg("--demo");
        }
        crate::platform::no_window(&mut cmd);
        let out = cmd.output().map_err(|e| format!("{}: {e}", binary.display()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("проект не создан: {}", err.trim()));
        }
    }
    remember(project)
}

fn remember(project: &Path) -> Result<(), String> {
    let path = config_dir().ok_or("нет каталога настроек")?.join("desktop.json");
    write_json(&path, &serde_json::json!({ "project": project }))
}
