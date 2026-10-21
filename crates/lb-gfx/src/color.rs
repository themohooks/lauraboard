//! Colour parsing: `#RGB`, `#RRGGBB`, `#RRGGBBAA` and a few names.

use tiny_skia::Color;

pub fn parse_color(s: &str) -> Color {
    try_parse(s).unwrap_or(Color::WHITE)
}

pub fn try_parse(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let h = |i: usize, n: usize| u8::from_str_radix(&hex[i..i + n], 16).ok();
        return match hex.len() {
            3 => Some(Color::from_rgba8(h(0, 1)? * 17, h(1, 1)? * 17, h(2, 1)? * 17, 255)),
            6 => Some(Color::from_rgba8(h(0, 2)?, h(2, 2)?, h(4, 2)?, 255)),
            8 => Some(Color::from_rgba8(h(0, 2)?, h(2, 2)?, h(4, 2)?, h(6, 2)?)),
            _ => None,
        };
    }
    match s.to_ascii_lowercase().as_str() {
        "white" => Some(Color::WHITE),
        "black" => Some(Color::BLACK),
        "red" => Some(Color::from_rgba8(255, 0, 0, 255)),
        "green" => Some(Color::from_rgba8(0, 128, 0, 255)),
        "blue" => Some(Color::from_rgba8(0, 0, 255, 255)),
        "yellow" => Some(Color::from_rgba8(255, 255, 0, 255)),
        "transparent" => Some(Color::TRANSPARENT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses() {
        let c = parse_color("#FF000080");
        assert!((c.red() - 1.0).abs() < 1e-6 && (c.alpha() - 128.0 / 255.0).abs() < 1e-3);
        assert_eq!(parse_color("#fff"), Color::WHITE);
        assert!(try_parse("#12").is_none());
    }
}
