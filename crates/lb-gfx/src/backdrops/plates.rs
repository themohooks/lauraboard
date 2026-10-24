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
        _ => return None,
    }
    Some(p)
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
