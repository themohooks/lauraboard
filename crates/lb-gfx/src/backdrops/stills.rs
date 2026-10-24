//! Full-frame still backgrounds (for scenes, slates, filler pictures).

use tiny_skia::{Pixmap, Transform};

use super::{Backdrop, Seq, c, circle, glow, linear, rect, vignette, with_alpha};

pub const STILLS: &[Backdrop] = &[
    Backdrop { stem: "bg-midnight", title: "Полночь" },
    Backdrop { stem: "bg-sunset", title: "Закат" },
    Backdrop { stem: "bg-ocean", title: "Океан" },
    Backdrop { stem: "bg-forest", title: "Лес" },
    Backdrop { stem: "bg-graphite", title: "Графит" },
    Backdrop { stem: "bg-aurora", title: "Аврора" },
    Backdrop { stem: "bg-bokeh", title: "Боке" },
    Backdrop { stem: "bg-dots", title: "Точки" },
    Backdrop { stem: "bg-waves", title: "Волны" },
    Backdrop { stem: "bg-studio", title: "Студия" },
    Backdrop { stem: "bg-news", title: "Новости" },
    Backdrop { stem: "bg-pastel", title: "Пастель" },
    Backdrop { stem: "bg-ember", title: "Угли" },
    Backdrop { stem: "bg-violet", title: "Фиолетовый туман" },
    Backdrop { stem: "bg-lines", title: "Диагонали" },
    Backdrop { stem: "bg-cosmos", title: "Космос" },
];

