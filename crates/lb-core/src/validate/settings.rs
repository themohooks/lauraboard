//! channel.toml sanity checks.

use super::Report;
use crate::planner::PlanMode;
use crate::project::ChannelData;

pub(super) fn check(ch: &ChannelData, r: &mut Report) {
    let c = &ch.config;
    let loc = "channel.toml";
    if c.video.width < 16 || c.video.height < 16 {
        r.error(loc, "video size is too small");
    }
    if !c.video.width.is_multiple_of(2) || !c.video.height.is_multiple_of(2) {
        r.error(loc, "video width and height must be even numbers");
    }
    if !(1.0..=120.0).contains(&c.video.fps) {
        r.error(loc, format!("video fps {} out of range 1..120", c.video.fps));
    }
    if ![8000, 16000, 22050, 32000, 44100, 48000, 96000].contains(&c.audio.sample_rate) {
        r.warn(loc, format!("unusual audio sample rate {}", c.audio.sample_rate));
    }
    if !(1..=8).contains(&c.audio.channels) {
        r.error(loc, "audio channels must be 1..8");
    }
    if c.outputs.iter().all(|o| !o.enabled) && !c.record.enabled {
        r.warn(loc, "no enabled outputs and recording is off — nothing will be broadcast");
    }
    for o in &c.outputs {
        if o.url.trim().is_empty() {
            r.error(loc, format!("output '{}' has an empty url", o.name));
        }
    }
    if c.encoder.gop_seconds <= 0.0 {
        r.error(loc, "encoder.gop_seconds must be positive");
    }
    if c.graphics.fps <= 0.0 || c.graphics.fps > c.video.fps {
        r.warn(loc, "graphics.fps should be between 1 and the video fps");
    }
    if let PlanMode::Sequential { playlist } = &c.planning.mode
        && ch.playlist(playlist).is_none()
    {
        r.error(loc, format!("sequential mode playlist '{playlist}' does not exist"));
    }
    if c.planning.horizon.ms() < 3_600_000 {
        r.warn(loc, "planning.horizon shorter than 1 hour");
    }
}
