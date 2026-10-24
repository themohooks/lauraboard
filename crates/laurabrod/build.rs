//! Windows: application icon and manifest (see `lb-winres`), and a main
//! thread stack big enough for the GUI window in debug builds.

fn main() {
    lb_winres::embed(std::path::Path::new("../lb-gui/assets/app/laurabrod.ico"));
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:16777216");
    }
}
