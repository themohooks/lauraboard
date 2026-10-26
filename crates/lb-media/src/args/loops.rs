//! Ready-made background videos: seamless loops computed by FFmpeg (`geq`
//! at a small size, then scaled and softened). Every motion is periodic in
//! [`PERIOD_S`], so the last frame leads into the first and the video can
//! repeat endlessly behind a scene.

use super::{fps_str, sv};

/// Loop length, seconds.
pub const PERIOD_S: u32 = 20;
/// Size the pattern is computed at (smooth pictures need no more).
const SRC: (u32, u32) = (384, 216);

pub struct LoopSpec {
    pub stem: &'static str,
    pub title: &'static str,
    /// Brightness pattern `v` in [-1, 1] over X, Y, `w` = 2π/period · T.
    v: &'static str,
    /// Colour at v = -1 and v = 1 (`r, g, b` 0…255).
    dark: (u8, u8, u8),
    light: (u8, u8, u8),
}

pub const LOOPS: &[LoopSpec] = &[
    LoopSpec {
        stem: "loop-plasma-blue",
        title: "Плазма, синяя",
        v: "(sin(X/23+W)+sin(Y/17-W)+sin((X+Y)/31+2*W))/3",
        dark: (8, 12, 40),
        light: (40, 90, 200),
    },
    LoopSpec {
        stem: "loop-plasma-violet",
        title: "Плазма, фиолетовая",
        v: "(sin(X/29-W)+sin(Y/21+2*W)+sin((X-Y)/37+W))/3",
        dark: (20, 6, 40),
        light: (150, 60, 190),
    },
    LoopSpec {
        stem: "loop-flow-sunset", title: "Перелив, закат", v: "sin(X/70+Y/140+W)", dark: (40, 8, 40), light: (220, 90, 40)
    },
    LoopSpec { stem: "loop-flow-sea", title: "Перелив, море", v: "sin(X/90-Y/60+W)", dark: (2, 30, 50), light: (20, 160, 190) },
    LoopSpec { stem: "loop-waves", title: "Волны", v: "sin(Y/26+2*sin(X/60+W))", dark: (4, 20, 45), light: (22, 80, 135) },
    LoopSpec {
        stem: "loop-aurora",
        title: "Северное сияние",
        v: "2*(exp(-pow((Y-80-25*sin(X/50+W)-8*sin(X/23-2*W))/48,2))+0.5*exp(-pow((Y-130-20*sin(X/40-W))/40,2)))/1.5-1",
        dark: (2, 8, 22),
        light: (30, 170, 130),
    },
    LoopSpec {
        stem: "loop-pulse",
        title: "Пульс",
        v: "sin(hypot(X-192,Y-108)/16-W)*exp(-hypot(X-192,Y-108)/160)",
        dark: (6, 10, 25),
        light: (45, 70, 160),
    },
    LoopSpec { stem: "loop-grid", title: "Мозаика", v: "sin(X/37+W)*sin(Y/37-W)", dark: (10, 14, 22), light: (48, 62, 92) },
    LoopSpec {
        stem: "loop-ember",
        title: "Тлеющие угли",
        v: "(sin(X/41+W)*sin(Y/29-W)+sin((X+Y)/53+W))/2",
        dark: (12, 3, 1),
        light: (150, 50, 12),
    },
    LoopSpec {
        stem: "loop-mint", title: "Мятный туман", v: "(sin(X/50+W)+sin(Y/40-W))/2", dark: (210, 240, 230), light: (120, 210, 200)
    },
];

impl LoopSpec {
    /// `geq` colour expression for one channel.
    fn channel(&self, a: u8, b: u8) -> String {
        let v = self.v.replace('W', &format!("(2*PI*T/{PERIOD_S})"));
        format!("{a}+({b}-{a})*({v}+1)/2")
    }
}

/// FFmpeg arguments (without the program) writing the loop to `out` at
/// `w`×`h`, `fps`.
pub fn loop_args(spec: &LoopSpec, w: u32, h: u32, fps: f64, out: &str) -> Vec<String> {
    let src = format!("nullsrc=s={}x{}:r={}:d={PERIOD_S}", SRC.0, SRC.1, fps_str(fps));
    let geq = format!(
        "geq=r='{}':g='{}':b='{}'",
        spec.channel(spec.dark.0, spec.light.0),
        spec.channel(spec.dark.1, spec.light.1),
        spec.channel(spec.dark.2, spec.light.2)
    );
    let vf = format!("format=rgb24,{geq},scale={w}:{h}:flags=bicubic,gblur=sigma=2,noise=alls=3:allf=t,format=yuv420p");
    let mut a = sv(&["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-f", "lavfi", "-i", &src, "-vf", &vf]);
    a.extend(sv(&[
        "-t",
        &PERIOD_S.to_string(),
        "-an",
        "-c:v",
        "libx264",
        "-preset",
        "medium",
        "-crf",
        "24",
        "-g",
        &(fps.round() as i64 * 2).to_string(),
    ]));
    a.extend(sv(&["-movflags", "+faststart", out]));
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periodic_expression() {
        let a = loop_args(&LOOPS[0], 1920, 1080, 25.0, "out.mp4").join(" ");
        assert!(a.contains("nullsrc=s=384x216:r=25:d=20"), "{a}");
        assert!(a.contains("(2*PI*T/20)"));
        assert!(!a.contains("+W)"), "W replaced");
        assert!(a.ends_with("out.mp4"));
    }

    #[test]
    fn stems_are_unique() {
        let mut s: Vec<&str> = LOOPS.iter().map(|l| l.stem).collect();
        s.sort();
        s.dedup();
        assert_eq!(s.len(), LOOPS.len());
    }
}
