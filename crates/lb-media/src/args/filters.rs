//! Filter chains for normalising any source to the channel format.

use lb_core::config::{AudioFormat, ScaleMode, VideoFormat};
use lb_core::media::MediaDisplay;
use lb_core::media::display::Rotate;

use super::color::ffmpeg_color;
use super::{fps_str, secs};

/// Video normalisation: per-item clean-up and picture settings, size,
/// aspect handling, SAR, fps, pixel format, optional fades (relative to the
/// item start, duration `dur_ms`).
///
/// The result goes between `[in]` and `[out]` labels of a filter graph; the
/// blurred-background mode splits the stream internally.
pub fn video_chain(v: &VideoFormat, d: &MediaDisplay, fade_in_ms: i64, fade_out_ms: i64, dur_ms: i64) -> String {
    let (w, h) = (v.width, v.height);
    let pre = source_filters(d).join(",");
    let pre = if pre.is_empty() { String::new() } else { format!("{pre},") };
    let mut f: Vec<String> = Vec::new();
    let scaled = match d.scale.unwrap_or(v.scale) {
        ScaleMode::Fit => {
            f.push(format!("scale={w}:{h}:force_original_aspect_ratio=decrease"));
            f.push(format!("pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color={}", ffmpeg_color(&v.background)));
            None
        }
        ScaleMode::Fill => {
            f.push(format!("scale={w}:{h}:force_original_aspect_ratio=increase"));
            f.push(format!("crop={w}:{h}"));
            None
        }
        ScaleMode::Stretch => {
            f.push(format!("scale={w}:{h}"));
            None
        }
        ScaleMode::Blur => Some(format!(
            "split=2[lbbg][lbfg];\
             [lbbg]scale={sw}:{sh}:force_original_aspect_ratio=increase,crop={sw}:{sh},boxblur=10:2,scale={w}:{h},setsar=1[lbbgs];\
             [lbfg]scale={w}:{h}:force_original_aspect_ratio=decrease,setsar=1[lbfgs];\
             [lbbgs][lbfgs]overlay=(W-w)/2:(H-h)/2",
            // blur at quarter size: cheaper and smoother
            sw = (w / 4).max(2) & !1,
            sh = (h / 4).max(2) & !1,
        )),
    };
    f.push("setsar=1".into());
    // identical colour metadata for every item (no filter reconfiguration
    // at item boundaries in the encoder)
    f.push("setparams=range=tv:colorspace=bt709:color_primaries=bt709:color_trc=bt709".into());
    f.push(format!("fps={}:start_time=0", fps_str(v.fps)));
    f.push("format=yuv420p".into());
    if fade_in_ms > 0 {
        f.push(format!("fade=t=in:st=0:d={}", secs(fade_in_ms)));
    }
    if fade_out_ms > 0 && dur_ms > fade_out_ms {
        f.push(format!("fade=t=out:st={}:d={}", secs(dur_ms - fade_out_ms), secs(fade_out_ms)));
    }
    match scaled {
        Some(graph) => format!("{pre}{graph},{}", f.join(",")),
        None => format!("{pre}{}", f.join(",")),
    }
}

/// Filters applied to the source picture before scaling.
fn source_filters(d: &MediaDisplay) -> Vec<String> {
    let mut f: Vec<String> = Vec::new();
    if d.deinterlace {
        f.push("bwdif=mode=send_frame:deint=interlaced".into());
    }
    if let Some(r) = d.aspect_ratio() {
        // square pixels at the requested shape: later scaling keeps it
        f.push(format!("scale=trunc(ih*{r:.4}/2)*2:ih"));
        f.push("setsar=1".into());
    } else {
        // honour the file's own pixel aspect (anamorphic video)
        f.push("scale=trunc(iw*sar/2)*2:ih".into());
        f.push("setsar=1".into());
    }
    let c = d.crop.clamped();
    if !c.is_empty() {
        let (l, t, r, b) = (c.left / 100.0, c.top / 100.0, c.right / 100.0, c.bottom / 100.0);
        f.push(format!(
            "crop=trunc(iw*{:.4}/2)*2:trunc(ih*{:.4}/2)*2:trunc(iw*{l:.4}):trunc(ih*{t:.4})",
            1.0 - l - r,
            1.0 - t - b
        ));
    }
    match d.rotate {
        Rotate::None => {}
        Rotate::Cw90 => f.push("transpose=clock".into()),
        Rotate::R180 => f.push("hflip,vflip".into()),
        Rotate::Ccw90 => f.push("transpose=cclock".into()),
    }
    if d.flip_h {
        f.push("hflip".into());
    }
    if d.flip_v {
        f.push("vflip".into());
    }
    if d.denoise > 0.01 {
        let s = d.denoise.clamp(0.0, 10.0);
        f.push(format!("hqdn3d={s:.1}:{:.1}:{:.1}:{:.1}", s * 0.75, s * 1.5, s * 1.1));
    }
    if d.has_eq() {
        f.push(format!(
            "eq=brightness={:.3}:contrast={:.3}:saturation={:.3}:gamma={:.3}",
            d.brightness.clamp(-1.0, 1.0),
            d.contrast.clamp(0.0, 2.0),
            d.saturation.clamp(0.0, 3.0),
            d.gamma.clamp(0.1, 3.0)
        ));
    }
    if d.sharpen > 0.01 {
        f.push(format!("unsharp=5:5:{:.2}:5:5:0", d.sharpen.clamp(0.0, 2.0)));
    }
    f
}

