//! Video editor: [`EditSpec`] → ffmpeg filter graph and command lines for
//! rendering a new file and for one preview frame.
//!
//! Region positions are written as expressions of the picture size
//! (`iw*0.25`), so they stay right whatever the decoder delivers; only
//! `delogo` needs whole pixels and therefore the source size.

use lb_core::Ms;
use lb_core::media::edit::{EditSpec, Region, RegionMode};

use super::color::ffmpeg_color;
use super::{base_args, secs, sv};

/// What the builder needs to know about the source file.
#[derive(Debug, Clone, Copy, Default)]
pub struct Source {
    pub width: u32,
    pub height: u32,
    pub duration_ms: Ms,
    pub has_audio: bool,
}

/// `[0:v:0]` → `[v]` graph. With `at = Some(t)` (a preview frame at source
/// time `t`) only the regions visible at `t` are drawn, without time
/// switches, and crop and fades are left out so the frame matches the
/// coordinates of the source picture.
pub fn video_graph(e: &EditSpec, src: &Source, at: Option<Ms>) -> String {
    let (start, end) = e.range(src.duration_ms);
    let mut g: Vec<String> = Vec::new();
    let mut cur = "[0:v:0]".to_string();
    let mut n = 0;
    for r in &e.regions {
        let window = match at {
            Some(t) if !visible(r, t) => continue,
            Some(_) => None,
            None => window(r, start, end),
        };
        let next = format!("[lbr{n}]");
        g.push(format!("{cur}{}{next}", region_filter(r, n, src, window.as_deref())));
        cur = next;
        n += 1;
    }
    let mut tail: Vec<String> = Vec::new();
    if at.is_none() {
        let c = e.crop.clamped();
        if !c.is_empty() {
            let (l, t, r, b) = (c.left / 100.0, c.top / 100.0, c.right / 100.0, c.bottom / 100.0);
            tail.push(format!("crop=trunc(iw*{:.4}/2)*2:trunc(ih*{:.4}/2)*2:trunc(iw*{l:.4}):trunc(ih*{t:.4})", 1.0 - l - r, 1.0 - t - b));
        }
        let dur = end - start;
        if e.fade_in_ms > 0 {
            tail.push(format!("fade=t=in:st=0:d={}", secs(e.fade_in_ms.min(dur))));
        }
        if e.fade_out_ms > 0 && dur > e.fade_out_ms {
            tail.push(format!("fade=t=out:st={}:d={}", secs(dur - e.fade_out_ms), secs(e.fade_out_ms)));
        }
    }
    tail.push("format=yuv420p".into());
    g.push(format!("{cur}{}[v]", tail.join(",")));
    g.join(";")
}

/// Audio filters (`-af`), empty when nothing changes. `None` = no sound.
pub fn audio_chain(e: &EditSpec, src: &Source) -> Option<String> {
    if e.mute || !src.has_audio {
        return None;
    }
    let (start, end) = e.range(src.duration_ms);
    let audio_start = e.audio_start_ms.unwrap_or(start).clamp(start, end);
    let audio_end = e.audio_end_ms.unwrap_or(end).clamp(audio_start, end);
    let dur = audio_end - audio_start;
    if dur <= 0 {
        return Some("volume=0".into());
    }
    let mut f: Vec<String> = Vec::new();
    if e.audio_start_ms.is_some() || e.audio_end_ms.is_some() {
        f.push(format!("atrim=start={}:end={},asetpts=PTS-STARTPTS", secs(audio_start - start), secs(audio_end - start)));
    }
    if e.volume_db.abs() > 0.01 {
        f.push(format!("volume={:.2}dB", e.volume_db.clamp(-40.0, 20.0)));
    }
    let fade_in = e.audio_fade_in_ms.unwrap_or(e.fade_in_ms).max(0);
    let fade_out = e.audio_fade_out_ms.unwrap_or(e.fade_out_ms).max(0);
    if fade_in > 0 {
        f.push(format!("afade=t=in:st=0:d={}", secs(fade_in.min(dur))));
    }
    if fade_out > 0 && dur > fade_out {
        f.push(format!("afade=t=out:st={}:d={}", secs(dur - fade_out), secs(fade_out)));
    }
    if audio_start > start {
        f.push(format!("adelay={}:all=1", audio_start - start));
    }
    if e.audio_start_ms.is_some() || e.audio_end_ms.is_some() {
        f.push(format!("apad=whole_dur={}", secs(end - start)));
    }
    Some(f.join(","))
}

