//! Textures for plates and shapes ("picture background" of a shape): wide
//! strips that are cropped to the shape, so they look right at any size.

use tiny_skia::{Pixmap, Transform};

use super::{Backdrop, Seq, c, circle, glow, linear, rect, with_alpha};

pub const PLATES: &[Backdrop] = &[
    Backdrop { stem: "plate-blue-gloss", title: "Синий глянец" },
    Backdrop { stem: "plate-red", title: "Красный акцент" },
    Backdrop { stem: "plate-carbon", title: "Карбон" },
    Backdrop { stem: "plate-glass", title: "Стекло" },
    Backdrop { stem: "plate-gold", title: "Золото" },
    Backdrop { stem: "plate-neon", title: "Неон" },
    Backdrop { stem: "plate-stripes", title: "Полосы" },
    Backdrop { stem: "plate-dusk", title: "Сумерки" },
    Backdrop { stem: "plate-emerald", title: "Изумруд" },
    Backdrop { stem: "plate-steel", title: "Сталь" },
    Backdrop { stem: "plate-sunrise", title: "Рассвет" },
    Backdrop { stem: "plate-night", title: "Ночь со звёздами" },
    Backdrop { stem: "plate-ocean", title: "Океан" },
    Backdrop { stem: "plate-sky", title: "Небо" },
    Backdrop { stem: "plate-fire", title: "Пламя" },
    Backdrop { stem: "plate-violet-gloss", title: "Фиолетовый глянец" },
    Backdrop { stem: "plate-mint", title: "Мята" },
    Backdrop { stem: "plate-silver", title: "Серебро" },
    Backdrop { stem: "plate-chevrons", title: "Шевроны" },
    Backdrop { stem: "plate-halftone", title: "Растр" },
    Backdrop { stem: "plate-paper", title: "Бумага" },
    Backdrop { stem: "plate-newsroom", title: "Новостная студия" },
    Backdrop { stem: "plate-aurora", title: "Северное сияние" },
    Backdrop { stem: "plate-holo", title: "Голограмма" },
    Backdrop { stem: "plate-rose", title: "Роза" },
    Backdrop { stem: "plate-grid", title: "Сетка" },
    Backdrop { stem: "plate-split", title: "Два цвета" },
    Backdrop { stem: "plate-cyber", title: "Киберпанк" },
];

/// Plate textures are this size (cropped / scaled to the shape).
pub const SIZE: (u32, u32) = (1200, 240);

/// Light band across the top (glossy look).
fn gloss(p: &mut Pixmap, a: f32) {
    let (fw, fh) = (p.width() as f32, p.height() as f32);
    let mut top = Pixmap::new(p.width(), (fh * 0.5) as u32).expect("size");
    linear(&mut top, (0.0, 0.0), (0.0, 1.0), &[(0.0, with_alpha("#FFFFFF", a)), (1.0, with_alpha("#FFFFFF", 0.0))]);
    p.draw_pixmap(0, 0, top.as_ref(), &Default::default(), Transform::identity(), None);
    let _ = fw;
}

