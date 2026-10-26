//! `cargo dist`: one command from sources to an installer.
//!
//! 1. release build of `lauraboard` and of `lauraboard-setup` without payload
//!    (that copy is shipped as the uninstaller);
//! 2. stage the files and pack them with `manifest.json` into a zip;
//! 3. rebuild `lauraboard-setup` with the zip embedded (`LB_PAYLOAD`);
//! 4. `target/dist/lauraboard-<version>-<os>-<arch>-setup[.exe]`
//!    (`$CARGO_TARGET_DIR/dist` when that is set).
//!
//! Builds for the host OS. `cargo dist --linux` builds the Linux installer
//! from any OS inside a Docker container (`rust:1-bookworm` with the GUI
//! libraries) → `target/linux/dist/`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use zip::write::SimpleFileOptions;

const EXE: &str = std::env::consts::EXE_SUFFIX;

/// Image for `--linux`; the cargo registry is cached in a named volume.
const LINUX_IMAGE: &str = "rust:1-bookworm";
const LINUX_DEPS: &str = "libfontconfig-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libgl1-mesa-dev pkg-config";

pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None => build(),
        Some("--linux") => linux_in_docker(),
        Some(other) => Err(format!("неизвестный параметр {other} (есть --linux)")),
    }
}

fn linux_in_docker() -> Result<(), String> {
    let root = crate::root();
    println!("сборка Linux-установщика в Docker ({LINUX_IMAGE})");
    let script = format!("apt-get update -qq && apt-get install -y -qq {LINUX_DEPS} >/dev/null && cargo dist");
    let status = Command::new("docker")
        .args(["run", "--rm", "-v"])
        .arg(format!("{}:/src", root.display()))
        .args(["-v", "lauraboard-cargo-registry:/usr/local/cargo/registry", "-w", "/src", "-e", "CARGO_TARGET_DIR=/src/target/linux"])
        .args([LINUX_IMAGE, "bash", "-c", &script])
        .env("MSYS_NO_PATHCONV", "1")
        .status()
        .map_err(|e| format!("docker: {e} (установлен ли Docker?)"))?;
    if !status.success() {
        return Err("сборка в Docker завершилась с ошибкой".into());
    }
    println!("    {}", root.join("target").join("linux").join("dist").display());
    Ok(())
}

fn build() -> Result<(), String> {
    let root = crate::root();
    let version = app_version(&root)?;
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    let release = target.join("release");
    let dist = target.join("dist");
    let stage = dist.join("stage");

    step(1, &format!("сборка LauraBoard {version} ({os}/{arch})"));
    cargo(&root, &["build", "--locked", "--release", "-p", "lauraboard", "-p", "lb-installer"], None)?;

    step(2, "упаковка");
    let _ = std::fs::remove_dir_all(&stage);
    let binary = format!("lauraboard{EXE}");
    let uninstaller = format!("uninstall{EXE}");
    let assets = crate::app_assets();
    let gui_assets = root.join("crates/lb-gui/assets");
    let mut files: Vec<(PathBuf, String)> = vec![
        (release.join(&binary), binary.clone()),
        (release.join(format!("lauraboard-setup{EXE}")), uninstaller.clone()),
        (assets.join("lauraboard.svg"), "lauraboard.svg".into()),
        (root.join("README.md"), "README.md".into()),
        (gui_assets.join("fonts/OFL-Roboto.txt"), "licenses/OFL-Roboto.txt".into()),
        (gui_assets.join("fonts/OFL-RobotoMono.txt"), "licenses/OFL-RobotoMono.txt".into()),
        (gui_assets.join("icons/LICENSE"), "licenses/MaterialSymbols-Apache-2.0.txt".into()),
    ];
    collect_files(&root, &root.join("docs"), &mut files)?;
    if let Some(bundle) = std::env::var_os("LB_FFMPEG_DIR") {
        let bundle = PathBuf::from(bundle);
        for tool in ["ffmpeg", "ffprobe"] {
            let path = bundle.join(format!("{tool}{EXE}"));
            if !path.is_file() {
                return Err(format!("нет {}", path.display()));
            }
        }
        collect_files(&bundle, &bundle, &mut files)?;
    }
    files.sort_by(|a, b| a.1.cmp(&b.1));
    for (src, rel) in &files {
        let dst = stage.join(rel);
        std::fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::copy(src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
    }
    let size: u64 = files.iter().map(|(_, rel)| std::fs::metadata(stage.join(rel)).map(|m| m.len()).unwrap_or(0)).sum();
    let manifest = serde_json::json!({
        "version": version, "os": os, "arch": arch,
        "binary": binary, "icon": "lauraboard.svg", "uninstaller": uninstaller, "size": size,
    });
    let payload = dist.join(format!("payload-{os}-{arch}.zip"));
    pack(&stage, &files, &manifest, &payload, &[binary.as_str(), uninstaller.as_str(), "ffmpeg", "ffprobe", "ffmpeg.exe", "ffprobe.exe"])?;
    println!("    {} файлов, {} → {}", files.len(), mb(size), mb(len(&payload)));

    step(3, "сборка установщика с приложением внутри");
    cargo(&root, &["build", "--locked", "--release", "-p", "lb-installer"], Some(&payload))?;
    let out = dist.join(format!("lauraboard-{version}-{os}-{arch}-setup{EXE}"));
    std::fs::copy(release.join(format!("lauraboard-setup{EXE}")), &out).map_err(|e| format!("{}: {e}", out.display()))?;

    step(4, "готово");
    println!("    {} ({})", out.display(), mb(len(&out)));
    Ok(())
}

/// Keep linked guides and screenshots in offline documentation too.
fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, String)>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            if entry.file_name() != "__pycache__" {
                collect_files(root, &path, files)?;
            }
        } else if kind.is_file() {
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
            files.push((path, rel));
        }
    }
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

/// Version of the `lauraboard` package.
fn app_version(root: &Path) -> Result<String, String> {
    let manifest = root.join("crates/lauraboard/Cargo.toml");
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

#[cfg(test)]
mod tests {
    #[test]
    fn release_documentation_includes_linked_guides_and_screenshots() {
        let root = crate::root();
        let mut files = Vec::new();
        super::collect_files(&root, &root.join("docs"), &mut files).unwrap();
        for linked in ["docs/guides/first-broadcast.md", "docs/screenshots/interface/clock-properties-light.png"] {
            assert!(files.iter().any(|(source, name)| name == linked && source.is_file()), "missing offline document: {linked}");
        }
        assert!(files.iter().all(|(_, name)| !name.contains('\\')));
    }
}
