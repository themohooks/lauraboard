use crate::ctx::Ctx;
use anyhow::{Result, anyhow};
use std::path::Path;

fn location(name: &str) -> Result<std::path::PathBuf> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err(anyhow!("Invalid service name"));
    }
    Ok(std::path::PathBuf::from(std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME unavailable"))?)
        .join("Library/LaunchAgents")
        .join(format!("org.lauraboard.{name}.plist")))
}
fn xml(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
pub fn install(name: &str, exe: &Path, root: &Path) -> Result<()> {
    let path = location(name)?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>Label</key><string>org.lauraboard.{name}</string><key>ProgramArguments</key><array><string>{}</string><string>--project</string><string>{}</string><string>run</string></array><key>WorkingDirectory</key><string>{}</string><key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>ProcessType</key><string>Background</string></dict></plist>"#,
        xml(&exe.to_string_lossy()),
        xml(&root.to_string_lossy()),
        xml(&root.to_string_lossy())
    );
    std::fs::write(&path, plist)?;
    let uid = std::process::Command::new("id").arg("-u").output()?;
    let domain = format!("gui/{}", String::from_utf8_lossy(&uid.stdout).trim());
    if !std::process::Command::new("launchctl").args(["bootstrap", &domain]).arg(&path).status()?.success() {
        return Err(anyhow!("launchctl bootstrap failed"));
    }
    Ok(())
}
pub fn uninstall(name: &str) -> Result<()> {
    let path = location(name)?;
    let uid = std::process::Command::new("id").arg("-u").output()?;
    let domain = format!("gui/{}", String::from_utf8_lossy(&uid.stdout).trim());
    let _ = std::process::Command::new("launchctl").args(["bootout", &domain]).arg(&path).status()?;
    std::fs::remove_file(path)?;
    Ok(())
}
pub fn service_main(_: &str, ctx: &Ctx) -> Result<()> {
    crate::commands::run::run(ctx, true)
}
