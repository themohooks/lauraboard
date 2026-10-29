//! Experimental remote operator client. No embedded playout engine.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    // The generated debug UI needs more stack than android-activity's
    // default Rust thread provides. Keep the same OS thread / Android looper.
    stacker::grow(16 * 1024 * 1024, move || run(app));
}

#[cfg(target_os = "android")]
fn run(app: slint::android::AndroidApp) {
    let Some(data) = app.internal_data_path() else {
        eprintln!("Android internal data directory is unavailable");
        return;
    };
    if let Err(e) = slint::android::init(app) {
        eprintln!("Android platform: {e}");
        return;
    }
    if let Err(e) = lb_gui::run_android(data) {
        eprintln!("LauraBoard: {e}");
    }
}
