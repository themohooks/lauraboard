//! Native crash capture in the supervisor, outside the potentially corrupted
//! process. Only last-chance exceptions are fatal; handled SEH is left alone.
use std::io::{self, Write};
use std::os::windows::{ffi::OsStrExt, io::AsRawHandle, process::CommandExt};
use std::path::Path;
use std::process::{Command, ExitStatus};

use windows_sys::Win32::Foundation::{CloseHandle, DBG_CONTINUE, DBG_EXCEPTION_NOT_HANDLED, HANDLE, LocalFree};
use windows_sys::Win32::System::Diagnostics::Debug::*;
use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQueryEx};
use windows_sys::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
use windows_sys::Win32::System::Threading::{
    DEBUG_ONLY_THIS_PROCESS, GetThreadDescription, OpenThread, THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION,
};

pub fn status(command: &mut Command, log: &Path, desktop: bool) -> io::Result<ExitStatus> {
    // DEBUG_ONLY_THIS_PROCESS keeps FFmpeg and browser helpers independent.
    command.creation_flags(DEBUG_ONLY_THIS_PROCESS | if desktop { 0x08000000 } else { 0 });
    let mut child = command.spawn()?;
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(log) {
        let _ = writeln!(file, "PID приложения: {}\n", child.id());
    }
    let process = child.as_raw_handle() as HANDLE;
    let mut initial_breakpoint = true;
    loop {
        let mut event = DEBUG_EVENT::default();
        // Must run on the same OS thread that called CreateProcess.
        if unsafe { WaitForDebugEventEx(&mut event, u32::MAX) } == 0 {
            let error = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let mut continuation = DBG_CONTINUE;
        let exited = event.dwDebugEventCode == EXIT_PROCESS_DEBUG_EVENT;
        match event.dwDebugEventCode {
            EXCEPTION_DEBUG_EVENT => {
                // SAFETY: the event tag selects the active union member.
                let exception = unsafe { event.u.Exception };
                let code = exception.ExceptionRecord.ExceptionCode as u32;
                if initial_breakpoint && code == 0x80000003 && exception.dwFirstChance != 0 {
                    initial_breakpoint = false;
                } else {
                    continuation = DBG_EXCEPTION_NOT_HANDLED;
                    if exception.dwFirstChance == 0 {
                        if let Err(error) = report(process, event.dwProcessId, event.dwThreadId, exception.ExceptionRecord, log) {
                            if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(log) {
                                let _ = writeln!(file, "Не удалось полностью сохранить диагностику: {error}");
                                let _ = file.sync_data();
                            }
                        }
                    }
                }
            }
            CREATE_PROCESS_DEBUG_EVENT => {
                // Windows owns the event's process/thread handles and closes
                // them on exit; the debugger owns only the image-file handle.
                let file = unsafe { event.u.CreateProcessInfo.hFile };
                if !file.is_null() {
                    unsafe {
                        CloseHandle(file);
                    }
                }
            }
            LOAD_DLL_DEBUG_EVENT => {
                let file = unsafe { event.u.LoadDll.hFile };
                if !file.is_null() {
                    unsafe {
                        CloseHandle(file);
                    }
                }
            }
            _ => {}
        }
        if unsafe { ContinueDebugEvent(event.dwProcessId, event.dwThreadId, continuation) } == 0 {
            let error = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        if exited {
            return child.wait();
        }
    }
}

fn exception_name(code: u32) -> &'static str {
    match code {
        0xc0000005 => "нарушение доступа к памяти (ACCESS_VIOLATION)",
        0xc00000fd => "переполнение стека (STACK_OVERFLOW)",
        0xc0000374 => "повреждение кучи (HEAP_CORRUPTION)",
        0xc0000409 => "аварийная остановка защиты процесса (FAIL_FAST)",
        0xc000001d => "недопустимая инструкция (ILLEGAL_INSTRUCTION)",
        0xc0000094 => "целочисленное деление на ноль",
        0x80000003 => "необработанная точка останова",
        _ => "необработанное системное исключение",
    }
}

struct Thread(HANDLE);
impl Drop for Thread {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn report(process: HANDLE, pid: u32, tid: u32, mut record: EXCEPTION_RECORD, log: &Path) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new().append(true).open(log)?;
    let code = record.ExceptionCode as u32;
    writeln!(file, "\nНативный сбой: 0x{code:08X} — {}", exception_name(code))?;
    writeln!(file, "Процесс: {pid}; сбойный поток: {tid}")?;
    writeln!(file, "Адрес исключения: {}", location(process, record.ExceptionAddress as u64))?;
    if code == 0xc0000005 && record.NumberParameters >= 2 {
        let operation = match record.ExceptionInformation[0] {
            0 => "чтение",
            1 => "запись",
            8 => "исполнение",
            _ => "доступ",
        };
        writeln!(file, "Операция: {operation}; адрес памяти: 0x{:016X}", record.ExceptionInformation[1])?;
    }
    if record.NumberParameters > 0 {
        writeln!(file, "Параметры исключения: {:X?}", &record.ExceptionInformation[..(record.NumberParameters as usize).min(15)])?;
    }
    // Preserve the minimal report before asking DbgHelp to inspect memory.
    file.sync_data()?;
    let thread = Thread(unsafe { OpenThread(THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, 0, tid) });
    if thread.0.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut description = std::ptr::null_mut();
    if unsafe { GetThreadDescription(thread.0, &mut description) } >= 0 && !description.is_null() {
        let mut len = 0;
        unsafe {
            while *description.add(len) != 0 {
                len += 1;
            }
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(description, len));
            let _ = writeln!(file, "Имя потока: {name}");
            LocalFree(description.cast());
        }
    }
    let mut context = Box::<CONTEXT>::default();
    #[cfg(target_arch = "x86_64")]
    {
        context.ContextFlags = CONTEXT_FULL_AMD64;
    }
    #[cfg(target_arch = "x86")]
    {
        context.ContextFlags = CONTEXT_FULL_X86;
    }
    #[cfg(target_arch = "aarch64")]
    {
        context.ContextFlags = CONTEXT_FULL_ARM64;
    }
    let have_context = unsafe { GetThreadContext(thread.0, &mut *context) } != 0;
    if !have_context {
        writeln!(file, "Регистры потока недоступны: {}", io::Error::last_os_error())?;
    }
    let dump = log.with_extension("dmp");
    let result = write_dump(process, pid, tid, &dump, have_context.then_some((&mut record, &mut *context)));
    match result {
        Ok(()) => writeln!(file, "Дамп аварии: {}", dump.display())?,
        Err(error) => writeln!(file, "Не удалось сохранить дамп {}: {error}", dump.display())?,
    }
    if have_context {
        writeln!(file, "Стек сбойного потока (модуль + смещение; символы зависят от наличия PDB):")?;
        stack(process, thread.0, &mut context, &mut file)?;
    }
    file.sync_data()
}

