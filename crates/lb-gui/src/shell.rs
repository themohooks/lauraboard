//! Material 3 interface built with Slint (`ui/app.slint`).

use slint::ComponentHandle;

use crate::GuiOptions;
use crate::bridge;
use crate::prefs::UiPrefs;

slint::include_modules!();

/// Open the main window (blocks until it is closed).
pub fn run(opts: GuiOptions) -> Result<(), String> {
    let prefs = UiPrefs::load();
    let window = AppWindow::new().map_err(|e| e.to_string())?;
    bridge::start(&window, &opts, prefs);
    let result = window.run().map_err(|e| e.to_string());
    bridge::stop();
    result
}
