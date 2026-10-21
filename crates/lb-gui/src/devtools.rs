//! Developer tools: headless rendering of the Slint UI for visual review.

use std::path::Path;
use std::rc::Rc;

use slint::ComponentHandle;
use slint::Rgb8Pixel;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};

use crate::shell::{AppWindow, Theme, ThemeMode};

struct Headless(Rc<MinimalSoftwareWindow>);

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

/// Render `section` in light and dark themes to `<out>/<section>-{light,dark}.png`.
/// `gallery:<page>` renders one gallery block.
pub fn snapshot(out: &Path, section: &str, width: u32, height: u32, scale: f32) -> Result<(), String> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let ui = AppWindow::new().map_err(|e| e.to_string())?;
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
    window.set_size(slint::PhysicalSize::new(width, height));
    let (name, page) = section.split_once(':').unwrap_or((section, ""));
    ui.set_section(name.into());
    ui.set_gallery_page(page.into());
    // No time passes between frames here, so animated properties would keep stale values.
    ui.global::<Theme>().set_animations(false);
    ui.show().map_err(|e| e.to_string())?;
    for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
        ui.global::<Theme>().set_mode(mode);
        let mut buf = vec![Rgb8Pixel::default(); (width * height) as usize];
        window.request_redraw();
        window.draw_if_needed(|r| {
            r.render(&mut buf, width as usize);
        });
        let raw: Vec<u8> = buf.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        let img = image::RgbImage::from_raw(width, height, raw).ok_or("bad buffer")?;
        let path = out.join(format!("{}-{name}.png", section.replace(':', "-")));
        img.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}
