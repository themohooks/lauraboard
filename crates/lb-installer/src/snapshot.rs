//! Developer tool: render every wizard page with sample data to PNG
//! (light and dark), without a display.
//!
//! `cargo run -p lb-installer --features snapshot -- --snapshot <out dir>`

use std::path::Path;
use std::rc::Rc;

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, ModelRc, Rgb8Pixel, SharedString, VecModel};

use crate::ui::{InstallerWindow, Setup, SetupMode, SetupPage, Theme, ThemeMode};

struct Headless(Rc<MinimalSoftwareWindow>);

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

pub fn run(out: &Path) -> Result<(), String> {
    let (width, height) = (880u32, 640u32);
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let ui = InstallerWindow::new().map_err(|e| e.to_string())?;
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    window.set_size(slint::PhysicalSize::new(width, height));
    ui.global::<Theme>().set_animations(false);
    ui.show().map_err(|e| e.to_string())?;

    let s = ui.global::<Setup>();
    s.set_version("0.1.0".into());
    s.set_size_text("≈ 41 МБ".into());
    s.set_ffmpeg_hint(crate::ffmpeg::install_hint().into());
    s.set_install_dir(r"C:\Users\operator\AppData\Local\Programs\Laurabrod".into());
    s.set_dir_note("Папка будет создана".into());
    s.set_project_dir(r"C:\Users\operator\Documents\Laurabrod".into());
    s.set_installed_dir(r"C:\Users\operator\AppData\Local\Programs\Laurabrod".into());

    type Prep = fn(&Setup);
    let shots: [(&str, Prep); 10] = [
        ("welcome", |s| {
            s.set_mode(SetupMode::Install);
            s.set_page(SetupPage::Welcome);
        }),
        ("welcome-update", |s| {
            s.set_installed_version("0.0.9".into());
            s.set_ffmpeg_found(true);
            s.set_ffmpeg_version("7.1-full_build-www.gyan.dev".into());
        }),
        ("location", |s| {
            s.set_installed_version("".into());
            s.set_page(SetupPage::Location);
        }),
        ("location-error", |s| {
            s.set_install_dir("Laurabrod".into());
            s.set_dir_error("Нужен полный путь".into());
        }),
        ("options", |s| {
            s.set_dir_error("".into());
            s.set_page(SetupPage::Options);
        }),
        ("progress", |s| {
            s.set_page(SetupPage::Progress);
            s.set_progress(0.46);
            s.set_step("Копирование файлов".into());
        }),
        ("done", |s| {
            s.set_page(SetupPage::Done);
            s.set_success(true);
            s.set_warnings(ModelRc::new(VecModel::from(vec![SharedString::from("Проект не создан: FFmpeg не найден")])));
        }),
        ("failed", |s| {
            s.set_success(false);
            s.set_warnings(ModelRc::default());
            s.set_error(r"C:\Program Files\Laurabrod: Отказано в доступе. (os error 5)".into());
        }),
        ("uninstall", |s| {
            s.set_mode(SetupMode::Uninstall);
            s.set_installed_version("0.1.0".into());
            s.set_page(SetupPage::Welcome);
        }),
        ("missing", |s| s.set_mode(SetupMode::Missing)),
    ];
    for (name, prep) in shots {
        prep(&s);
        for (theme, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
            ui.global::<Theme>().set_mode(mode);
            slint::platform::update_timers_and_animations();
            let mut buf = vec![Rgb8Pixel::default(); (width * height) as usize];
            window.request_redraw();
            window.draw_if_needed(|r| {
                r.render(&mut buf, width as usize);
            });
            let raw: Vec<u8> = buf.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
            let img = image::RgbImage::from_raw(width, height, raw).ok_or("bad buffer")?;
            let path = out.join(format!("setup-{name}-{theme}.png"));
            img.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("{}", path.display());
        }
    }
    Ok(())
}
