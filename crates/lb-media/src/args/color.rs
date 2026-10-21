//! Colour helpers: `#RRGGBB` / `#RRGGBBAA` → ffmpeg `0xRRGGBB`.

/// Convert a CSS-like colour to ffmpeg syntax. Named colours pass through.
pub fn ffmpeg_color(c: &str) -> String {
    let c = c.trim();
    match c.strip_prefix('#') {
        Some(hex) if hex.len() == 6 || hex.len() == 8 => format!("0x{}", &hex[..6]),
        Some(hex) if hex.len() == 3 => {
            let e: String = hex.chars().flat_map(|ch| [ch, ch]).collect();
            format!("0x{e}")
        }
        _ => c.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts() {
        assert_eq!(ffmpeg_color("#1A2B3C"), "0x1A2B3C");
        assert_eq!(ffmpeg_color("#1A2B3CFF"), "0x1A2B3C");
        assert_eq!(ffmpeg_color("#abc"), "0xaabbcc");
        assert_eq!(ffmpeg_color("black"), "black");
    }
}
