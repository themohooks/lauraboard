//! Scene: background (colour, still image or a looped video) with music
//! tracks played one after another, in the intermediate format.

use lb_core::config::{AudioFormat, VideoFormat};
use lb_core::media::MediaDisplay;

use super::color::ffmpeg_color;
use super::filters::{audio_chain, silence_src, video_chain};
use super::{base_args, fps_str, frame_align_ms, intermediate_codec_args, secs, sv};

#[derive(Debug, Clone, PartialEq)]
pub enum SceneVideo {
    /// `#RRGGBB`.
    Color(String),
    /// Resolved path of a still image.
    Image(String),
    /// Resolved path of a video and its length (for the loop position).
    Video { input: String, length_ms: i64 },
}

/// A piece of music: resolved path, where to start, how long. An empty
/// `input` is silence (a pause between tracks, the delay before them).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneTrack {
    pub input: String,
    pub seek_ms: i64,
    pub play_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneJob {
    pub video: SceneVideo,
    /// Picture settings of the background item.
    pub display: MediaDisplay,
    pub tracks: Vec<SceneTrack>,
    pub music_volume_db: f32,
    pub music_fade_in_ms: i64,
    pub music_fade_out_ms: i64,
    /// The music ends this long into the airing (set number of plays).
    pub music_end_ms: Option<i64>,
    /// Where in the scene the airing starts (after a restart / rewind).
    pub offset_ms: i64,
    pub duration_ms: i64,
    /// Gain and transition fades of the item itself.
    pub volume_db: f32,
    pub fade_in_ms: i64,
    pub fade_out_ms: i64,
}

fn layout(a: &AudioFormat) -> String {
    match a.channels {
        1 => "mono".into(),
        2 => "stereo".into(),
        n => format!("{n}c"),
    }
}