/// Paint still `stem` at `w`×`h` (`None`: unknown stem).
pub fn paint(stem: &str, w: u32, h: u32) -> Option<Pixmap> {
    let mut p = Pixmap::new(w.max(16), h.max(16))?;
    let (fw, fh) = (p.width() as f32, p.height() as f32);
    match stem {
        "bg-midnight" => {
            linear(&mut p, (0.0, 0.0), (0.0, 1.0), &[(0.0, c("#0B1020")), (1.0, c("#1E3A8A"))]);
            glow(&mut p, 0.75, 0.25, 0.5, with_alpha("#3B82F6", 0.35));
            vignette(&mut p, 0.5);
        }
        "bg-sunset" => {
            linear(&mut p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#1A1033")), (0.55, c("#9D174D")), (1.0, c("#F59E0B"))]);
            glow(&mut p, 0.8, 0.85, 0.45, with_alpha("#FDE68A", 0.35));
            vignette(&mut p, 0.4);
        }
        "bg-ocean" => {
            linear(&mut p, (0.0, 0.0), (0.0, 1.0), &[(0.0, c("#002B45")), (1.0, c("#0EA5E9"))]);
            glow(&mut p, 0.3, 0.2, 0.5, with_alpha("#67E8F9", 0.25));
            vignette(&mut p, 0.45);
        }
        "bg-forest" => {
            linear(&mut p, (0.0, 1.0), (1.0, 0.0), &[(0.0, c("#022C22")), (1.0, c("#15803D"))]);
            glow(&mut p, 0.65, 0.35, 0.45, with_alpha("#BEF264", 0.2));
            vignette(&mut p, 0.5);
        }
        "bg-graphite" => {
            linear(&mut p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#111827")), (1.0, c("#374151"))]);
            let step = fh / 18.0;
            for i in -20..60 {
                let t = Transform::from_rotate_at(-30.0, fw / 2.0, fh / 2.0);
                rect(&mut p, i as f32 * step * 2.0 - fw * 0.5, -fh, step * 0.5, fh * 3.0, with_alpha("#FFFFFF", 0.025), t);
            }
            vignette(&mut p, 0.55);
        }
        "bg-aurora" => {
            linear(&mut p, (0.0, 0.0), (0.0, 1.0), &[(0.0, c("#020617")), (1.0, c("#0F172A"))]);
            glow(&mut p, 0.25, 0.35, 0.45, with_alpha("#14B8A6", 0.45));
            glow(&mut p, 0.6, 0.25, 0.4, with_alpha("#8B5CF6", 0.4));
            glow(&mut p, 0.85, 0.6, 0.35, with_alpha("#22C55E", 0.3));
            vignette(&mut p, 0.4);
        }
        "bg-bokeh" => {
            linear(&mut p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#0B1026")), (1.0, c("#1F2A5A"))]);
            let mut s = Seq(0x9E37_79B9);
            let colors = ["#F472B6", "#60A5FA", "#FBBF24", "#A78BFA", "#34D399"];
            for i in 0..42 {
                let (x, y, r) = (s.next() * fw, s.next() * fh, (0.02 + s.next() * 0.07) * fw);
                circle(&mut p, x, y, r, with_alpha(colors[i % colors.len()], 0.05 + s.next() * 0.12));
            }
            vignette(&mut p, 0.45);
        }
        "bg-dots" => {
            linear(&mut p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#0F172A")), (1.0, c("#1E293B"))]);
            let step = fh / 24.0;
            let mut y = step / 2.0;
            while y < fh {
                let mut x = step / 2.0;
                while x < fw {
                    circle(&mut p, x, y, step * 0.08, with_alpha("#94A3B8", 0.35));
                    x += step;
                }
                y += step;
            }
            glow(&mut p, 0.5, 0.5, 0.5, with_alpha("#38BDF8", 0.15));
            vignette(&mut p, 0.6);
        }
        "bg-waves" => {
            linear(&mut p, (0.0, 0.0), (0.0, 1.0), &[(0.0, c("#0C4A6E")), (1.0, c("#082F49"))]);
            for k in 0..6 {
                let mut pb = tiny_skia::PathBuilder::new();
                let base = fh * (0.45 + k as f32 * 0.1);
                pb.move_to(0.0, fh);
                let mut x = 0.0;
                while x <= fw {
                    let y = base + (x / fw * std::f32::consts::TAU * 1.5 + k as f32).sin() * fh * 0.05;
                    pb.line_to(x, y);
                    x += fw / 120.0;
                }
                pb.line_to(fw, fh);
                pb.close();
                if let Some(path) = pb.finish() {
                    let mut paint = tiny_skia::Paint::default();
                    paint.set_color(with_alpha("#38BDF8", 0.08));
                    paint.anti_alias = true;
                    p.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                }
            }
            vignette(&mut p, 0.4);
        }
        "bg-studio" => {
            linear(&mut p, (0.0, 0.0), (0.0, 1.0), &[(0.0, c("#1F2937")), (1.0, c("#0B0F14"))]);
            glow(&mut p, 0.5, 0.42, 0.55, with_alpha("#E5E7EB", 0.35));
            vignette(&mut p, 0.65);
        }
        "bg-news" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#0B1A3A")), (1.0, c("#1D4ED8"))]);
            let t = Transform::from_rotate_at(-18.0, fw / 2.0, fh / 2.0);
            rect(&mut p, fw * 0.55, -fh, fw * 0.9, fh * 3.0, with_alpha("#B91C1C", 0.85), t);
            rect(&mut p, fw * 0.52, -fh, fw * 0.012, fh * 3.0, with_alpha("#FFFFFF", 0.7), t);
            for i in 0..14 {
                rect(&mut p, 0.0, fh * (0.08 + i as f32 * 0.065), fw, 1.0_f32.max(fh / 900.0), with_alpha("#FFFFFF", 0.05), Transform::identity());
            }
            vignette(&mut p, 0.45);
        }
        "bg-pastel" => {
            linear(&mut p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#FCE7F3")), (0.5, c("#E0E7FF")), (1.0, c("#CFFAFE"))]);
            glow(&mut p, 0.2, 0.8, 0.4, with_alpha("#FFFFFF", 0.6));
        }
        "bg-ember" => {
            linear(&mut p, (0.0, 1.0), (0.0, 0.0), &[(0.0, c("#7C2D12")), (0.6, c("#1C0A05")), (1.0, c("#0A0503"))]);
            let mut s = Seq(0x2545_F491);
            for _ in 0..90 {
                let (x, y) = (s.next() * fw, fh * (0.5 + s.next() * 0.5));
                circle(&mut p, x, y, fh * (0.002 + s.next() * 0.006), with_alpha("#FDBA74", 0.2 + s.next() * 0.6));
            }
            vignette(&mut p, 0.5);
        }
        "bg-violet" => {
            linear(&mut p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#1E1B4B")), (1.0, c("#581C87"))]);
            glow(&mut p, 0.3, 0.7, 0.5, with_alpha("#C084FC", 0.3));
            glow(&mut p, 0.8, 0.2, 0.4, with_alpha("#F0ABFC", 0.2));
            vignette(&mut p, 0.45);
        }
        "bg-lines" => {
            linear(&mut p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#0F172A")), (1.0, c("#134E4A"))]);
            let t = Transform::from_rotate_at(35.0, fw / 2.0, fh / 2.0);
            for i in -30..60 {
                let a = if i % 5 == 0 { 0.12 } else { 0.04 };
                rect(&mut p, i as f32 * fh / 12.0 - fw * 0.5, -fh, (fh / 400.0).max(1.0), fh * 3.0, with_alpha("#5EEAD4", a), t);
            }
            vignette(&mut p, 0.5);
        }
        "bg-cosmos" => {
            linear(&mut p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#000005")), (1.0, c("#0B0B2B"))]);
            glow(&mut p, 0.7, 0.4, 0.4, with_alpha("#6D28D9", 0.35));
            glow(&mut p, 0.3, 0.65, 0.3, with_alpha("#0EA5E9", 0.25));
            let mut s = Seq(0x1234_5677);
            for _ in 0..260 {
                let (x, y) = (s.next() * fw, s.next() * fh);
                circle(&mut p, x, y, fh * (0.0006 + s.next() * 0.0016), with_alpha("#FFFFFF", 0.3 + s.next() * 0.7));
            }
        }
        _ => return None,
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_still_paints_something() {
        for b in STILLS {
            let p = paint(b.stem, 160, 90).unwrap_or_else(|| panic!("{} not painted", b.stem));
            let opaque = p.data().chunks_exact(4).filter(|px| px[3] == 255).count();
            assert_eq!(opaque, 160 * 90, "{} must be opaque", b.stem);
        }
        assert!(paint("nope", 10, 10).is_none());
    }
}
