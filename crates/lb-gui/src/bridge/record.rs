//! Recording settings dialog on the Sources screen (format, folder, file
//! length and size, retention) and the live recording facts.

use lb_core::Dur;
use lb_core::config::{ChannelConfig, RecordFormat};
use slint::{ComponentHandle, SharedString};

use super::{Ctx, with};
use crate::fmt::bytes;
use crate::model::RecordingView;
use crate::shell::{AppWindow, Src};

const FORMATS: [RecordFormat; 3] = [RecordFormat::Mp4, RecordFormat::Mkv, RecordFormat::Ts];

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Src>();
    g.on_record_edit(|| with(open));
    g.on_record_browse(|| {
        with(|c| {
            let Some(ui) = c.window() else { return };
            if let Some(dir) = rfd::FileDialog::new().set_title("Папка для записей").pick_folder() {
                ui.global::<Src>().set_r_dir(dir.display().to_string().into());
            }
        })
    });
    g.on_record_save(|| with(save));
}

fn open(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let Some(cfg) = super::sources::config(c) else {
        c.toast("Настройки канала недоступны: нужна роль администратора", true);
        return;
    };
    let g = ui.global::<Src>();
    let r = &cfg.record;
    g.set_r_enabled(r.enabled);
    g.set_r_format(FORMATS.iter().position(|f| *f == r.format).unwrap_or(0) as i32);
    g.set_r_dir(r.dir.clone().into());
    g.set_r_segment((r.segment.ms() / 60_000).to_string().into());
    g.set_r_max_mb(opt_num(r.max_file_mb as f64));
    g.set_r_keep_days(opt_num(r.keep_days as f64));
    g.set_r_max_gb(opt_num(r.max_total_gb));
    g.set_r_prefix(r.prefix.clone().into());
    g.set_r_prefix_hint(format!("пусто — «{}»", cfg.id).into());
    g.set_r_error(SharedString::new());
    g.set_dialog("record".into());
}

/// 0 → "" (no limit), 2.5 → "2.5".
fn opt_num(x: f64) -> SharedString {
    if x <= 0.0 {
        return SharedString::new();
    }
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string().into()
}

/// Empty = 0; otherwise a non-negative number.
fn limit(s: &str, what: &str) -> Result<f64, String> {
    let s = s.trim().replace(',', ".");
    if s.is_empty() {
        return Ok(0.0);
    }
    s.parse::<f64>().ok().filter(|v| *v >= 0.0 && v.is_finite()).ok_or_else(|| format!("{what}: нужно число (пусто — без ограничения)"))
}

fn apply(ui: &AppWindow, cfg: &mut ChannelConfig) -> Result<(), String> {
    let g = ui.global::<Src>();
    let r = &mut cfg.record;
    r.enabled = g.get_r_enabled();
    r.format = FORMATS.get(g.get_r_format().max(0) as usize).copied().unwrap_or_default();
    let dir = g.get_r_dir().trim().to_string();
    if dir.is_empty() {
        return Err("Укажите папку для записей".into());
    }
    r.dir = dir;
    let mins = limit(&g.get_r_segment(), "Длина файла")?;
    if !(1.0..=1440.0).contains(&mins) {
        return Err("Длина файла: от 1 до 1440 минут".into());
    }
    r.segment = Dur::mins(mins.round() as i64);
    r.max_file_mb = limit(&g.get_r_max_mb(), "Размер файла")?.round() as u64;
    if r.max_file_mb > 0 && r.max_file_mb < 10 {
        return Err("Размер файла: не меньше 10 МБ".into());
    }
    r.keep_days = limit(&g.get_r_keep_days(), "Хранить дней")?.round() as u32;
    r.max_total_gb = limit(&g.get_r_max_gb(), "Всего на диске")?;
    r.prefix = g.get_r_prefix().trim().to_string();
    Ok(())
}

fn save(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let Some(mut cfg) = super::sources::config(c) else { return };
    let before = cfg.clone();
    if let Err(e) = apply(&ui, &mut cfg) {
        ui.global::<Src>().set_r_error(e.into());
        return;
    }
    ui.global::<Src>().set_dialog(SharedString::new());
    if cfg == before {
        return;
    }
    let commit = move |c: &mut Ctx| {
        c.channel_config = serde_json::to_value(&cfg).ok();
        super::sources::refresh(c);
        c.save_doc("/config", cfg, "Настройки записи");
    };
    if c.model.status.output.running {
        c.ask("Перезапустить кодировщик?", "Запись включается и настраивается через кодировщик — в эфире будет обрыв на несколько секунд.", "Сохранить", commit);
    } else {
        commit(c);
    }
}

/// Facts of the recording card: settings plus what is being written now.
pub fn facts(cfg: &ChannelConfig, live: Option<&RecordingView>) -> Vec<(String, String)> {
    let r = &cfg.record;
    let mut v: Vec<(String, String)> = vec![("Запись".into(), if r.enabled { "включена" } else { "выключена" }.into())];
    if !r.enabled {
        return v;
    }
    let mut how = format!("{}, файлы по {} мин", r.format.extension().to_uppercase(), r.segment.ms() / 60_000);
    if r.max_file_mb > 0 {
        how.push_str(&format!(", до {} МБ", r.max_file_mb));
    }
    v.push(("Формат".into(), how));
    v.push(("Папка".into(), live.map(|l| l.dir.clone()).unwrap_or_else(|| r.dir.clone())));
    let mut keep: Vec<String> = Vec::new();
    if r.keep_days > 0 {
        keep.push(format!("{} дн.", r.keep_days));
    }
    if r.max_total_gb > 0.0 {
        keep.push(format!("не больше {} ГБ", opt_num(r.max_total_gb)));
    }
    v.push(("Хранить".into(), if keep.is_empty() { "всё".into() } else { keep.join(", ") }));
    match live {
        Some(l) => {
            if let Some(f) = &l.file {
                v.push(("Пишется".into(), format!("{f} · {}", bytes(l.file_bytes))));
            }
            v.push(("Всего записей".into(), format!("{} · {}", l.files, bytes(l.total_bytes))));
            if let Some(free) = l.free_bytes {
                v.push(("Свободно на диске".into(), bytes(free)));
            }
        }
        None => v.push(("Сейчас".into(), "не пишется (кодировщик остановлен)".into())),
    }
    v
}
