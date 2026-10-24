//! `cargo xtask backgrounds`: render the ready-made backgrounds once into
//! `crates/lb-engine/assets/backgrounds/` (committed; the engine embeds
//! them, so a new channel gets them at once, without FFmpeg work).
//!
//! * `stills/<stem>.jpg` — full-frame pictures, 1920×1080
//! * `loops/<stem>.mp4` — seamless 20 s video loops, 1920×1080, 25 fps
//! * `plates/<stem>.png` — plate / shape textures
//!
//! `cargo xtask backgrounds --force` renders existing files again.

use std::path::{Path, PathBuf};
use std::process::Command;

use lb_gfx::backdrops::{plates, stills};
use lb_media::args::loops::{self, LOOPS};

const SIZE: (u32, u32) = (1920, 1080);
const FPS: f64 = 25.0;
const JPEG_QUALITY: u8 = 90;

pub fn out_dir() -> PathBuf {
    crate::root().join("crates/lb-engine/assets/backgrounds")
}

pub fn run(args: &[String]) -> Result<(), String> {
    let force = args.iter().any(|a| a == "--force");
    let out = out_dir();
    for sub in ["stills", "loops", "plates"] {
        std::fs::create_dir_all(out.join(sub)).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    let mut total = 0u64;
    for b in stills::STILLS {
        let path = out.join("stills").join(format!("{}.jpg", b.stem));
        if force || !path.exists() {
            let p = stills::paint(b.stem, SIZE.0, SIZE.1).ok_or_else(|| format!("unknown still {}", b.stem))?;
            let rgb: Vec<u8> = p.data().chunks_exact(4).flat_map(|px| [px[0], px[1], px[2]]).collect();
            let img = image::RgbImage::from_raw(p.width(), p.height(), rgb).ok_or("still buffer")?;
            let mut f = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, JPEG_QUALITY).encode_image(&img).map_err(|e| e.to_string())?;
        }
        total += report(&path);
    }
    for b in plates::PLATES {
        let path = out.join("plates").join(format!("{}.png", b.stem));
        if force || !path.exists() {
            let p = plates::paint(b.stem).ok_or_else(|| format!("unknown plate {}", b.stem))?;
            p.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        total += report(&path);
    }
    for l in LOOPS {
        let path = out.join("loops").join(format!("{}.mp4", l.stem));
        if force || !path.exists() {
            let args = loops::loop_args(l, SIZE.0, SIZE.1, FPS, &path.to_string_lossy());
            let st = Command::new("ffmpeg").args(&args).status().map_err(|e| format!("ffmpeg: {e} (нужен ffmpeg в PATH)"))?;
            if !st.success() {
                return Err(format!("ffmpeg failed for {}", l.stem));
            }
        }
        total += report(&path);
    }
    println!("{} — {:.1} MB", out.display(), total as f64 / 1_048_576.0);
    Ok(())
}

fn report(path: &Path) -> u64 {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    println!("  {:>7} KB  {}", len / 1024, path.file_name().unwrap_or_default().to_string_lossy());
    len
}
