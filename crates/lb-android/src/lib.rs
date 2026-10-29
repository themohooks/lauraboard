//! Experimental remote operator client. No embedded playout engine.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
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
