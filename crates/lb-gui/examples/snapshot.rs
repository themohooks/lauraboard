//! Render UI sections to PNG files without a display.
//!
//! `cargo run -p lb-gui --example snapshot --features snapshot -- <out dir> [section] [WxH] [scale]`
//! writes `<section>-light.png` and `<section>-dark.png`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = std::path::PathBuf::from(args.first().map(String::as_str).unwrap_or("target/snapshots"));
    let section = args.get(1).cloned().unwrap_or_else(|| "gallery".into());
    let (w, h) = args
        .get(2)
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or((1400, 860));
    let scale = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    if let Err(e) = lb_gui::devtools::snapshot(&out, &section, w, h, scale) {
        eprintln!("snapshot failed: {e}");
        std::process::exit(1);
    }
}
