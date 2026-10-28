"""Conservative Cargo settings for standard GitHub runners, public or private."""
import ctypes
import os
from pathlib import Path
import platform
import subprocess

GIB = 1024 ** 3


def memory_bytes():
    system = platform.system()
    if system == "Linux":
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                return int(line.split()[1]) * 1024
    if system == "Darwin":
        return int(subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True))
    if system == "Windows":
        class MemoryStatus(ctypes.Structure):
            _fields_ = [("length", ctypes.c_uint32), ("load", ctypes.c_uint32)] + [
                (name, ctypes.c_uint64) for name in (
                    "total_phys", "avail_phys", "total_page", "avail_page",
                    "total_virtual", "avail_virtual", "avail_extended")]
        status = MemoryStatus()
        status.length = ctypes.sizeof(status)
        if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
            return status.total_phys
    raise RuntimeError("Cannot determine runner memory")


def settings(cpus, memory, desktop=True, x86=False, fast_release=False):
    # Slint's generated Rust can consume several GiB in one compiler process.
    per_job = 6 * GIB if desktop else 2 * GIB
    jobs = 1 if x86 else max(1, min(cpus, memory // per_job, 2 if desktop else 4))
    result = {
        "CARGO_BUILD_JOBS": str(jobs),
        "CARGO_PROFILE_DEV_DEBUG": "0",
        "CARGO_PROFILE_TEST_DEBUG": "0",
        "CARGO_INCREMENTAL": "0",
    }
    if fast_release:
        # Keep opt-level=3, but avoid cross-crate and local LTO on small runners.
        result.update(CARGO_PROFILE_RELEASE_LTO="off", CARGO_PROFILE_RELEASE_CODEGEN_UNITS="16")
    return result


if __name__ == "__main__":
    try:
        memory = memory_bytes()
    except (OSError, RuntimeError, ValueError, AttributeError, subprocess.SubprocessError) as error:
        print(f"Memory detection failed: {error}; using conservative settings")
        memory = 4 * GIB
    values = settings(os.cpu_count() or 1, memory, desktop=os.environ.get("DESKTOP", "true") == "true",
                      x86=os.environ.get("X86") == "true", fast_release=os.environ.get("FAST_RELEASE") == "true")
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as output:
        for key, value in values.items():
            output.write(f"{key}={value}\n")
    print(f"Runner: {platform.system()}/{platform.machine()}, {os.cpu_count()} CPUs, {memory / GIB:.1f} GiB RAM")
    print("Cargo settings:", values)