/// Audio normalisation: resample, layout, gain, loudness, fades, padding
/// with silence so audio always covers the item duration.
pub fn audio_chain(a: &AudioFormat, volume_db: f32, fade_in_ms: i64, fade_out_ms: i64, dur_ms: i64) -> String {
    let layout = match a.channels {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        n => format!("{n}c"),
    };
    // Preserve the source A/V timeline, including delayed audio starts and
    // timestamp gaps, before sending samples to the continuous encoder.
    let mut f: Vec<String> = vec![format!("aresample={}:async=1000:first_pts=0", a.sample_rate)];
    if a.normalize {
        f.push("loudnorm=I=-23:TP=-2:LRA=11".into());
        f.push(format!("aresample={}", a.sample_rate));
    }
    f.push(format!("aformat=sample_fmts=s16:channel_layouts={layout}"));
    if volume_db.abs() > 0.01 {
        f.push(format!("volume={volume_db:.2}dB"));
    }
    if fade_in_ms > 0 {
        f.push(format!("afade=t=in:st=0:d={}", secs(fade_in_ms)));
    }
    if fade_out_ms > 0 && dur_ms > fade_out_ms {
        f.push(format!("afade=t=out:st={}:d={}", secs(dur_ms - fade_out_ms), secs(fade_out_ms)));
    }
    f.push("apad".into());
    f.join(",")
}

/// `anullsrc` source matching the channel audio format.
pub fn silence_src(a: &AudioFormat) -> String {
    let layout = if a.channels == 1 { "mono" } else { "stereo" };
    format!("anullsrc=r={}:cl={layout}", a.sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_chain_with_fades() {
        let v = VideoFormat::default();
        let c = video_chain(&v, &MediaDisplay::default(), 500, 1000, 10_000);
        assert!(c.contains("setsar=1,scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720"));
        assert!(c.contains("fps=25"));
        assert!(c.contains("fade=t=in:st=0:d=0.500"));
        assert!(c.contains("fade=t=out:st=9.000:d=1.000"));
    }

    #[test]
    fn item_display_settings() {
        let v = VideoFormat::default();
        let d = MediaDisplay {
            scale: Some(ScaleMode::Stretch),
            aspect: Some("4:3".into()),
            deinterlace: true,
            saturation: 0.0,
            crop: lb_core::media::display::Crop { bottom: 5.0, ..Default::default() },
            ..Default::default()
        };
        let c = video_chain(&v, &d, 0, 0, 10_000);
        assert!(c.starts_with("bwdif="), "{c}");
        assert!(c.contains("scale=trunc(ih*1.3333/2)*2:ih"));
        assert!(c.contains("crop=trunc(iw*1.0000/2)*2:trunc(ih*0.9500/2)*2"));
        assert!(c.contains("eq=brightness=0.000:contrast=1.000:saturation=0.000"));
        assert!(c.contains("scale=1280:720,setsar=1"));
        assert!(!c.contains("pad="));
    }

    #[test]
    fn blurred_bars_split_the_stream() {
        let v = VideoFormat::default();
        let d = MediaDisplay { scale: Some(ScaleMode::Blur), ..Default::default() };
        let c = video_chain(&v, &d, 0, 0, 10_000);
        assert!(c.contains("split=2[lbbg][lbfg];"));
        assert!(c.contains("boxblur"));
        assert!(c.contains("[lbbgs][lbfgs]overlay=(W-w)/2:(H-h)/2,setsar=1"));
    }

    #[test]
    fn audio_chain_pads() {
        let a = AudioFormat::default();
        let c = audio_chain(&a, -3.0, 0, 0, 1000);
        assert!(c.contains("aformat=sample_fmts=s16:channel_layouts=stereo"));
        assert!(c.contains("volume=-3.00dB"));
        assert!(c.ends_with("apad"));
    }
}
