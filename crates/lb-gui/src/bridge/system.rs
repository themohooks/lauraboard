//! Statistics → "Система": host metrics from `GET /system`, polled every
//! two seconds while the view is open, with a two-minute history of CPU
//! and memory load.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

use serde::Deserialize;
use slint::{ComponentHandle, SharedString};

use super::{Ctx, vec_model, with};
use crate::fmt::bytes;
use crate::net::Msg;
use crate::shell::{AppWindow, Stats, SysCard, SysDisk, SysLine, SysProc};

const EVERY: Duration = Duration::from_secs(2);
/// Samples kept for the history charts (2 minutes).
const HISTORY: usize = 60;

/// Mirror of the server's `SystemInfo` (the GUI depends only on the API).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SystemInfo {
    pub host: String,
    pub os: String,
    pub arch: String,
    pub cpu_brand: String,
    pub cpu_cores: usize,
    pub cpu_threads: usize,
    pub cpu_mhz: u64,
    pub cpu_percent: f32,
    pub cpu_per_core: Vec<f32>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub uptime_s: u64,
    pub disks: Vec<DiskInfo>,
    pub net_rx_per_s: u64,
    pub net_tx_per_s: u64,
    pub app: ProcessInfo,
    pub ffmpeg: Vec<ProcessInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DiskInfo {
    pub mount: String,
    pub name: String,
    pub file_system: String,
    pub kind: String,
    pub removable: bool,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub read_per_s: u64,
    pub write_per_s: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub memory_bytes: u64,
}

#[derive(Default)]
pub struct SystemState {
    cpu: VecDeque<f32>,
    mem: VecDeque<f32>,
    busy: bool,
}

