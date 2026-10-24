//! Compiles the Slint UI with the Material style.

fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("material".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint UI failed to compile");
    // the generated window builds on the main thread's stack: 1 MB (the
    // MSVC default) is too little for it in debug builds
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-examples=/STACK:16777216");
    }
}
