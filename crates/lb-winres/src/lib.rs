//! Build-script helper: gives a Windows executable its icon and an
//! application manifest without `rc.exe` / the Windows SDK.
//!
//! It writes a compiled resource file (`.res`: `RT_ICON` images +
//! `RT_GROUP_ICON` from an `.ico`, `RT_MANIFEST`) and hands it to the MSVC
//! linker. On other targets it does nothing.
//!
//! The manifest runs the program as the invoking user (no UAC prompt even
//! for names like `*-setup.exe`), enables per-monitor DPI awareness, long
//! paths and common controls v6 (native dialogs).

use std::path::Path;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_MANIFEST: u16 = 24;
const LANG_EN_US: u16 = 0x0409;

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/></application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
</assembly>
"#;

/// Embed `ico` and the manifest into every binary of the calling package
/// (tests included, so they also run without elevation).
pub fn embed(ico: &Path) {
    println!("cargo:rerun-if-changed={}", ico.display());
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "msvc" {
        return;
    }
    let icon = std::fs::read(ico).unwrap_or_else(|e| panic!("{}: {e}", ico.display()));
    let res = build_res(&icon).unwrap_or_else(|e| panic!("{}: {e}", ico.display()));
    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("app.res");
    std::fs::write(&out, res).expect("write app.res");
    println!("cargo:rustc-link-arg={}", out.display());
    // the linker must not add a manifest of its own next to ours
    println!("cargo:rustc-link-arg=/MANIFEST:NO");
}

/// One resource entry with ordinal type and name (header is always 32 bytes).
fn push_resource(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, lang: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(kind.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(name.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // data version
    out.extend(flags.to_le_bytes());
    out.extend(lang.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // version
    out.extend(0u32.to_le_bytes()); // characteristics
    out.extend(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| "обрезанный .ico".into())
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| "обрезанный .ico".into())
}

/// `.ico` file → `.res` with the icon group (ordinal 1) and the manifest.
pub fn build_res(ico: &[u8]) -> Result<Vec<u8>, String> {
    if u16_at(ico, 0)? != 0 || u16_at(ico, 2)? != 1 {
        return Err("это не .ico".into());
    }
    let count = u16_at(ico, 4)? as usize;
    let mut res = Vec::new();
    // a .res file starts with an empty entry
    push_resource(&mut res, 0, 0, 0, 0, &[]);
    let mut group = Vec::with_capacity(6 + count * 14);
    group.extend([0, 0, 1, 0]);
    group.extend((count as u16).to_le_bytes());
    for i in 0..count {
        let e = 6 + i * 16;
        let entry = ico.get(e..e + 16).ok_or("обрезанный .ico")?;
        let size = u32_at(ico, e + 8)? as usize;
        let offset = u32_at(ico, e + 12)? as usize;
        let image = ico.get(offset..offset + size).ok_or("обрезанный .ico")?;
        let id = (i + 1) as u16;
        push_resource(&mut res, RT_ICON, id, 0x1010, LANG_EN_US, image);
        // GRPICONDIRENTRY = ICONDIRENTRY with the offset replaced by the id
        group.extend(&entry[..12]);
        group.extend(id.to_le_bytes());
    }
    push_resource(&mut res, RT_GROUP_ICON, 1, 0x1030, LANG_EN_US, &group);
    push_resource(&mut res, RT_MANIFEST, 1, 0x0030, LANG_EN_US, MANIFEST.as_bytes());
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal .ico with one 1-byte "image".
    fn ico() -> Vec<u8> {
        let mut v = vec![0, 0, 1, 0, 1, 0];
        v.extend([16, 16, 0, 0, 1, 0, 32, 0]);
        v.extend(1u32.to_le_bytes());
        v.extend(22u32.to_le_bytes());
        v.push(0xAB);
        v
    }

    #[test]
    fn res_layout() {
        let res = build_res(&ico()).unwrap();
        // empty header entry
        assert_eq!(&res[..8], &[0, 0, 0, 0, 32, 0, 0, 0]);
        // icon entry: 1 byte of data padded to 4
        assert_eq!(u32_at(&res, 32).unwrap(), 1);
        assert_eq!(u16_at(&res, 42).unwrap(), RT_ICON);
        assert_eq!(res[64], 0xAB);
        // group follows at 68: 6 + 14 bytes
        assert_eq!(u32_at(&res, 68).unwrap(), 20);
        assert_eq!(u16_at(&res, 78).unwrap(), RT_GROUP_ICON);
        assert_eq!(u16_at(&res, 100 + 6 + 12).unwrap(), 1, "group entry points at icon id 1");
        assert!(res.windows(9).any(|w| w == b"asInvoker"));
        assert_eq!(res.len() % 4, 0);
    }

    #[test]
    fn rejects_other_files() {
        assert!(build_res(b"\x89PNG....").is_err());
        assert!(build_res(&[0, 0, 1, 0, 2, 0]).is_err());
    }
}
