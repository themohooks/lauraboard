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
use crate::shell::{AppWindow, Stats, SysCard, SysLine, SysProc};

const EVERY: Duration = Duration::from_secs(2);
/// Samples kept for the history charts (2 minutes).
const HISTORY: usize = 60;

/// Mirror of the server's `SystemInfo` (the GUI depends only on the API).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SystemInfo {
    pub cpu_percent: f32,
    pub cpu_threads: usize,
    pub memory_total: u64,
    pub memory_used: u64,
    pub uptime_s: u64,
    pub components: Vec<ProcessInfo>,
    pub traffic: Vec<NetworkInfo>,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProcessInfo {
    pub name: String,
    pub cpu: f32,
    pub memory_bytes: u64,
    pub read_per_s: u64,
    pub write_per_s: u64,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NetworkInfo {
    pub name: String,
    pub rx_per_s: u64,
    pub tx_per_s: u64,
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
                Err(e) => g.set_sys_error(format!("Нет данных о приложении: {}", crate::ru::server(&e)).into()),
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
    let read: u64 = s.components.iter().map(|p| p.read_per_s).sum();
    let written: u64 = s.components.iter().map(|p| p.write_per_s).sum();
    g.set_sys_cards(vec_model(vec![
        card("CPU LauraBoard", format!("{:.1} %", s.cpu_percent), "Программа и все дочерние процессы".into(), cpu),
        card("ОЗУ LauraBoard", bytes(s.memory_used), format!("{:.1} % доступной памяти компьютера", mem * 100.0), mem),
        card("Дисковый ввод / вывод", format!("↓ {}/с", bytes(read)), format!("↑ {}/с", bytes(written)), -1.0),
    ]));
    g.set_sys_cpu_history(vec_model(c.system.cpu.iter().copied().collect()));
    g.set_sys_mem_history(vec_model(c.system.mem.iter().copied().collect()));
    let divisor = s.cpu_threads.max(1) as f32;
    g.set_sys_procs(vec_model(
        s.components
            .iter()
            .map(|p| SysProc {
                name: p.name.clone().into(),
                cpu: format!("{:.1} %", p.cpu / divisor).into(),
                memory: bytes(p.memory_bytes).into(),
                share: (p.cpu / divisor / 100.0).clamp(0.0, 1.0),
                io: format!("↓ {} · ↑ {}/с", bytes(p.read_per_s), bytes(p.write_per_s)).into(),
            })
            .collect(),
    ));
    let mut info = vec![SysLine { label: "Время работы приложения".into(), value: uptime(s.uptime_s).into() }];
    info.extend(
        s.traffic.iter().map(|t| SysLine {
            label: t.name.clone().into(),
            value: format!("↓ {}/с · ↑ {}/с", bytes(t.rx_per_s), bytes(t.tx_per_s)).into(),
        }),
    );
    g.set_sys_info(vec_model(info));
}
