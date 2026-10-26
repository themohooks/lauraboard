//! One window per server: a second start (shortcut clicked while the window
//! sits in the tray) asks the running GUI to show itself and exits.
//!
//! The running GUI listens on a loopback port written to
//! `<config dir>/lauraboard/gui.port`; the request names the API it would
//! connect to, so a GUI for another server or project still opens.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

fn port_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("lauraboard").join("gui.port"))
}

/// Ask a running GUI for `base_url` to show its window. `true`: it did.
pub fn activate_running(base_url: &str) -> bool {
    let Some(port) = port_file().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| s.trim().parse::<u16>().ok()) else {
        return false;
    };
    let Ok(mut s) = TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, port).into(), Duration::from_millis(300)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    if writeln!(s, "show {base_url}").is_err() {
        return false;
    }
    let mut answer = String::new();
    BufReader::new(s).read_line(&mut answer).is_ok() && answer.trim() == "ok"
}

/// Accept "show" requests for `base_url`; `show` runs on the UI thread.
/// Returns the port to [`release`] on exit.
pub fn listen(base_url: String, show: fn()) -> Option<u16> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let file = port_file()?;
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&file, port.to_string()).ok()?;
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let _ = conn.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            if BufReader::new(&conn).read_line(&mut line).is_err() {
                continue;
            }
            let mine = line.trim().strip_prefix("show ").is_some_and(|u| u == base_url);
            if mine {
                let _ = slint::invoke_from_event_loop(show);
            }
            let _ = (&conn).write_all(if mine { b"ok\n" } else { b"no\n" });
        }
    });
    Some(port)
}

/// Forget the port on exit, unless another GUI has taken the file over.
pub fn release(port: Option<u16>) {
    let (Some(port), Some(f)) = (port, port_file()) else { return };
    if std::fs::read_to_string(&f).ok().and_then(|s| s.trim().parse::<u16>().ok()) == Some(port) {
        let _ = std::fs::remove_file(f);
    }
}