/// Render the edited file (H.264 / AAC in MP4). Progress goes to stdout
/// (`-progress pipe:1`), see [`crate::progress::ProgressParser`].
pub fn render_args(input: &str, output: &str, e: &EditSpec, src: &Source) -> Vec<String> {
    let (start, end) = e.range(src.duration_ms);
    let mut a = base_args("error");
    a.extend(sv(&["-progress", "pipe:1", "-nostats", "-y"]));
    if start > 0 {
        a.extend(["-ss".into(), secs(start)]);
    }
    a.extend(["-i".into(), input.to_string()]);
    a.extend(["-t".into(), secs(end - start)]);
    a.extend(["-filter_complex".into(), video_graph(e, src, None), "-map".into(), "[v]".into()]);
    match audio_chain(e, src) {
        Some(af) => {
            a.extend(sv(&["-map", "0:a:0"]));
            if !af.is_empty() {
                a.extend(["-af".into(), af]);
            }
            a.extend(sv(&["-c:a", "aac", "-b:a", "192k"]));
        }
        None => a.push("-an".into()),
    }
    a.extend(sv(&["-sn", "-dn", "-map_metadata", "-1", "-c:v", "libx264", "-preset", "medium", "-crf", "18"]));
    a.extend(sv(&["-pix_fmt", "yuv420p", "-movflags", "+faststart"]));
    a.push(output.to_string());
    a
}

/// One JPEG frame at source time `at_ms` with the hidden areas drawn,
/// `width` pixels wide, on stdout.
pub fn frame_args(input: &str, e: &EditSpec, src: &Source, at_ms: Ms, width: u32) -> Vec<String> {
    let graph = video_graph(e, src, Some(at_ms));
    let graph = format!("{},scale={width}:-2[v]", graph.trim_end_matches("[v]"));
    let mut a = base_args("error");
    if at_ms > 0 {
        a.extend(["-ss".into(), secs(at_ms)]);
    }
    a.extend(["-i".into(), input.to_string(), "-filter_complex".into(), graph]);
    a.extend(sv(&["-map", "[v]", "-frames:v", "1", "-an", "-q:v", "3", "-f", "image2", "-c:v", "mjpeg", "pipe:1"]));
    a
}

fn visible(r: &Region, t: Ms) -> bool {
    r.from_ms.is_none_or(|f| t >= f) && r.to_ms.is_none_or(|e| t <= e)
}

/// `enable` expression in output time (the render starts at `start`).
fn window(r: &Region, start: Ms, end: Ms) -> Option<String> {
    if r.from_ms.is_none() && r.to_ms.is_none() {
        return None;
    }
    let from = (r.from_ms.unwrap_or(0) - start).max(0);
    let to = (r.to_ms.unwrap_or(end) - start).max(0);
    Some(format!("between(t,{},{})", secs(from), secs(to)))
}

/// Only `#RGB` / `#RRGGBB` colours reach the graph (no filter syntax).
fn safe_color(c: &str) -> String {
    let hex = c.trim().trim_start_matches('#');
    if matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        ffmpeg_color(&format!("#{hex}"))
    } else {
        "0x000000".into()
    }
}