thread_local! {
    static TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

pub fn connect(ui: &AppWindow) {
    ui.global::<Stats>().on_view_changed(|| with(opened));
}

/// The statistics screen or its view changed: poll while "Система" shows.
pub fn opened(c: &mut Ctx) {
    let on = c.section() == "stats" && c.window().is_some_and(|ui| ui.global::<Stats>().get_view() == 1);
    TIMER.with(|t| {
        let mut t = t.borrow_mut();
        if !on {
            *t = None;
            return;
        }
        if t.is_none() {
            let timer = slint::Timer::default();
            timer.start(slint::TimerMode::Repeated, EVERY, || with(poll));
            *t = Some(timer);
        }
    });
    if on {
        poll(c);
    }
}

fn poll(c: &mut Ctx) {
    if c.section() != "stats" {
        TIMER.with(|t| *t.borrow_mut() = None);
        return;
    }
    if std::mem::replace(&mut c.system.busy, true) {
        return;
    }
    c.worker.spawn(|api| {
        let r: Result<SystemInfo, String> = api.get("/system");
        Ok(Msg::Apply(Box::new(move |c: &mut Ctx| {
            c.system.busy = false;
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Stats>();
            match r {
                Ok(info) => {
                    g.set_sys_error(SharedString::new());
                    show(c, &info);
                }
                Err(e) => g.set_sys_error(format!("Нет данных о системе: {}", crate::ru::server(&e)).into()),
            }
        })))
    });
}

fn share(part: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { (part as f64 / total as f64) as f32 }
}

fn uptime(s: u64) -> String {
    let (d, h, m) = (s / 86_400, s % 86_400 / 3600, s % 3600 / 60);
    if d > 0 { format!("{d} д {h} ч") } else { format!("{h} ч {m} мин") }
}

fn push(q: &mut VecDeque<f32>, v: f32) {
    q.push_back(v.clamp(0.0, 1.0));
    while q.len() > HISTORY {
        q.pop_front();
    }
}

fn show(c: &mut Ctx, s: &SystemInfo) {
    let Some(ui) = c.window() else { return };
    let g = ui.global::<Stats>();
    let cpu = s.cpu_percent / 100.0;
    let mem = share(s.memory_used, s.memory_total);
    push(&mut c.system.cpu, cpu);
    push(&mut c.system.mem, mem);
    let card = |label: &str, value: String, detail: String, share: f32| SysCard {
        label: label.into(),
        value: value.into(),
        detail: detail.into(),
        share,
    };
    let threads = s.cpu_threads.max(1) as f32;
    let ours = s.app.cpu + s.ffmpeg.iter().map(|p| p.cpu).sum::<f32>();
    let mut cards = vec![
        card("Процессор", format!("{:.0} %", s.cpu_percent), format!("LauraBoard с FFmpeg: {:.0} %", ours / threads), cpu),
        card("Память", format!("{:.0} %", mem * 100.0), format!("{} из {}", bytes(s.memory_used), bytes(s.memory_total)), mem),
    ];
    if s.swap_total > 0 {
        let sw = share(s.swap_used, s.swap_total);
        cards.push(card("Файл подкачки", format!("{:.0} %", sw * 100.0), format!("{} из {}", bytes(s.swap_used), bytes(s.swap_total)), sw));
    }
    cards.push(card("Сеть", format!("↓ {}/с", bytes(s.net_rx_per_s)), format!("↑ {}/с", bytes(s.net_tx_per_s)), -1.0));
    cards.push(card("Работает без перезагрузки", uptime(s.uptime_s), s.host.clone(), -1.0));
    g.set_sys_cards(vec_model(cards));
    g.set_sys_cores(vec_model(s.cpu_per_core.iter().map(|v| v / 100.0).collect()));
    g.set_sys_cpu_history(vec_model(c.system.cpu.iter().copied().collect()));
    g.set_sys_mem_history(vec_model(c.system.mem.iter().copied().collect()));

    let mut disks: Vec<&DiskInfo> = s.disks.iter().filter(|d| d.total_bytes > 0).collect();
    disks.sort_by(|a, b| a.mount.cmp(&b.mount));
    disks.dedup_by(|a, b| a.mount == b.mount);
    g.set_sys_disks(vec_model(
        disks
            .iter()
            .map(|d| {
                let kind = match d.kind.as_str() {
                    "ssd" => " · SSD",
                    "hdd" => " · HDD",
                    _ => "",
                };
                SysDisk {
                    mount: d.mount.clone().into(),
                    detail: format!(
                        "свободно {} из {}{kind}{}{}",
                        bytes(d.free_bytes),
                        bytes(d.total_bytes),
                        if d.file_system.is_empty() { String::new() } else { format!(" · {}", d.file_system) },
                        if d.removable { " · съёмный" } else { "" }
                    )
                    .into(),
                    share: 1.0 - share(d.free_bytes, d.total_bytes),
                    io: format!("чт {}/с · зап {}/с", bytes(d.read_per_s), bytes(d.write_per_s)).into(),
                }
            })
            .collect(),
    ));

    let proc_row = |p: &ProcessInfo, label: String| SysProc {
        name: label.into(),
        cpu: format!("{:.0} %", p.cpu / threads).into(),
        memory: bytes(p.memory_bytes).into(),
        share: (p.cpu / 100.0 / threads).clamp(0.0, 1.0),
    };
    let mut procs = vec![proc_row(&s.app, format!("LauraBoard (pid {})", s.app.pid))];
    procs.extend(s.ffmpeg.iter().map(|p| proc_row(p, format!("ffmpeg (pid {})", p.pid))));
    g.set_sys_procs(vec_model(procs));

    let line = |l: &str, v: String| SysLine { label: l.into(), value: v.into() };
    g.set_sys_info(vec_model(vec![
        line("Имя компьютера", s.host.clone()),
        line("Система", format!("{} · {}", s.os, s.arch)),
        line("Процессор", format!("{} · {} ядер, {} потоков · {} МГц", s.cpu_brand, s.cpu_cores, s.cpu_threads, s.cpu_mhz)),
        line("Память", bytes(s.memory_total)),
        line("Процессов FFmpeg", s.ffmpeg.len().to_string()),
    ]));
}
