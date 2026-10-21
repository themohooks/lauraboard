//! The TOML examples in docs/configuration.md must parse with the real
//! configuration types (keeps the documentation honest).

use lb_core::config::{AppConfig, ChannelConfig};
use lb_core::graphics::GraphicsConfig;
use lb_core::rules::RuleSet;
use lb_core::schedule::ScheduleSet;

fn toml_blocks_under(doc: &str, heading: &str) -> Vec<String> {
    let start = doc.find(&format!("## {heading}")).unwrap_or_else(|| panic!("heading {heading}"));
    let rest = &doc[start + 3..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    let section = &rest[..end];
    let mut out = Vec::new();
    let mut it = section.split("```toml\n").skip(1);
    for b in it.by_ref() {
        out.push(b.split("```").next().unwrap().to_string());
    }
    out
}

fn doc() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/configuration.md")).unwrap()
}

#[test]
fn app_config_example() {
    for b in toml_blocks_under(&doc(), "laurabrod.toml") {
        toml::from_str::<AppConfig>(&b).unwrap();
    }
}

#[test]
fn channel_example() {
    let c: ChannelConfig = toml::from_str(&toml_blocks_under(&doc(), "channel.toml")[0]).unwrap();
    assert_eq!(c.outputs.len(), 2);
}

#[test]
fn schedule_example() {
    let s: ScheduleSet = toml::from_str(&toml_blocks_under(&doc(), "schedule.toml")[0]).unwrap();
    assert_eq!(s.schedules[0].templates[0].blocks[0].id, "morning");
    // source variants, one per document
    for line in toml_blocks_under(&doc(), "schedule.toml")[1].lines().filter(|l| l.starts_with("source")) {
        let v: toml::Table = toml::from_str(line).unwrap();
        serde_json::from_value::<lb_core::schedule::BlockSource>(serde_json::to_value(&v["source"]).unwrap()).unwrap();
    }
}

#[test]
fn rules_example() {
    // the section has two blocks (rules, then ad breaks and policies)
    let all = toml_blocks_under(&doc(), "rules.toml").join(
        "
",
    );
    let r: RuleSet = toml::from_str(&all).unwrap();
    assert_eq!(r.rules[0].id, "ads");
    assert_eq!(r.ad_breaks[0].max_spots, Some(3));
}

#[test]
fn graphics_example() {
    let g: GraphicsConfig = toml::from_str(&toml_blocks_under(&doc(), "graphics.toml")[0]).unwrap();
    assert_eq!(g.elements.len(), 2);
    assert!(g.rules[0].timing.last.is_some());
}
