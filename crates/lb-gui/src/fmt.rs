//! Display formatting shared by the bridge modules.

use lb_core::Ms;
use lb_core::media::MediaKind;
use lb_core::rundown::RundownItem;

/// `HH:MM:SS` (negative values get a minus sign).
pub fn hms(ms: Ms) -> String {
    lb_core::fmt_ms(ms)
}

/// Stable id used by the UI for colors / icons (`Kinds` in indicators.slint).
pub fn kind_id(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Program => "program",
        MediaKind::Jingle => "jingle",
        MediaKind::Ad => "ad",
        MediaKind::Promo => "promo",
        MediaKind::Filler => "filler",
        MediaKind::Image => "image",
        MediaKind::Stream => "stream",
    }
}

pub fn kind_name(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Program => "Передача",
        MediaKind::Jingle => "Заставка",
        MediaKind::Ad => "Реклама",
        MediaKind::Promo => "Анонс",
        MediaKind::Filler => "Заполнение",
        MediaKind::Image => "Изображение",
        MediaKind::Stream => "Поток",
    }
}

pub const KINDS: [MediaKind; 7] =
    [MediaKind::Program, MediaKind::Jingle, MediaKind::Ad, MediaKind::Promo, MediaKind::Filler, MediaKind::Image, MediaKind::Stream];

/// Kind id of a rundown item; empty slots are "gap".
pub fn item_kind(it: &RundownItem) -> &'static str {
    if it.is_slate() { "gap" } else { kind_id(it.kind) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hms_formats_hours() {
        assert_eq!(hms(3_725_000), "01:02:05");
        assert_eq!(hms(-5_000), "-00:00:05");
    }
}
