//! Host metrics for the statistics screen: CPU, memory, disks, network,
//! uptime, and the program's own processes (Laurabrod and its FFmpeg
//! children). Rates (CPU %, disk and network throughput) are measured
//! between two snapshots, so the monitor is kept and asked repeatedly.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, Networks, Pid, ProcessesToUpdate, System};

/// Snapshots closer together than this reuse the previous one (CPU usage
/// needs some time between measurements to mean anything).
const MIN_INTERVAL: Duration = Duration::from_millis(900);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DiskInfo {
    /// Mount point (`C:\`, `/`).
    pub mount: String,
    pub name: String,
    pub file_system: String,
    /// `ssd`, `hdd` or `unknown`.
    pub kind: String,
    pub removable: bool,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Throughput since the previous snapshot (bytes/s).
    pub read_per_s: u64,
    pub write_per_s: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    /// Percent of one core (may exceed 100 on several cores).
    pub cpu: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SystemInfo {
    pub host: String,
    pub os: String,
    pub arch: String,
    pub cpu_brand: String,
    pub cpu_cores: usize,
    pub cpu_threads: usize,
    /// MHz of the first core.
    pub cpu_mhz: u64,
    /// Whole machine, 0…100.
    pub cpu_percent: f32,
    /// Per logical core, 0…100.
    pub cpu_per_core: Vec<f32>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub uptime_s: u64,
    pub disks: Vec<DiskInfo>,
    /// All network interfaces together (bytes/s).
    pub net_rx_per_s: u64,
    pub net_tx_per_s: u64,
    /// Laurabrod itself.
    pub app: ProcessInfo,
    /// FFmpeg processes started by Laurabrod (decoders, encoders).
    pub ffmpeg: Vec<ProcessInfo>,
}

pub struct SystemMonitor {
    sys: System,
    disks: Disks,
    nets: Networks,
    last: Option<(Instant, SystemInfo)>,
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        SystemMonitor { sys, disks: Disks::new_with_refreshed_list(), nets: Networks::new_with_refreshed_list(), last: None }
    }

    /// Current metrics (rates since the previous call).
    pub fn snapshot(&mut self) -> SystemInfo {
        if let Some((at, info)) = &self.last
            && at.elapsed() < MIN_INTERVAL
        {
            return info.clone();
        }
        let secs = self.last.as_ref().map(|(at, _)| at.elapsed().as_secs_f64()).unwrap_or(1.0).max(0.001);
        let per_s = |bytes: u64| (bytes as f64 / secs) as u64;
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.disks.refresh(true);
        self.nets.refresh(true);

        let cpus = self.sys.cpus();
        let me = sysinfo::get_current_pid().ok();
        let proc_info = |pid: Pid, p: &sysinfo::Process| ProcessInfo {
            pid: pid.as_u32(),
            name: p.name().to_string_lossy().into_owned(),
            cpu: p.cpu_usage(),
            memory_bytes: p.memory(),
        };
        let app = me.and_then(|pid| self.sys.process(pid).map(|p| proc_info(pid, p))).unwrap_or_default();
        // FFmpeg started by us: direct children named ffmpeg
        let mut ffmpeg: Vec<ProcessInfo> = self
            .sys
            .processes()
            .iter()
            .filter(|(_, p)| me.is_some() && p.parent() == me)
            .filter(|(_, p)| p.name().to_string_lossy().to_ascii_lowercase().starts_with("ffmpeg"))
            .map(|(pid, p)| proc_info(*pid, p))
            .collect();
        ffmpeg.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));

        let disks = self
            .disks
            .list()
            .iter()
            .map(|d| {
                let u = d.usage();
                DiskInfo {
                    mount: d.mount_point().display().to_string(),
                    name: d.name().to_string_lossy().into_owned(),
                    file_system: d.file_system().to_string_lossy().into_owned(),
                    kind: match d.kind() {
                        sysinfo::DiskKind::SSD => "ssd".into(),
                        sysinfo::DiskKind::HDD => "hdd".into(),
                        _ => "unknown".into(),
                    },
                    removable: d.is_removable(),
                    total_bytes: d.total_space(),
                    free_bytes: d.available_space(),
                    read_per_s: per_s(u.read_bytes),
                    write_per_s: per_s(u.written_bytes),
                }
            })
            .collect();
        let (rx, tx) = self.nets.iter().fold((0u64, 0u64), |(r, t), (_, n)| (r + n.received(), t + n.transmitted()));

        let info = SystemInfo {
            host: System::host_name().unwrap_or_default(),
            os: System::long_os_version().unwrap_or_else(|| System::name().unwrap_or_default()),
            arch: System::cpu_arch(),
            cpu_brand: cpus.first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
            cpu_cores: System::physical_core_count().unwrap_or(cpus.len()),
            cpu_threads: cpus.len(),
            cpu_mhz: cpus.first().map(|c| c.frequency()).unwrap_or(0),
            cpu_percent: self.sys.global_cpu_usage(),
            cpu_per_core: cpus.iter().map(|c| c.cpu_usage()).collect(),
            memory_total: self.sys.total_memory(),
            memory_used: self.sys.used_memory(),
            swap_total: self.sys.total_swap(),
            swap_used: self.sys.used_swap(),
            uptime_s: System::uptime(),
            disks,
            net_rx_per_s: per_s(rx),
            net_tx_per_s: per_s(tx),
            app,
            ffmpeg,
        };
        self.last = Some((Instant::now(), info.clone()));
        info
    }
}

/// Process-wide monitor (rates need the previous snapshot).
pub fn snapshot() -> SystemInfo {
    static MONITOR: Mutex<Option<SystemMonitor>> = Mutex::new(None);
    let mut m = MONITOR.lock().unwrap_or_else(|e| e.into_inner());
    m.get_or_insert_with(SystemMonitor::new).snapshot()
}

/// Free space on the disk holding `path` (the longest matching mount).
pub fn free_space(path: &Path) -> Option<u64> {
    let abs = std::fs::canonicalize(path).ok()?;
    // Windows: \\?\D:\x → D:\x
    let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase();
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| abs.starts_with(&d.mount_point().to_string_lossy().to_lowercase()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_has_basics() {
        let mut m = SystemMonitor::new();
        let a = m.snapshot();
        assert!(a.memory_total > 0);
        assert!(a.cpu_threads > 0);
        assert!(a.app.pid > 0);
        assert!(free_space(Path::new(".")).is_some());
    }
}