pub fn scene_args(job: &SceneJob, v: &VideoFormat, a: &AudioFormat, loglevel: &str) -> Vec<String> {
    let mut args = base_args(loglevel);
    let rt = ["-re", "-readrate_initial_burst", "0"];
    match &job.video {
        SceneVideo::Color(c) => {
            let src = format!("color=c={}:s={}x{}:r={}", ffmpeg_color(c), v.width, v.height, fps_str(v.fps));
            args.extend(sv(&rt));
            args.extend(sv(&["-f", "lavfi", "-i", &src]));
        }
        SceneVideo::Image(p) => {
            args.extend(sv(&rt));
            args.extend(sv(&["-loop", "1", "-framerate", &fps_str(v.fps), "-i", p]));
        }
        SceneVideo::Video { input, length_ms } => {
            args.extend(sv(&rt));
            args.extend(sv(&["-stream_loop", "-1"]));
            let at = if *length_ms > 0 { job.offset_ms % length_ms } else { 0 };
            if at > 0 {
                args.extend(sv(&["-ss", &secs(at)]));
            }
            args.extend(sv(&["-i", input]));
        }
    }
    for t in &job.tracks {
        if t.input.is_empty() {
            args.extend(sv(&["-f", "lavfi", "-t", &secs(t.play_ms.max(1)), "-i", &silence_src(a)]));
            continue;
        }
        if t.seek_ms > 0 {
            args.extend(sv(&["-ss", &secs(t.seek_ms)]));
        }
        args.extend(sv(&["-t", &secs(t.play_ms.max(1)), "-i", &t.input]));
    }
    if job.tracks.is_empty() {
        args.extend(sv(&["-f", "lavfi", "-i", &silence_src(a)]));
    }
    let dur = if job.duration_ms > 0 { frame_align_ms(job.duration_ms, v.fps) } else { 0 };
    let vf = video_chain(v, &job.display, job.fade_in_ms, job.fade_out_ms, dur);
    let af = audio_chain(a, job.volume_db, job.fade_in_ms, job.fade_out_ms, dur);
    let mut fc = format!("[0:v:0]{vf}[v];");
    if job.tracks.is_empty() {
        fc.push_str(&format!("[1:a:0]{af}[a]"));
    } else {
        // every track in the channel format, then one after another
        let norm = format!("aresample={},aformat=sample_fmts=s16:channel_layouts={}", a.sample_rate, layout(a));
        let mut labels = String::new();
        for i in 0..job.tracks.len() {
            fc.push_str(&format!("[{}:a:0]{norm}[m{i}];", i + 1));
            labels.push_str(&format!("[m{i}]"));
        }
        let mut music = vec![format!("{labels}concat=n={}:v=0:a=1", job.tracks.len())];
        if job.music_volume_db.abs() > 0.01 {
            music.push(format!("volume={:.2}dB", job.music_volume_db));
        }
        // the music starts after the leading silence (a delay)
        let lead: i64 = job.tracks.iter().take_while(|t| t.input.is_empty()).map(|t| t.play_ms).sum();
        if job.music_fade_in_ms > 0 {
            let st = if lead > 0 { secs(lead) } else { "0".into() };
            music.push(format!("afade=t=in:st={st}:d={}", secs(job.music_fade_in_ms)));
        }
        // the music ends with the scene, `dur` from here (the airing may
        // start inside), or earlier after a set number of plays
        let end = job.music_end_ms.filter(|e| *e > 0).map_or(dur, |e| if dur > 0 { e.min(dur) } else { e });
        if job.music_fade_out_ms > 0 && end - lead > job.music_fade_out_ms {
            music.push(format!("afade=t=out:st={}:d={}", secs(end - job.music_fade_out_ms), secs(job.music_fade_out_ms)));
        }
        fc.push_str(&format!("{},{af}[a]", music.join(",")));
    }
    args.extend(sv(&["-filter_complex", &fc, "-map", "[v]", "-map", "[a]"]));
    if dur > 0 {
        args.extend(sv(&["-t", &secs(dur)]));
    }
    args.extend(intermediate_codec_args(v.fps, a.channels));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(video: SceneVideo, tracks: Vec<SceneTrack>) -> SceneJob {
        SceneJob {
            video,
            display: Default::default(),
            tracks,
            music_volume_db: -6.0,
            music_fade_in_ms: 1000,
            music_fade_out_ms: 2000,
            music_end_ms: None,
            offset_ms: 0,
            duration_ms: 60_000,
            volume_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
        }
    }

    fn joined(j: &SceneJob) -> String {
        scene_args(j, &VideoFormat::default(), &AudioFormat::default(), "warning").join(" ")
    }

    #[test]
    fn colour_with_silence() {
        let s = joined(&job(SceneVideo::Color("#102030".into()), vec![]));
        assert!(s.contains("-f lavfi -i color=c=0x102030:s=1280x720:r=25"), "{s}");
        assert!(s.contains("anullsrc"));
        assert!(s.contains("[1:a:0]aresample"));
        assert!(s.contains("-t 60.000"));
    }

    #[test]
    fn pauses_and_music_shorter_than_the_scene() {
        let tracks = vec![
            SceneTrack { input: String::new(), seek_ms: 0, play_ms: 5_000 },
            SceneTrack { input: "a.mp3".into(), seek_ms: 0, play_ms: 20_000 },
            SceneTrack { input: String::new(), seek_ms: 0, play_ms: 2_000 },
            SceneTrack { input: "b.mp3".into(), seek_ms: 0, play_ms: 10_000 },
        ];
        let mut j = job(SceneVideo::Color("#000000".into()), tracks);
        j.music_end_ms = Some(37_000);
        let s = joined(&j);
        assert!(s.contains("-f lavfi -t 5.000 -i anullsrc"), "{s}");
        assert!(s.contains("-f lavfi -t 2.000 -i anullsrc"), "{s}");
        assert!(s.contains("concat=n=4:v=0:a=1"), "{s}");
        assert!(s.contains("afade=t=in:st=5.000:d=1.000"), "{s}");
        assert!(s.contains("afade=t=out:st=35.000:d=2.000"), "{s}");
        // the scene itself still lasts its full length
        assert!(s.contains("-t 60.000"));
    }

    #[test]
    fn looped_video_with_two_tracks() {
        let tracks = vec![
            SceneTrack { input: "a.mp3".into(), seek_ms: 5000, play_ms: 30_000 },
            SceneTrack { input: "b.mp3".into(), seek_ms: 0, play_ms: 30_000 },
        ];
        let mut j = job(SceneVideo::Video { input: "loop.mp4".into(), length_ms: 20_000 }, tracks);
        j.offset_ms = 45_000;
        let s = joined(&j);
        assert!(s.contains("-stream_loop -1 -ss 5.000 -i loop.mp4"), "{s}");
        assert!(s.contains("-ss 5.000 -t 30.000 -i a.mp3 -t 30.000 -i b.mp3"), "{s}");
        assert!(s.contains("[m0][m1]concat=n=2:v=0:a=1,volume=-6.00dB,afade=t=in:st=0:d=1.000,afade=t=out:st=58.000:d=2.000"), "{s}");
        assert!(!s.contains("anullsrc"));
    }
}
