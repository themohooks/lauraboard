//! Windows: `.lnk` shortcuts, user PATH and the uninstall entry, all
//! per-user (HKCU), so no administrator rights are needed.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, RegType};

use super::{APP_COMMENT, APP_NAME, PathChange, Registration};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Laurabrod";

/// Child console programs run without a console window.
pub fn no_window(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Start menu entry (`…\Start Menu\Programs\Laurabrod.lnk`).
pub fn menu_shortcut(binary: &Path, _icon: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = dirs::data_dir().ok_or("нет каталога AppData")?.join(r"Microsoft\Windows\Start Menu\Programs");
    Ok(vec![shortcut(binary, &dir)?])
}

pub fn desktop_shortcut(binary: &Path, _icon: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = dirs::desktop_dir().ok_or("нет рабочего стола")?;
    Ok(vec![shortcut(binary, &dir)?])
}

fn shortcut(binary: &Path, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let lnk = dir.join(format!("{APP_NAME}.lnk"));
    let mut link = mslnk::ShellLink::new(binary).map_err(|e| format!("ярлык: {e}"))?;
    link.set_name(Some(APP_COMMENT.to_string()));
    link.set_working_dir(binary.parent().map(|d| d.display().to_string()));
    link.set_icon_location(Some(binary.display().to_string()));
    link.create_lnk(&lnk).map_err(|e| format!("{}: {e}", lnk.display()))?;
    Ok(lnk)
}

fn environment() -> Result<RegKey, String> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE).map_err(|e| format!("реестр: {e}"))
}

fn read_path(env: &RegKey) -> (String, RegType) {
    match env.get_raw_value("Path") {
        Ok(v) => {
            let wide: Vec<u16> = v.bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            (String::from_utf16_lossy(&wide).trim_end_matches('\0').to_string(), v.vtype)
        }
        Err(_) => (String::new(), RegType::REG_EXPAND_SZ),
    }
}

fn write_path(env: &RegKey, value: &str, vtype: RegType) -> Result<(), String> {
    let bytes: Vec<u8> = value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    env.set_raw_value("Path", &winreg::RegValue { bytes: bytes.into(), vtype }).map_err(|e| format!("PATH: {e}"))?;
    broadcast_environment();
    Ok(())
}

fn same_dir(a: &str, b: &Path) -> bool {
    !a.trim().is_empty() && a.trim().trim_end_matches('\\').eq_ignore_ascii_case(b.display().to_string().trim_end_matches('\\'))
}

/// Append the install directory to the user's PATH (once).
pub fn add_to_path(dir: &Path, _binary: &Path) -> Result<PathChange, String> {
    let env = environment()?;
    let (path, vtype) = read_path(&env);
    if !path.split(';').any(|p| same_dir(p, dir)) {
        let joined =
            if path.is_empty() || path.ends_with(';') { format!("{path}{}", dir.display()) } else { format!("{path};{}", dir.display()) };
        write_path(&env, &joined, vtype)?;
    }
    Ok(PathChange { entry: Some(dir.to_path_buf()), extras: Vec::new() })
}

pub fn remove_from_path(dir: &Path) -> Result<(), String> {
    let env = environment()?;
    let (path, vtype) = read_path(&env);
    // only our entry goes; everything else (even empty items) stays as it was
    let kept: Vec<&str> = path.split(';').filter(|p| p.is_empty() || !same_dir(p, dir)).collect();
    let joined = kept.join(";");
    if joined != path { write_path(&env, &joined, vtype) } else { Ok(()) }
}

/// Tell Explorer (and new terminals) that the environment changed.
fn broadcast_environment() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};
    let name: Vec<u16> = "Environment".encode_utf16().chain([0]).collect();
    let mut result = 0usize;
    // SAFETY: `name` outlives the call; the result pointer is valid.
    unsafe { SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, name.as_ptr() as isize, SMTO_ABORTIFHUNG, 3000, &mut result) };
}

/// Entry in "Settings → Apps" with the uninstaller.
pub fn register(r: &Registration) -> Result<bool, String> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL_KEY).map_err(|e| format!("реестр: {e}"))?;
    let un = r.uninstaller.display().to_string();
    let set = |name: &str, value: &str| key.set_value(name, &value).map_err(|e| format!("реестр {name}: {e}"));
    set("DisplayName", APP_NAME)?;
    set("DisplayVersion", r.version)?;
    set("Publisher", "Laurabrod contributors")?;
    set("Comments", APP_COMMENT)?;
    set("InstallLocation", &r.dir.display().to_string())?;
    set("DisplayIcon", &r.binary.display().to_string())?;
    set("UninstallString", &format!("\"{un}\" --uninstall"))?;
    set("QuietUninstallString", &format!("\"{un}\" --uninstall --silent"))?;
    let dword = |name: &str, value: u32| key.set_value(name, &value).map_err(|e| format!("реестр {name}: {e}"));
    dword("EstimatedSize", (r.size / 1024).min(u32::MAX as u64) as u32)?;
    dword("NoModify", 1)?;
    dword("NoRepair", 1)?;
    Ok(true)
}

pub fn unregister() {
    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY);
}

/// Files Windows keeps locked (the running uninstaller itself) are deleted
/// by a hidden `cmd` once this process has exited; then the empty folder.
pub fn delete_later(files: &[PathBuf], dir: &Path) {
    let mut script = String::from("ping 127.0.0.1 -n 3 > nul");
    for f in files {
        script.push_str(&format!(" & del /f /q \"{}\"", f.display()));
    }
    script.push_str(&format!(" & rmdir \"{}\"", dir.display()));
    let _ = Command::new("cmd").raw_arg(format!("/C \"{script}\"")).creation_flags(CREATE_NO_WINDOW).spawn();
}

/// Start the installed application as if from its shortcut.
pub fn launch(binary: &Path) -> Result<(), String> {
    let mut cmd = Command::new(binary);
    cmd.current_dir(binary.parent().unwrap_or(Path::new(".")));
    no_window(&mut cmd);
    cmd.spawn().map(|_| ()).map_err(|e| format!("{}: {e}", binary.display()))
}

/// Show console output in `--silent` mode (the installer has no console).
pub fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: plain Win32 call; fails harmlessly without a parent console.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}
