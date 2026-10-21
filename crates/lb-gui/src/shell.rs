//! Material 3 interface built with Slint (`ui/app.slint`).

use slint::ComponentHandle;

use crate::GuiOptions;
use crate::prefs::UiPrefs;

slint::include_modules!();

/// Open the main window (blocks until it is closed).
pub fn run(_opts: GuiOptions) -> Result<(), String> {
    let prefs = UiPrefs::load();
    let window = AppWindow::new().map_err(|e| e.to_string())?;
    window.run().map_err(|e| e.to_string())?;
    prefs.save();
    Ok(())
}
