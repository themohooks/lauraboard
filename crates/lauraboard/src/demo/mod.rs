//! Demo channel: generated test media, playlists, an hourly news grid,
//! jingles / ads / promos rules and on-screen graphics.
//!
//! * [`media`] — test clips generated with FFmpeg (lavfi)
//! * [`config`] — playlists, schedule, rules, graphics, outputs
//! * [`logo`] — channel logo PNG rendered with `lb-gfx`

mod config;
mod logo;
mod media;

use std::path::Path;

use anyhow::Result;

use lb_core::project::ChannelData;

pub fn populate(root: &Path, ch: &mut ChannelData) -> Result<()> {
    let ffmpeg = "ffmpeg";
    let media_dir = root.join("media");
    std::fs::create_dir_all(&media_dir)?;
    logo::write(&media_dir.join("logo.png"))?;
    let generated = media::generate(ffmpeg, &media_dir);
    if !generated {
        eprintln!("warning: FFmpeg not available — demo media not generated; the configuration references media/*.mp4");
    }
    config::apply(root, ch, generated)?;
    Ok(())
}
