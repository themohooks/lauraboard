//! One native window: connection form first, then the existing operator UI.
use crate::{bridge, prefs::UiPrefs, shell::AppWindow};
use slint::ComponentHandle;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA: OnceLock<PathBuf> = OnceLock::new();
pub(crate) fn data_dir() -> &'static Path {
    DATA.get().expect("Android data directory initialized before UI").as_path()
}

pub fn run(data: PathBuf) -> Result<(), String> {
    // NativeActivity may be recreated in the same process after destruction.
    DATA.get_or_init(|| data);
    let window = AppWindow::new().map_err(|e| e.to_string())?;
    window.set_mobile(true);
    window.set_connection_visible(true);
    window.set_connection_url(std::fs::read_to_string(data_dir().join("server.txt")).unwrap_or_default().into());
    let weak = window.as_weak();
    window.on_connect_server(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_connection_busy() {
            return;
        }
        let opts = match crate::connection::options(&ui.get_connection_url(), &ui.get_connection_token()) {
            Ok(opts) => opts,
            Err(e) => {
                ui.set_connection_error(e.into());
                return;
            }
        };
        ui.set_connection_busy(true);
        ui.set_connection_error("Подключение…".into());
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            // Authenticate and validate the API before creating the bridge workers.
            let result = crate::net::api::Api::new(&opts.base_url, opts.token.clone()).get::<Vec<serde_json::Value>>("/channels");
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                ui.set_connection_busy(false);
                match result {
                    Err(e) => ui.set_connection_error(e.into()),
                    Ok(_) => {
                        if let Err(e) = std::fs::write(data_dir().join("server.txt"), &opts.base_url) {
                            ui.set_connection_error(format!("Не удалось сохранить адрес: {e}").into());
                            return;
                        }
                        ui.set_connection_token("".into());
                        let mut prefs = UiPrefs::load();
                        prefs.close_to_tray = false;
                        prefs.notify.enabled = false;
                        prefs.updates.auto = false;
                        bridge::start(&ui, &opts, prefs);
                        ui.set_connection_visible(false);
                    }
                }
            });
        });
    });
    let result = window.run().map_err(|e| e.to_string());
    bridge::stop();
    result
}