pub fn paint(stem: &str) -> Option<Pixmap> {
    let (w, h) = SIZE;
    let mut p = Pixmap::new(w, h)?;
    let (fw, fh) = (w as f32, h as f32);
    match stem {
        "plate-blue-gloss" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#1E3A8A")), (1.0, c("#2563EB"))]);
            gloss(&mut p, 0.28);
        }
        "plate-red" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#7F1D1D")), (0.6, c("#B91C1C")), (1.0, c("#EF4444"))]);
            gloss(&mut p, 0.18);
        }
        "plate-carbon" => {
            linear(&mut p, (0.0, 0.0), (0.0, 1.0), &[(0.0, c("#27272A")), (1.0, c("#09090B"))]);
            let step = fh / 10.0;
            for i in -10..140 {
                let t = Transform::from_rotate_at(45.0, fw / 2.0, fh / 2.0);
                rect(&mut p, i as f32 * step - fw, -fw, step * 0.45, fw * 3.0, with_alpha("#FFFFFF", 0.035), t);
            }
            gloss(&mut p, 0.12);
        }
        "plate-glass" => {
            linear(&mut p, (0.0, 0.0), (1.0, 1.0), &[(0.0, with_alpha("#FFFFFF", 0.35)), (1.0, with_alpha("#FFFFFF", 0.12))]);
            gloss(&mut p, 0.25);
        }
        "plate-gold" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#78350F")), (0.35, c("#D97706")), (0.6, c("#FCD34D")), (1.0, c("#B45309"))]);
            gloss(&mut p, 0.3);
        }
        "plate-neon" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#0A0A1A")), (1.0, c("#111133"))]);
            rect(&mut p, 0.0, fh - fh * 0.06, fw, fh * 0.06, c("#22D3EE"), Transform::identity());
            glow(&mut p, 0.15, 1.0, 0.35, with_alpha("#22D3EE", 0.35));
            glow(&mut p, 0.85, 0.0, 0.3, with_alpha("#E879F9", 0.3));
        }
        "plate-stripes" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#0F172A")), (1.0, c("#1E293B"))]);
            for i in 0..40 {
                let t = Transform::from_rotate_at(-20.0, fw / 2.0, fh / 2.0);
                rect(&mut p, i as f32 * fw / 20.0 - fw * 0.5, -fh, fw / 40.0, fh * 3.0, with_alpha("#F59E0B", 0.07), t);
            }
        }
        "plate-dusk" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#312E81")), (0.5, c("#6D28D9")), (1.0, c("#DB2777"))]);
            gloss(&mut p, 0.15);
        }
        "plate-emerald" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#064E3B")), (1.0, c("#059669"))]);
            gloss(&mut p, 0.2);
        }
        "plate-steel" => {
            linear(&mut p, (0.0, 0.0), (0.0, 1.0), &[(0.0, c("#9CA3AF")), (0.5, c("#4B5563")), (1.0, c("#1F2937"))]);
            let mut s = Seq(0x51ED_270B);
            for _ in 0..400 {
                let y = s.next() * fh;
                rect(&mut p, 0.0, y, fw, 1.0, with_alpha("#FFFFFF", s.next() * 0.05), Transform::identity());
            }
        }
        "plate-sunrise" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#F97316")), (1.0, c("#FACC15"))]);
            gloss(&mut p, 0.22);
        }
        "plate-night" => {
            linear(&mut p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#020617")), (1.0, c("#1E1B4B"))]);
            let mut s = Seq(0x0BAD_F00D);
            for _ in 0..120 {
                circle(&mut p, s.next() * fw, s.next() * fh, 0.6 + s.next() * 1.6, with_alpha("#FFFFFF", 0.2 + s.next() * 0.6));
            }
        }
        _ => return more(stem, &mut p).then_some(p),
    }
    Some(p)
}

