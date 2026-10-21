//! `backup` / `restore`: project configuration archive (media excluded).

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use zip::write::SimpleFileOptions;

use crate::ctx::Ctx;
use crate::exit::{Code, bail};

/// Files included in backups, relative to the project root.
fn config_files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from(lb_core::project::APP_FILE)];
    let ch_root = root.join(lb_core::project::CHANNELS_DIR);
    let Ok(rd) = std::fs::read_dir(&ch_root) else { return out };
    for e in rd.flatten().filter(|e| e.path().is_dir()) {
        let rel = PathBuf::from(lb_core::project::CHANNELS_DIR).join(e.file_name());
        for f in ["channel.toml", "library.json", "schedule.toml", "rules.toml", "graphics.toml", "state/state.json"] {
            if root.join(&rel).join(f).exists() {
                out.push(rel.join(f));
            }
        }
        if let Ok(pl) = std::fs::read_dir(e.path().join("playlists")) {
            for p in pl.flatten().filter(|p| p.path().extension().is_some_and(|x| x == "json")) {
                out.push(rel.join("playlists").join(p.file_name()));
            }
        }
    }
    out
}

pub fn backup(ctx: &Ctx, out: Option<PathBuf>) -> Result<()> {
    ctx.project()?; // must be a valid project
    let out = out.unwrap_or_else(|| ctx.root.join(format!("backup-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S"))));
    let mut zip = zip::ZipWriter::new(File::create(&out).with_context(|| format!("creating {}", out.display()))?);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let files = config_files(&ctx.root);
    for rel in &files {
        let name = rel.to_string_lossy().replace('\\', "/");
        zip.start_file(name, opts)?;
        let mut buf = Vec::new();
        File::open(ctx.root.join(rel))?.read_to_end(&mut buf)?;
        zip.write_all(&buf)?;
    }
    zip.finish()?;
    println!("backup written: {} ({} files)", out.display(), files.len());
    Ok(())
}

pub fn restore(ctx: &Ctx, file: &Path, force: bool) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(file).with_context(|| format!("opening {}", file.display()))?)?;
    let mut written = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let Some(rel) = f.enclosed_name() else { continue };
        let target = ctx.root.join(rel);
        if target.exists() && !force {
            return Err(bail(Code::Invalid, format!("{} exists (use --force to overwrite)", target.display())));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        std::fs::write(&target, buf)?;
        written += 1;
    }
    println!("restored {written} files into {}", ctx.root.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_and_restore_roundtrip() {
        let src = tempfile::tempdir().unwrap();
        lb_core::project::Project::init(src.path(), "main").unwrap();
        let ctx = Ctx { root: src.path().to_path_buf(), channel: None, token: None, json: false };
        let zipf = src.path().join("b.zip");
        backup(&ctx, Some(zipf.clone())).unwrap();
        let dst = tempfile::tempdir().unwrap();
        let ctx2 = Ctx { root: dst.path().to_path_buf(), channel: None, token: None, json: false };
        restore(&ctx2, &zipf, false).unwrap();
        assert!(lb_core::project::Project::load(dst.path()).is_ok());
        assert!(restore(&ctx2, &zipf, false).is_err());
    }
}
