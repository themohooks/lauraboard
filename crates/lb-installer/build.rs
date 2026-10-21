//! Compiles the installer UI (Material style, components shared with
//! `lb-gui` through the `@m3` library path) and embeds the payload.
//!
//! The payload is the zip archive named by `LB_PAYLOAD` (made by
//! `cargo dist`). Without it the binary carries nothing and works as the
//! uninstaller only.

use std::collections::HashMap;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let shared = manifest.join("../lb-gui/ui");
    let config = slint_build::CompilerConfiguration::new()
        .with_style("material".into())
        .with_library_paths(HashMap::from([("m3".to_string(), shared)]));
    slint_build::compile_with_config("ui/installer.slint", config).expect("Slint UI failed to compile");

    lb_winres::embed(&manifest.join("../lb-gui/assets/app/laurabrod.ico"));

    println!("cargo:rerun-if-env-changed=LB_PAYLOAD");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payload.zip");
    match std::env::var_os("LB_PAYLOAD") {
        Some(src) => {
            println!("cargo:rerun-if-changed={}", PathBuf::from(&src).display());
            std::fs::copy(&src, &out).unwrap_or_else(|e| panic!("LB_PAYLOAD {}: {e}", PathBuf::from(&src).display()));
        }
        None => std::fs::write(&out, []).expect("write empty payload"),
    }
}
