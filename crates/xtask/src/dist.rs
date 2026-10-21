//! `cargo dist`: one command from sources to an installer.
//!
//! 1. release build of `laurabrod` and of `laurabrod-setup` without payload
//!    (that copy is shipped as the uninstaller);
//! 2. stage the files and pack them with `manifest.json` into a zip;
//! 3. rebuild `laurabrod-setup` with the zip embedded (`LB_PAYLOAD`);
//! 4. `target/dist/laurabrod-<version>-<os>-<arch>-setup[.exe]`.
//!
//! Builds for the host OS: run it on Windows for the Windows installer and
//! on Linux for the Linux one.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use zip::write::SimpleFileOptions;

const EXE: &str = std::env::consts::EXE_SUFFIX;

pub fn run() -> Result<(), String> {
    let root = crate::root();
    let version = app_version(&root)?;
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let release = root.join("target").join("release");
    let dist = root.join("target").join("dist");
    let stage = dist.join("stage");

    step(1, &format!("сборка Laurabrod {version} ({os}/{arch})"));
    cargo(&root, &["build", "--release", "-p", "laurabrod", "-p", "lb-installer"], None)?;

    step(2, "упаковка");
    let _ = std::fs::remove_dir_all(&stage);
    let binary = format!("laurabrod{EXE}");
    let uninstaller = format!("uninstall{EXE}");
    let assets = crate::app_assets();
    let gui_assets = root.join("crates/lb-gui/assets");
    let mut files: Vec<(PathBuf, String)> = vec![
        (release.join(&binary), binary.clone()),
        (release.join(format!("laurabrod-setup{EXE}")), uninstaller.clone()),
        (assets.join("laurabrod.svg"), "laurabrod.svg".into()),
        (root.join("README.md"), "README.md".into()),
        (gui_assets.join("fonts/OFL-Roboto.txt"), "licenses/OFL-Roboto.txt".into()),
        (gui_assets.join("fonts/OFL-RobotoMono.txt"), "licenses/OFL-RobotoMono.txt".into()),
        (gui_assets.join("icons/LICENSE"), "licenses/MaterialSymbols-Apache-2.0.txt".into()),
    ];
    for doc in std::fs::read_dir(root.join("docs")).map_err(|e| format!("docs: {e}"))?.flatten() {
        let path = doc.path();
        if path.extension().is_some_and(|x| x == "md") {
            files.push((path.clone(), format!("docs/{}", doc.file_name().to_string_lossy())));
        }
    }
    for (src, rel) in &files {
        let dst = stage.join(rel);
        std::fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::copy(src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
    }
    let size: u64 = files.iter().map(|(_, rel)| std::fs::metadata(stage.join(rel)).map(|m| m.len()).unwrap_or(0)).sum();
    let manifest = serde_json::json!({
        "version": version, "os": os, "arch": arch,
        "binary": binary, "icon": "laurabrod.svg", "uninstaller": uninstaller, "size": size,
    });
    let payload = dist.join(format!("payload-{os}-{arch}.zip"));
    pack(&stage, &files, &manifest, &payload, &[binary.as_str(), uninstaller.as_str()])?;
    println!("    {} файлов, {} → {}", files.len(), mb(size), mb(len(&payload)));

    step(3, "сборка установщика с приложением внутри");
    cargo(&root, &["build", "--release", "-p", "lb-installer"], Some(&payload))?;
    let out = dist.join(format!("laurabrod-{version}-{os}-{arch}-setup{EXE}"));
    std::fs::copy(release.join(format!("laurabrod-setup{EXE}")), &out).map_err(|e| format!("{}: {e}", out.display()))?;

    step(4, "готово");
    println!("    {} ({})", out.display(), mb(len(&out)));
    Ok(())
}

fn step(n: u32, text: &str) {
    println!("[{n}/4] {text}");
}

fn len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn mb(bytes: u64) -> String {
    format!("{:.1} МБ", bytes as f64 / (1024.0 * 1024.0))
}

/// Version of the `laurabrod` package.
fn app_version(root: &Path) -> Result<String, String> {
    let manifest = root.join("crates/laurabrod/Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let doc: toml::Table = text.parse().map_err(|e| format!("{}: {e}", manifest.display()))?;
    doc.get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "нет package.version".into())
}

fn cargo(root: &Path, args: &[&str], payload: Option<&Path>) -> Result<(), String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.args(args).current_dir(root);
    match payload {
        Some(p) => cmd.env("LB_PAYLOAD", p),
        None => cmd.env_remove("LB_PAYLOAD"),
    };
    let status = cmd.status().map_err(|e| format!("cargo: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("cargo {} завершился с ошибкой", args.join(" "))) }
}

fn pack(stage: &Path, files: &[(PathBuf, String)], manifest: &serde_json::Value, out: &Path, executables: &[&str]) -> Result<(), String> {
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let base = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(Some(9));
    let err = |e: zip::result::ZipError| e.to_string();
    zip.start_file("manifest.json", base.unix_permissions(0o644)).map_err(err)?;
    zip.write_all(serde_json::to_string_pretty(manifest).unwrap().as_bytes()).map_err(|e| e.to_string())?;
    for (_, rel) in files {
        let mode = if executables.contains(&rel.as_str()) { 0o755 } else { 0o644 };
        zip.start_file(rel.as_str(), base.unix_permissions(mode).large_file(false)).map_err(err)?;
        let mut src = std::fs::File::open(stage.join(rel)).map_err(|e| e.to_string())?;
        std::io::copy(&mut src, &mut zip).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(err)?;
    Ok(())
}
