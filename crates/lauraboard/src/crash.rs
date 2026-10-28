//! Keep crash reporting outside the process that owns the GUI and engine.
use std::io::Write;
use std::path::Path;

pub const LOG_ENV: &str = "LAURABOARD_CRASH_LOG";

pub fn supervise(root: &Path, desktop: bool) -> i32 {
    if desktop {
        crate::desktop::detach_console();
    }
    let result = (|| -> anyhow::Result<_> {
        let app = lb_core::project::files::read_toml_or_default::<lb_core::config::AppConfig>(&root.join(lb_core::project::APP_FILE))?;
        let dir = lb_core::project::paths::resolve(root, &app.log.dir);
        std::fs::create_dir_all(&dir)?;
        let path =
            std::path::absolute(dir.join(format!("session-{}-{}.log", chrono::Local::now().format("%Y%m%d-%H%M%S"), std::process::id())))?;
        std::fs::write(&path, format!("LauraBoard {}\nНачало сеанса: {}\n\n", env!("CARGO_PKG_VERSION"), chrono::Local::now()))?;
        let mut cmd = std::process::Command::new(std::env::current_exe()?);
        cmd.args(std::env::args_os().skip(1)).env(LOG_ENV, &path);
        // A desktop launch must not flash a second console window.
        #[cfg(windows)]
        if desktop {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let stderr = std::fs::OpenOptions::new().append(true).open(&path)?;
        let stdout = stderr.try_clone()?;
        // After `detach_console` the inherited stdin is a dead console
        // handle and Windows refuses to start the child (os error 6).
        let stdin = if desktop { std::process::Stdio::null() } else { std::process::Stdio::inherit() };
        let status = cmd.stdin(stdin).stdout(stdout).stderr(stderr).status()?;
        let code = status.code().unwrap_or(1);
        if !status.success() && code != crate::exit::Code::Cancelled as i32 {
            let mut file = std::fs::OpenOptions::new().append(true).open(&path)?;
            writeln!(file, "\nКритическая ошибка: процесс неожиданно завершился ({status}).")?;
            let log = String::from_utf8_lossy(&std::fs::read(&path)?).into_owned();
            lb_gui::show_crash_report(&log, &path.display().to_string()).map_err(anyhow::Error::msg)?;
        }
        Ok(code)
    })();
    match result {
        Ok(code) => code,
        Err(e) => {
            crate::report(desktop, &format!("Не удалось запустить приложение или открыть журнал: {e:#}"));
            1
        }
    }
}

pub fn install_hook() {
    let Some(path) = std::env::var_os(LOG_ENV) else { return };
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Synchronous write survives panic=abort and crashes on worker threads.
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&path) {
            let _ = writeln!(file, "\nКритическая ошибка: {info}\n{}", std::backtrace::Backtrace::force_capture());
            let _ = file.sync_data();
        }
        previous(info);
        // A panicking worker must not leave the GUI running in a broken state.
        std::process::exit(101);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_panic_preserves_report_before_exit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.log");
        std::fs::write(&path, "previous log entry\n").unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash::tests::panic_child", "--nocapture"])
            .env(LOG_ENV, &path)
            .env("LAURABOARD_TEST_CRASH", "1")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(101));
        let report = std::fs::read_to_string(path).unwrap();
        assert!(report.starts_with("previous log entry\n"));
        assert!(report.contains("Критическая ошибка"));
        assert!(report.contains("crash-report-test"));
        assert!(report.contains("crash.rs"));
    }

    #[test]
    fn panic_child() {
        if std::env::var_os("LAURABOARD_TEST_CRASH").is_none() {
            return;
        }
        install_hook();
        std::thread::spawn(|| panic!("crash-report-test")).join().unwrap();
    }
}