fn region_filter(r: &Region, n: usize, src: &Source, window: Option<&str>) -> String {
    let (x, y, w, h) = r.clamped();
    let s = r.strength.clamp(0.0, 1.0);
    let enable = window.map(|w| format!(":enable='{w}'")).unwrap_or_default();
    // the same rectangle for crop (iw/ih) and overlay (W/H)
    let rect = |iw: &str, ih: &str| {
        format!("trunc({iw}*{w:.4}/2)*2:trunc({ih}*{h:.4}/2)*2:trunc({iw}*{x:.4}):trunc({ih}*{y:.4})", w = w.max(0.005))
    };
    let at = format!("x=trunc(W*{x:.4}):y=trunc(H*{y:.4})");
    match r.mode {
        RegionMode::Fill => format!(
            "drawbox=x=trunc(iw*{x:.4}):y=trunc(ih*{y:.4}):w=trunc(iw*{w:.4}):h=trunc(ih*{h:.4}):color={}@1:t=fill{enable}",
            safe_color(&r.color)
        ),
        RegionMode::Delogo if src.width > 8 && src.height > 8 => {
            // delogo wants whole pixels strictly inside the picture
            let (fw, fh) = (src.width as f32, src.height as f32);
            let px = ((x * fw).round() as u32).clamp(1, src.width - 4);
            let py = ((y * fh).round() as u32).clamp(1, src.height - 4);
            let pw = ((w * fw).round() as u32).clamp(2, src.width - 1 - px);
            let ph = ((h * fh).round() as u32).clamp(2, src.height - 1 - py);
            format!("delogo=x={px}:y={py}:w={pw}:h={ph}{enable}")
        }
        RegionMode::Pixelate => {
            let k = (4.0 + s * 36.0).round() as u32;
            format!(
                "split=2[lbm{n}][lbc{n}];[lbc{n}]crop={},scale='max(1,trunc(iw/{k}))':'max(1,trunc(ih/{k}))',\
                 scale=iw*{k}:ih*{k}:flags=neighbor[lbe{n}];[lbm{n}][lbe{n}]overlay={at}{enable}",
                rect("iw", "ih"),
            )
        }
        // blur, and delogo when the size is unknown
        _ => {
            let sigma = 4.0 + s * 46.0;
            format!(
                "split=2[lbm{n}][lbc{n}];[lbc{n}]crop={},gblur=sigma={sigma:.1}:steps=3[lbe{n}];[lbm{n}][lbe{n}]overlay={at}{enable}",
                rect("iw", "ih"),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_audio_trim_preserves_sync_and_video_fades() {
        let e = EditSpec {
            start_ms: 10_000,
            end_ms: Some(40_000),
            audio_start_ms: Some(12_000),
            audio_end_ms: Some(35_000),
            fade_in_ms: 1_000,
            audio_fade_in_ms: Some(0),
            audio_fade_out_ms: Some(500),
            ..Default::default()
        };
        let audio = audio_chain(&e, &src()).unwrap();
        assert!(audio.contains("atrim=start=2.000:end=25.000,asetpts=PTS-STARTPTS"), "{audio}");
        assert!(!audio.contains("afade=t=in"));
        assert!(audio.contains("afade=t=out:st=22.500:d=0.500"));
        assert!(audio.contains("adelay=2000:all=1,apad=whole_dur=30.000"));
        assert!(video_graph(&e, &src(), None).contains("fade=t=in:st=0:d=1.000"));
        let silent = EditSpec { audio_start_ms: Some(45_000), ..e };
        assert_eq!(audio_chain(&silent, &src()).as_deref(), Some("volume=0"));
    }
    use lb_core::media::display::Crop;

    fn src() -> Source {
        Source { width: 1920, height: 1080, duration_ms: 60_000, has_audio: true }
    }

    fn region(mode: RegionMode) -> Region {
        Region { x: 0.8, y: 0.05, w: 0.15, h: 0.1, mode, strength: 0.5, color: "#102030".into(), from_ms: None, to_ms: Some(30_000) }
    }

    #[test]
    fn plain_render_is_a_reencode() {
        let a = render_args("in.mp4", "out.mp4", &EditSpec::default(), &src());
        assert!(!a.contains(&"-ss".to_string()));
        let g = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert_eq!(g, "[0:v:0]format=yuv420p[v]");
        assert!(a.contains(&"0:a:0".to_string()));
        assert!(!a.contains(&"-af".to_string()));
        assert_eq!(a.last().unwrap(), "out.mp4");
    }

    #[test]
    fn trim_regions_crop_fades() {
        let e = EditSpec {
            start_ms: 10_000,
            end_ms: Some(40_000),
            regions: vec![region(RegionMode::Blur), region(RegionMode::Fill)],
            crop: Crop { bottom: 10.0, ..Default::default() },
            fade_in_ms: 500,
            fade_out_ms: 1_000,
            volume_db: -3.0,
            ..Default::default()
        };
        let a = render_args("in.mp4", "out.mp4", &e, &src());
        let pos = |s: &str| a.iter().position(|x| x == s).unwrap();
        assert_eq!(a[pos("-ss") + 1], "10.000");
        assert_eq!(a[pos("-t") + 1], "30.000");
        let g = &a[pos("-filter_complex") + 1];
        assert!(g.starts_with("[0:v:0]split=2[lbm0][lbc0];[lbc0]crop="), "{g}");
        assert!(g.contains("gblur=sigma=27.0"));
        assert!(g.contains("overlay=x=trunc(W*0.8000):y=trunc(H*0.0500):enable='between(t,0.000,20.000)'[lbr0]"));
        assert!(g.contains("[lbr0]drawbox=") && g.contains("color=0x102030@1:t=fill"));
        assert!(g.contains("[lbr1]crop=trunc(iw*1.0000/2)*2:trunc(ih*0.9000/2)*2"));
        assert!(g.contains("fade=t=out:st=29.000:d=1.000"));
        assert!(g.ends_with("format=yuv420p[v]"));
        assert_eq!(a[pos("-af") + 1], "volume=-3.00dB,afade=t=in:st=0:d=0.500,afade=t=out:st=29.000:d=1.000");
    }

    #[test]
    fn preview_frame_draws_only_visible_regions() {
        let e =
            EditSpec { regions: vec![region(RegionMode::Pixelate)], crop: Crop { top: 5.0, ..Default::default() }, ..Default::default() };
        let a = frame_args("in.mp4", &e, &src(), 45_000, 640);
        let g = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert_eq!(g, "[0:v:0]format=yuv420p,scale=640:-2[v]");
        let a = frame_args("in.mp4", &e, &src(), 5_000, 640);
        let g = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert!(g.contains("flags=neighbor") && !g.contains("enable=") && !g.contains("crop=trunc(iw*1.0000"), "{g}");
    }

    #[test]
    fn delogo_uses_pixels_inside_the_frame() {
        let mut r = region(RegionMode::Delogo);
        (r.x, r.w) = (0.9, 0.5);
        let e = EditSpec { regions: vec![r], ..Default::default() };
        let g = video_graph(&e, &src(), None);
        assert!(g.contains("delogo=x=1728:y=54:w=191:h=108:enable="), "{g}");
        // unknown size: falls back to a blur
        let g = video_graph(&e, &Source::default(), None);
        assert!(g.contains("gblur"));
    }

    #[test]
    fn mute_and_colours() {
        let e = EditSpec { mute: true, ..Default::default() };
        assert!(render_args("i", "o", &e, &src()).contains(&"-an".to_string()));
        assert_eq!(safe_color("#abc"), "0xaabbcc");
        assert_eq!(safe_color("red:enable=1"), "0x000000");
    }
}
