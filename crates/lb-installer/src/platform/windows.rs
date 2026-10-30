//! Windows: `.lnk` shortcuts, PATH and the uninstall entry. A per-user
//! installation touches only HKCU and the user's folders (no administrator
//! rights); one for all users uses HKLM, Program Files and the common Start
//! menu / desktop, and runs elevated ([`elevate`]).

use std::ffi::OsString;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, RegType};

use super::{APP_COMMENT, APP_NAME, PathChange, Registration};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\LauraBoard";
const MACHINE_ENVIRONMENT: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// Child console programs run without a console window.
pub fn no_window(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

fn env_dir(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from).ok_or_else(|| format!("нет переменной {name}"))
}

/// Parent of a new installation: per-user programs or Program Files.
pub fn programs_dir(all_users: bool) -> Option<PathBuf> {
    if all_users { env_dir("ProgramFiles").ok() } else { Some(dirs::data_local_dir()?.join("Programs")) }
}

/// `%ProgramData%\LauraBoard` — the receipt of an all-users installation.
pub fn machine_data_dir() -> Option<PathBuf> {
    Some(env_dir("ProgramData").ok()?.join(APP_NAME))
}

/// Start menu entry (`…\Start Menu\Programs\LauraBoard.lnk`).
pub fn menu_shortcut(binary: &Path, _icon: &Path, all_users: bool) -> Result<Vec<PathBuf>, String> {
    let base = if all_users { env_dir("ProgramData")? } else { dirs::data_dir().ok_or("нет каталога AppData")? };
    Ok(vec![shortcut(binary, &base.join(r"Microsoft\Windows\Start Menu\Programs"))?])
}

pub fn desktop_shortcut(binary: &Path, _icon: &Path, all_users: bool) -> Result<Vec<PathBuf>, String> {
    let dir = if all_users { env_dir("PUBLIC")?.join("Desktop") } else { dirs::desktop_dir().ok_or("нет рабочего стола")? };
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

/// The user's or the system's environment key.
fn environment(all_users: bool) -> Result<RegKey, String> {
    let (root, key) = if all_users { (HKEY_LOCAL_MACHINE, MACHINE_ENVIRONMENT) } else { (HKEY_CURRENT_USER, "Environment") };
    RegKey::predef(root).open_subkey_with_flags(key, KEY_READ | KEY_WRITE).map_err(|e| format!("реестр: {e}"))
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

/// Append the install directory to the user's (or system) PATH, once.
pub fn add_to_path(dir: &Path, _binary: &Path, all_users: bool) -> Result<PathChange, String> {
    let env = environment(all_users)?;
    let (path, vtype) = read_path(&env);
    if !path.split(';').any(|p| same_dir(p, dir)) {
        let joined =
            if path.is_empty() || path.ends_with(';') { format!("{path}{}", dir.display()) } else { format!("{path};{}", dir.display()) };
        write_path(&env, &joined, vtype)?;
    }
    Ok(PathChange { entry: Some(dir.to_path_buf()), extras: Vec::new() })
}

pub fn remove_from_path(dir: &Path, all_users: bool) -> Result<(), String> {
    let env = environment(all_users)?;
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

fn uninstall_root(all_users: bool) -> RegKey {
    RegKey::predef(if all_users { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER })
}

/// Entry in "Settings → Apps" with the uninstaller.
pub fn register(r: &Registration) -> Result<bool, String> {
    let (key, _) = uninstall_root(r.all_users).create_subkey(UNINSTALL_KEY).map_err(|e| format!("реестр: {e}"))?;
    let un = r.uninstaller.display().to_string();
    let set = |name: &str, value: &str| key.set_value(name, &value).map_err(|e| format!("реестр {name}: {e}"));
    set("DisplayName", APP_NAME)?;
    set("DisplayVersion", r.version)?;
    set("Publisher", "LauraBoard contributors")?;
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

pub fn unregister(all_users: bool) {
    let _ = uninstall_root(all_users).delete_subkey_all(UNINSTALL_KEY);
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

/// Start the installed application as if from its shortcut — never with
/// the installer's administrator rights: an elevated installer asks
/// Explorer (running as the signed-in user) to start it.
pub fn launch(binary: &Path) -> Result<(), String> {
    let mut cmd = if elevated() {
        let mut c = Command::new("explorer.exe");
        c.arg(binary);
        c
    } else {
        let mut c = Command::new(binary);
        no_window(&mut c);
        c
    };
    cmd.current_dir(binary.parent().unwrap_or(Path::new(".")));
    cmd.spawn().map(|_| ()).map_err(|e| format!("{}: {e}", binary.display()))
}

/// Show console output in `--silent` mode (the installer has no console).
pub fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: plain Win32 call; fails harmlessly without a parent console.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// Running with administrator rights.
pub fn elevated() -> bool {
    // SAFETY: no arguments, no state.
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

/// Start this installer again with administrator rights (the UAC prompt)
/// and `args`. `wait`: block until it exits and return its exit code.
/// `Err` when the user declined the prompt.
pub fn elevate(args: &[OsString], wait: bool) -> Result<Option<u32>, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide().chain([0]).collect()
    };
    let params = args.iter().map(|a| quote(&a.to_string_lossy())).collect::<Vec<_>>().join(" ");
    let (verb, file, params) = (wide("runas".as_ref()), wide(exe.as_os_str()), wide(params.as_ref()));
    // SAFETY: zero is a valid "empty" SHELLEXECUTEINFOW; the strings outlive
    // the call; the process handle is closed below.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        if ShellExecuteExW(&mut info) == 0 {
            return Err("Нужны права администратора: запрос был отклонён".into());
        }
        let mut code = None;
        if wait && !info.hProcess.is_null() && WaitForSingleObject(info.hProcess, INFINITE) == WAIT_OBJECT_0 {
            let mut c = 0u32;
            if GetExitCodeProcess(info.hProcess, &mut c) != 0 {
                code = Some(c);
            }
        }
        if !info.hProcess.is_null() {
            CloseHandle(info.hProcess);
        }
        Ok(code)
    }
}

/// Wait (up to `timeout`) until process `pid` has exited.
pub fn wait_exit(pid: u32, timeout: std::time::Duration) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    // SAFETY: the handle is checked and closed; a gone process gives null.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, timeout.as_millis().min(u32::MAX as u128) as u32);
            CloseHandle(h);
        }
    }
}

/// One command-line argument as `CommandLineToArgvW` reads it back.
fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from('"');
    let mut slashes = 0;
    for ch in arg.chars() {
        match ch {
            '\\' => slashes += 1,
            // the backslashes before it are doubled, plus one for the quote
            '"' => {
                out.extend(std::iter::repeat_n('\\', slashes + 1));
                slashes = 0;
            }
            _ => slashes = 0,
        }
        out.push(ch);
    }
    out.extend(std::iter::repeat_n('\\', slashes));
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn arguments_are_quoted() {
        assert_eq!(quote("--silent"), "--silent");
        assert_eq!(quote(r"C:\Program Files\LauraBoard"), r#""C:\Program Files\LauraBoard""#);
        assert_eq!(quote(r"C:\a b\"), r#""C:\a b\\""#);
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(""), r#""""#);
    }
}