fn write_dump(process: HANDLE, pid: u32, tid: u32, path: &Path, context: Option<(&mut EXCEPTION_RECORD, &mut CONTEXT)>) -> io::Result<()> {
    let file = std::fs::File::create(path)?;
    let have_context = context.is_some();
    let mut pointers = context.map_or_else(EXCEPTION_POINTERS::default, |(record, context)| EXCEPTION_POINTERS {
        ExceptionRecord: record,
        ContextRecord: context,
    });
    let exception = MINIDUMP_EXCEPTION_INFORMATION { ThreadId: tid, ExceptionPointers: &mut pointers, ClientPointers: 0 };
    let exception = if have_context { &exception } else { std::ptr::null() };
    let ok = unsafe {
        MiniDumpWriteDump(
            process,
            pid,
            file.as_raw_handle() as HANDLE,
            MiniDumpWithThreadInfo | MiniDumpWithUnloadedModules,
            exception,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    file.sync_all()
}

fn location(process: HANDLE, address: u64) -> String {
    let mut info = MEMORY_BASIC_INFORMATION::default();
    let mut name = [0u16; 1024];
    let queried = unsafe { VirtualQueryEx(process, address as *const _, &mut info, std::mem::size_of_val(&info)) };
    if queried != 0 {
        let len = unsafe { K32GetModuleFileNameExW(process, info.AllocationBase, name.as_mut_ptr(), name.len() as u32) };
        if len != 0 {
            return format!(
                "0x{address:016X} — {} + 0x{:X}",
                String::from_utf16_lossy(&name[..len as usize]),
                address.saturating_sub(info.AllocationBase as u64)
            );
        }
    }
    format!("0x{address:016X} — модуль не определён")
}

fn stack(process: HANDLE, thread: HANDLE, context: &mut CONTEXT, out: &mut impl Write) -> io::Result<()> {
    unsafe {
        SymSetOptions(SYMOPT_DEFERRED_LOADS | SYMOPT_FAIL_CRITICAL_ERRORS | SYMOPT_NO_PROMPTS | SYMOPT_UNDNAME | SYMOPT_LOAD_LINES);
    }
    // Search local build symbols only; a crash report must not wait for a
    // network symbol server inherited through _NT_SYMBOL_PATH.
    let exe = std::env::current_exe()?;
    let search: Vec<u16> = exe.parent().unwrap_or(Path::new(".")).as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { SymInitializeW(process, search.as_ptr(), 1) } == 0 {
        return writeln!(out, "Стек недоступен: {}. Контекст сохранён в дампе.", io::Error::last_os_error());
    }
    let mut frame = STACKFRAME64::default();
    #[cfg(target_arch = "x86_64")]
    let (machine, pc, sp, fp) = (0x8664, context.Rip, context.Rsp, context.Rbp);
    #[cfg(target_arch = "x86")]
    let (machine, pc, sp, fp) = (0x014c, context.Eip as u64, context.Esp as u64, context.Ebp as u64);
    #[cfg(target_arch = "aarch64")]
    let (machine, pc, sp, fp) = (0xaa64, context.Pc, context.Sp, context.Sp);
    frame.AddrPC = ADDRESS64 { Offset: pc, Mode: AddrModeFlat, Segment: 0 };
    frame.AddrStack = ADDRESS64 { Offset: sp, Mode: AddrModeFlat, Segment: 0 };
    frame.AddrFrame = ADDRESS64 { Offset: fp, Mode: AddrModeFlat, Segment: 0 };
    let mut previous = (0, 0);
    for i in 0..64 {
        if unsafe {
            StackWalk64(
                machine,
                process,
                thread,
                &mut frame,
                (context as *mut CONTEXT).cast(),
                None,
                Some(SymFunctionTableAccess64),
                Some(SymGetModuleBase64),
                None,
            )
        } == 0
        {
            break;
        }
        let current = (frame.AddrPC.Offset, frame.AddrStack.Offset);
        if current.0 == 0 || current == previous {
            break;
        }
        writeln!(out, "  #{i:02} {}{}", location(process, current.0), symbol(process, current.0))?;
        previous = current;
    }
    unsafe {
        SymCleanup(process);
    }
    Ok(())
}

fn symbol(process: HANDLE, address: u64) -> String {
    // SYMBOL_INFO ends in a variable-length name; provide aligned trailing
    // storage for DbgHelp rather than writing beyond a standalone struct.
    let mut storage = [0u64; (std::mem::size_of::<SYMBOL_INFO>() + 256).div_ceil(8)];
    let info = storage.as_mut_ptr().cast::<SYMBOL_INFO>();
    let mut displacement = 0;
    unsafe {
        (*info).SizeOfStruct = std::mem::size_of::<SYMBOL_INFO>() as u32;
        (*info).MaxNameLen = 256;
        if SymFromAddr(process, address, &mut displacement, info) == 0 {
            return String::new();
        }
        let name = std::slice::from_raw_parts((*info).Name.as_ptr().cast::<u8>(), (*info).NameLen.min(255) as usize);
        let mut text = format!(" — {} + 0x{displacement:X}", String::from_utf8_lossy(name));
        let mut line = IMAGEHLP_LINE64 { SizeOfStruct: std::mem::size_of::<IMAGEHLP_LINE64>() as u32, ..Default::default() };
        let mut offset = 0;
        if SymGetLineFromAddr64(process, address, &mut offset, &mut line) != 0 && !line.FileName.is_null() {
            text.push_str(&format!(" ({}:{})", std::ffi::CStr::from_ptr(line.FileName.cast()).to_string_lossy(), line.LineNumber));
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_exception_preserves_module_context_and_dump() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("session.log");
        std::fs::write(&log, "previous entry\n").unwrap();
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", &format!("{}::exception_child", module_path!().split_once("::").unwrap().1), "--nocapture"])
            .env("LAURABOARD_TEST_NATIVE_CRASH", "1");
        let exit = status(&mut cmd, &log, true).unwrap();
        assert_eq!(exit.code().unwrap() as u32, 0xc0000005);
        let report = std::fs::read_to_string(&log).unwrap();
        assert!(report.starts_with("previous entry\n"));
        assert!(report.contains("ACCESS_VIOLATION"), "{report}");
        assert!(report.contains("KERNELBASE.dll") || report.contains("kernelbase.dll"), "{report}");
        assert!(report.contains("Имя потока: native-crash-test"), "{report}");
        assert!(report.contains("Операция: запись"), "{report}");
        assert!(report.contains("#00"), "{report}");
        assert!(report.contains("#02"), "{report}");
        assert!(report.contains("Дамп аварии:"), "{report}");
        let bytes = std::fs::read(log.with_extension("dmp")).unwrap();
        assert!(bytes.starts_with(b"MDMP"));
        let u32_at = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let streams = u32_at(8) as usize;
        let directory = u32_at(12) as usize;
        let exception = (0..streams).map(|i| directory + 12 * i).find(|&entry| u32_at(entry) == 6).expect("exception stream");
        let rva = u32_at(exception + 8) as usize;
        assert_eq!(u32_at(rva + 8), 0xc0000005);
        assert!(u32_at(rva + 160) > 0, "faulting thread register context must be present");
    }

    #[test]
    fn exception_child() {
        let Some(mode) = std::env::var_os("LAURABOARD_TEST_NATIVE_CRASH") else {
            return;
        };
        if mode == "handled" {
            unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
                if unsafe { (*(*info).ExceptionRecord).ExceptionCode as u32 } == 0xe0424242 {
                    EXCEPTION_CONTINUE_EXECUTION
                } else {
                    EXCEPTION_CONTINUE_SEARCH
                }
            }
            unsafe {
                let registration = AddVectoredExceptionHandler(1, Some(handler));
                assert!(!registration.is_null());
                RaiseException(0xe0424242, 0, 0, std::ptr::null());
                RemoveVectoredExceptionHandler(registration);
            }
            return;
        }
        std::thread::Builder::new()
            .name("native-crash-test".into())
            .spawn(|| unsafe {
                RaiseException(0xc0000005, 1, 2, [1usize, 0x1234].as_ptr());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn normal_exit_does_not_create_a_dump() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("session.log");
        std::fs::write(&log, "").unwrap();
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", &format!("{}::exception_child", module_path!().split_once("::").unwrap().1)])
            .env_remove("LAURABOARD_TEST_NATIVE_CRASH");
        assert!(status(&mut cmd, &log, true).unwrap().success());
        assert!(!log.with_extension("dmp").exists());
    }

    #[test]
    fn handled_native_exception_does_not_create_a_crash_report() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("session.log");
        std::fs::write(&log, "").unwrap();
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", &format!("{}::exception_child", module_path!().split_once("::").unwrap().1)])
            .env("LAURABOARD_TEST_NATIVE_CRASH", "handled");
        assert!(status(&mut cmd, &log, true).unwrap().success());
        assert!(!log.with_extension("dmp").exists());
        assert!(!std::fs::read_to_string(log).unwrap().contains("Нативный сбой"));
    }
}
