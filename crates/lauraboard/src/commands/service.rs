//! `service`: system service integration.
//!
//! * Linux: writes a systemd unit to `/etc/systemd/system/<name>.service`
//!   (or prints it when not root) and enables it.
//! * Windows: registers a service with `sc.exe` whose entry point is the
//!   hidden `service run` command (implemented with `windows-service`).

use anyhow::Result;

use crate::cli::ServiceCmd;
use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, cmd: ServiceCmd) -> Result<()> {
    let root = std::fs::canonicalize(&ctx.root)?;
    let exe = std::env::current_exe()?;
    match cmd {
        ServiceCmd::Install { name } => platform::install(&name, &exe, &root),
        ServiceCmd::Uninstall { name } => platform::uninstall(&name),
        ServiceCmd::Run { name } => platform::service_main(&name, ctx),
    }
}

/// systemd unit text (also documented in docs/install.md).
#[cfg_attr(windows, allow(dead_code))]
pub fn systemd_unit(exe: &std::path::Path, root: &std::path::Path) -> String {
    format!(
        "[Unit]\nDescription=LauraBoard TV playout\nAfter=network-online.target\nWants=network-online.target\n\n\
         [Service]\nType=simple\nExecStart={} --project {} run\nWorkingDirectory={}\nRestart=always\nRestartSec=3\nKillSignal=SIGINT\nTimeoutStopSec=20\n\n\
         [Install]\nWantedBy=multi-user.target\n",
        exe.display(),
        root.display(),
        root.display()
    )
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    use anyhow::Result;

    use crate::ctx::Ctx;
    use crate::exit::{Code, bail};

    pub fn install(name: &str, exe: &Path, root: &Path) -> Result<()> {
        let unit = super::systemd_unit(exe, root);
        let path = format!("/etc/systemd/system/{name}.service");
        match std::fs::write(&path, &unit) {
            Ok(()) => {
                let ok = std::process::Command::new("systemctl").args(["daemon-reload"]).status().is_ok_and(|s| s.success())
                    && std::process::Command::new("systemctl").args(["enable", "--now", name]).status().is_ok_and(|s| s.success());
                if !ok {
                    return Err(bail(Code::Error, "unit written but systemctl failed"));
                }
                println!("service '{name}' installed and started ({path})");
                Ok(())
            }
            Err(e) => {
                eprintln!("cannot write {path}: {e}\nRun with sudo, or install this unit manually:\n");
                println!("{unit}");
                Err(bail(Code::Error, "service not installed"))
            }
        }
    }

    pub fn uninstall(name: &str) -> Result<()> {
        let _ = std::process::Command::new("systemctl").args(["disable", "--now", name]).status();
        let path = format!("/etc/systemd/system/{name}.service");
        std::fs::remove_file(&path).map_err(|e| bail(Code::Error, format!("removing {path}: {e}")))?;
        let _ = std::process::Command::new("systemctl").arg("daemon-reload").status();
        println!("service '{name}' removed");
        Ok(())
    }

    pub fn service_main(_name: &str, ctx: &Ctx) -> Result<()> {
        // systemd runs `lauraboard run` directly; keep this for symmetry
        super::super::run::run(ctx, true)
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::OnceLock;
    use std::time::Duration;

    use anyhow::Result;
    use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::{define_windows_service, service_dispatcher};

    use crate::ctx::Ctx;
    use crate::exit::{Code, bail};

    fn sc(args: &[&str]) -> Result<()> {
        let out = std::process::Command::new("sc.exe").args(args).output()?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
            return Err(bail(Code::Error, format!("sc.exe {}: {}", args.join(" "), msg.trim())));
        }
        Ok(())
    }

    pub fn install(name: &str, exe: &Path, root: &Path) -> Result<()> {
        let bin = format!("\"{}\" --project \"{}\" service run --name {name}", exe.display(), root.display());
        sc(&["create", name, "binPath=", &bin, "start=", "auto", "DisplayName=", "LauraBoard TV playout"])?;
        let _ = sc(&["failure", name, "reset=", "86400", "actions=", "restart/5000/restart/5000/restart/30000"]);
        sc(&["start", name])?;
        println!("service '{name}' installed and started (requires an elevated prompt)");
        Ok(())
    }

    pub fn uninstall(name: &str) -> Result<()> {
        let _ = sc(&["stop", name]);
        sc(&["delete", name])?;
        println!("service '{name}' removed");
        Ok(())
    }

    struct SvcArgs {
        name: String,
        root: std::path::PathBuf,
    }
    static ARGS: OnceLock<SvcArgs> = OnceLock::new();

    define_windows_service!(ffi_service_main, service_entry);

    fn service_entry(_args: Vec<OsString>) {
        let Some(a) = ARGS.get() else { return };
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let handler = move |ev| match ev {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = stop_tx.send(true);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let Ok(h) = service_control_handler::register(&a.name, handler) else { return };
        let status = |state, accept| ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(20),
            process_id: None,
        };
        let _ = h.set_service_status(status(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN));
        let ctx = Ctx { root: a.root.clone(), channel: None, token: None, json: false };
        let _guard = crate::logging::init(&ctx.root, true);
        if let Err(e) = crate::commands::block_on(crate::commands::run::serve(&ctx, true, stop_rx)) {
            tracing::error!("service failed: {e}");
        }
        let _ = h.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()));
    }

    pub fn service_main(name: &str, ctx: &Ctx) -> Result<()> {
        let _ = ARGS.set(SvcArgs { name: name.to_string(), root: ctx.root.clone() });
        service_dispatcher::start(name, ffi_service_main).map_err(|e| bail(Code::Error, format!("not started by the service manager: {e}")))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unit_contains_run_command() {
        let u = super::systemd_unit(std::path::Path::new("/usr/bin/lauraboard"), std::path::Path::new("/srv/tv"));
        assert!(u.contains("ExecStart=/usr/bin/lauraboard --project /srv/tv run"));
        assert!(u.contains("Restart=always"));
    }
}