/// The second set of textures.
fn more(stem: &str, p: &mut Pixmap) -> bool {
    let (fw, fh) = (p.width() as f32, p.height() as f32);
    match stem {
        "plate-ocean" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#083344")), (0.6, c("#0E7490")), (1.0, c("#06B6D4"))]);
            // soft swell lines
            for i in 0..6 {
                let y = fh * (0.25 + i as f32 * 0.12);
                let t = Transform::from_rotate_at(-4.0, fw / 2.0, y);
                rect(p, -fw * 0.1, y, fw * 1.2, 2.0, with_alpha("#FFFFFF", 0.06), t);
            }
            gloss(p, 0.16);
        }
        "plate-sky" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#0369A1")), (1.0, c("#38BDF8"))]);
            glow(p, 0.85, 0.2, 0.3, with_alpha("#FFFFFF", 0.35));
            gloss(p, 0.3);
        }
        "plate-fire" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#450A0A")), (0.45, c("#B91C1C")), (0.8, c("#EA580C")), (1.0, c("#F59E0B"))]);
            glow(p, 0.9, 1.0, 0.25, with_alpha("#FDE047", 0.45));
            glow(p, 0.6, 1.1, 0.2, with_alpha("#F97316", 0.4));
            gloss(p, 0.12);
        }
        "plate-violet-gloss" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#3B0764")), (1.0, c("#7C3AED"))]);
            gloss(p, 0.28);
        }
        "plate-mint" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#0D9488")), (1.0, c("#5EEAD4"))]);
            gloss(p, 0.25);
        }
        "plate-silver" => {
            linear(p, (0.0, 0.0), (0.0, 1.0), &[(0.0, c("#F8FAFC")), (0.45, c("#CBD5E1")), (0.55, c("#94A3B8")), (1.0, c("#E2E8F0"))]);
            let mut s = Seq(0x5117_E401);
            for _ in 0..300 {
                let y = s.next() * fh;
                rect(p, 0.0, y, fw, 1.0, with_alpha("#FFFFFF", s.next() * 0.25), Transform::identity());
            }
        }
        "plate-chevrons" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#111827")), (1.0, c("#1F2937"))]);
            // arrows pointing right over the right half
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(with_alpha("#F59E0B", 0.25));
            paint.anti_alias = true;
            let stroke = tiny_skia::Stroke { width: 16.0, ..Default::default() };
            let mut x = fw * 0.55;
            while x < fw + fh {
                let mut pb = tiny_skia::PathBuilder::new();
                pb.move_to(x - fh * 0.3, fh * 0.15);
                pb.line_to(x, fh * 0.5);
                pb.line_to(x - fh * 0.3, fh * 0.85);
                if let Some(path) = pb.finish() {
                    p.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
                x += fh * 0.35;
            }
            gloss(p, 0.1);
        }
        "plate-halftone" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#1E1B4B")), (1.0, c("#312E81"))]);
            let step = 16.0;
            let (cols, rows) = ((fw / step) as i32, (fh / step) as i32 + 1);
            for i in 0..=cols {
                for j in 0..=rows {
                    let k = i as f32 / cols as f32;
                    let x = i as f32 * step + if j % 2 == 0 { 0.0 } else { step / 2.0 };
                    circle(p, x, j as f32 * step, 0.5 + k * k * 5.0, with_alpha("#818CF8", 0.35));
                }
            }
        }
        "plate-paper" => {
            linear(p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#FEFCE8")), (1.0, c("#F5F5F4"))]);
            let mut s = Seq(0x9A9E_2001);
            for _ in 0..1500 {
                circle(p, s.next() * fw, s.next() * fh, 0.5 + s.next(), with_alpha("#78716C", s.next() * 0.12));
            }
        }
        "plate-newsroom" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#0B1F4D")), (1.0, c("#12306E"))]);
            rect(p, 0.0, 0.0, fw * 0.03, fh, c("#DC2626"), Transform::identity());
            for i in 0..5 {
                let y = fh * (0.2 + i as f32 * 0.15);
                rect(p, fw * 0.55, y, fw * 0.45, 1.5, with_alpha("#93C5FD", 0.12), Transform::identity());
            }
            glow(p, 0.85, 0.5, 0.25, with_alpha("#3B82F6", 0.25));
            gloss(p, 0.14);
        }
        "plate-aurora" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#020617")), (1.0, c("#0F172A"))]);
            glow(p, 0.2, 0.9, 0.3, with_alpha("#22C55E", 0.45));
            glow(p, 0.5, 0.2, 0.3, with_alpha("#06B6D4", 0.4));
            glow(p, 0.85, 0.8, 0.3, with_alpha("#A855F7", 0.4));
        }
        "plate-holo" => {
            linear(p, (0.0, 0.0), (1.0, 1.0), &[(0.0, c("#F0ABFC")), (0.33, c("#A5B4FC")), (0.66, c("#67E8F9")), (1.0, c("#FDE68A"))]);
            gloss(p, 0.35);
        }
        "plate-rose" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#831843")), (0.6, c("#DB2777")), (1.0, c("#F472B6"))]);
            gloss(p, 0.2);
        }
        "plate-grid" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#18181B")), (1.0, c("#27272A"))]);
            let step = 24.0;
            let mut x = 0.0;
            while x < fw {
                rect(p, x, 0.0, 1.0, fh, with_alpha("#FFFFFF", 0.06), Transform::identity());
                x += step;
            }
            let mut y = 0.0;
            while y < fh {
                rect(p, 0.0, y, fw, 1.0, with_alpha("#FFFFFF", 0.06), Transform::identity());
                y += step;
            }
            glow(p, 0.1, 0.5, 0.3, with_alpha("#22D3EE", 0.18));
        }
        "plate-split" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#0F172A")), (1.0, c("#1E293B"))]);
            let t = Transform::from_rotate_at(18.0, fw * 0.22, fh / 2.0);
            rect(p, -fw, -fh * 2.0, fw * 1.22, fh * 5.0, c("#E11D48"), t);
            gloss(p, 0.14);
        }
        "plate-cyber" => {
            linear(p, (0.0, 0.0), (1.0, 0.0), &[(0.0, c("#0B0118")), (1.0, c("#1A0533"))]);
            glow(p, 0.0, 0.5, 0.25, with_alpha("#D946EF", 0.35));
            glow(p, 1.0, 0.5, 0.25, with_alpha("#22D3EE", 0.3));
            let mut y = 0.0;
            while y < fh {
                rect(p, 0.0, y, fw, 1.0, with_alpha("#000000", 0.3), Transform::identity());
                y += 4.0;
            }
            rect(p, 0.0, 0.0, fw, fh * 0.04, c("#F0ABFC"), Transform::identity());
            rect(p, 0.0, fh * 0.96, fw, fh * 0.04, c("#22D3EE"), Transform::identity());
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plate_paints() {
        for b in PLATES {
            assert!(paint(b.stem).is_some(), "{}", b.stem);
        }
    }
}
