//! Shared formatting helpers and small widgets.

use eframe::egui::{self, Color32, RichText, Ui};

use lb_core::Ms;
use lb_core::media::MediaKind;

pub fn hms(ms: Ms) -> String {
    lb_core::fmt_ms(ms)
}

pub fn kind_color(k: MediaKind) -> Color32 {
    match k {
        MediaKind::Program => Color32::from_rgb(90, 160, 255),
        MediaKind::Jingle => Color32::from_rgb(200, 140, 255),
        MediaKind::Ad => Color32::from_rgb(255, 170, 60),
        MediaKind::Promo => Color32::from_rgb(80, 210, 160),
        MediaKind::Filler => Color32::from_rgb(160, 160, 160),
        MediaKind::Image => Color32::from_rgb(120, 120, 120),
        MediaKind::Stream => Color32::from_rgb(255, 110, 110),
    }
}

pub fn kind_name(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Program => "передача",
        MediaKind::Jingle => "заставка",
        MediaKind::Ad => "реклама",
        MediaKind::Promo => "анонс",
        MediaKind::Filler => "заполнение",
        MediaKind::Image => "изображение",
        MediaKind::Stream => "поток",
    }
}

pub const KINDS: [MediaKind; 7] =
    [MediaKind::Program, MediaKind::Jingle, MediaKind::Ad, MediaKind::Promo, MediaKind::Filler, MediaKind::Image, MediaKind::Stream];

/// Coloured state badge.
pub fn pill(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::new().fill(color).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.label(RichText::new(text).color(Color32::BLACK).strong());
    });
}

/// Comma separated list editor for `Vec<String>`.
pub fn list_edit(ui: &mut Ui, label: &str, v: &mut Vec<String>) -> bool {
    let mut s = v.join(", ");
    ui.label(label);
    let changed = ui.text_edit_singleline(&mut s).changed();
    if changed {
        *v = s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    }
    changed
}

/// Optional duration editor (`Dur` as text: `30s`, `00:01:00`, empty = none).
pub fn dur_edit(ui: &mut Ui, label: &str, v: &mut Option<lb_core::Dur>, buf: &mut String) {
    ui.label(label);
    if buf.is_empty()
        && let Some(d) = v
    {
        *buf = d.to_string();
    }
    let r = ui.add(egui::TextEdit::singleline(buf).desired_width(90.0).hint_text("—"));
    if r.changed() {
        *v = if buf.trim().is_empty() { None } else { buf.parse().ok().or(*v) };
    }
}

/// A pending dangerous action awaiting confirmation.
pub struct Confirm {
    pub text: String,
    pub action: Box<dyn FnOnce(&mut crate::app::App)>,
}

/// Show a confirmation modal; returns the action when confirmed.
pub fn confirm_modal(ctx: &egui::Context, c: &Confirm) -> Option<bool> {
    let mut result = None;
    egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.heading("Подтверждение");
        ui.add_space(6.0);
        ui.label(&c.text);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Да, выполнить").color(Color32::from_rgb(255, 120, 120))).clicked() {
                result = Some(true);
            }
            if ui.button("Отмена").clicked() {
                result = Some(false);
            }
        });
    });
    result
}
