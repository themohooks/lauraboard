//! Recording settings dialog on the Sources screen (format, folder, file
//! length and size, retention) and the live recording facts.

use lb_core::Dur;
use lb_core::config::{ChannelConfig, RecordAudioCodec, RecordConfig, RecordFormat, RecordVideoCodec};
use slint::{ComponentHandle, Model, SharedString};

use super::{Ctx, vec_model, with};
use crate::fmt::bytes;
use crate::model::RecordingView;
use crate::shell::{AppWindow, Src};

const FORMATS: [RecordFormat; 10] = RecordFormat::ALL;
const HEIGHTS: [u32; 8] = [0, 2160, 1440, 1080, 720, 576, 480, 360];
const RATES: [u32; 6] = [0, 22050, 32000, 44100, 48000, 96000];
const PRESETS: [&str; 7] = ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow"];
const VCODECS: [RecordVideoCodec; 3] = [RecordVideoCodec::Copy, RecordVideoCodec::H264, RecordVideoCodec::H265];
const ACODECS: [RecordAudioCodec; 6] = [
    RecordAudioCodec::Copy,
    RecordAudioCodec::Aac,
    RecordAudioCodec::Mp3,
    RecordAudioCodec::Opus,
    RecordAudioCodec::Flac,
    RecordAudioCodec::Pcm,
];

fn vlabel(c: RecordVideoCodec) -> &'static str {
    match c {
        RecordVideoCodec::Copy => "Как в эфире",
        RecordVideoCodec::H264 => "H.264 (libx264)",
        RecordVideoCodec::H265 => "H.265 / HEVC (libx265)",
    }
}

fn alabel(c: RecordAudioCodec) -> &'static str {
    match c {
        RecordAudioCodec::Copy => "Как в эфире",
        RecordAudioCodec::Aac => "AAC",
        RecordAudioCodec::Mp3 => "MP3 (LAME)",
        RecordAudioCodec::Opus => "Opus",
        RecordAudioCodec::Flac => "FLAC — без потерь",
        RecordAudioCodec::Pcm => "PCM — без сжатия",
    }
}

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Src>();
    g.on_record_edit(|| with(open));
    g.on_record_browse(|| {
        with(|c| {
            let Some(ui) = c.window() else { return };
            if let Some(dir) = crate::file_dialog::FileDialog::new().set_title("Папка для записей").pick_folder() {
                ui.global::<Src>().set_r_dir(dir.display().to_string().into());
            }
        })
    });
    g.on_record_save(|| with(save));
    g.on_record_format_changed(|| {
        with(|c| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Src>();
            let (v, a) = (picked_video(&g), picked_audio(&g));
            codecs(&g, format(&g), v, a);
        })
    });
    g.on_record_acodec_changed(|| {
        with(|c| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Src>();
            audio_flags(&g, picked_audio(&g));
        })
    });
}

fn format(g: &Src) -> RecordFormat {
    FORMATS.get(g.get_r_format().max(0) as usize).copied().unwrap_or_default()
}

/// The codec chosen in the list (by its label: the lists follow the format).
fn picked_video(g: &Src) -> RecordVideoCodec {
    let label = g.get_r_vcodecs().row_data(g.get_r_vcodec().max(0) as usize).unwrap_or_default();
    VCODECS.into_iter().find(|c| vlabel(*c) == label.as_str()).unwrap_or_default()
}

fn picked_audio(g: &Src) -> RecordAudioCodec {
    let label = g.get_r_acodecs().row_data(g.get_r_acodec().max(0) as usize).unwrap_or_default();
    ACODECS.into_iter().find(|c| alabel(*c) == label.as_str()).unwrap_or_default()
}

/// Codec lists of format `f`, keeping the choice when it fits.
fn codecs(g: &Src, f: RecordFormat, v: RecordVideoCodec, a: RecordAudioCodec) {
    let vs = f.video_codecs();
    g.set_r_audio_only(f.audio_only());
    g.set_r_vcodecs(vec_model(vs.iter().map(|c| SharedString::from(vlabel(*c))).collect()));
    g.set_r_vcodec(vs.iter().position(|c| *c == v).unwrap_or(0) as i32);
    let auds = f.audio_codecs();
    g.set_r_acodecs(vec_model(auds.iter().map(|c| SharedString::from(alabel(*c))).collect()));
    let ai = auds.iter().position(|c| *c == a).unwrap_or(0);
    g.set_r_acodec(ai as i32);
    audio_flags(g, auds[ai]);
}

fn audio_flags(g: &Src, a: RecordAudioCodec) {
    g.set_r_a_copy(a == RecordAudioCodec::Copy);
    g.set_r_a_has_bitrate(a.has_bitrate());
    g.set_r_a_has_bits(a.has_bits());
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
    codecs(&g, r.format, r.video.codec, r.audio.codec);
    g.set_r_vmode(if r.video.bitrate.trim().is_empty() { 0 } else { 1 });
    g.set_r_crf(r.video.crf.to_string().into());
    g.set_r_vbitrate(r.video.bitrate.clone().into());
    g.set_r_preset(PRESETS.iter().position(|p| *p == r.video.preset).unwrap_or(2) as i32);
    g.set_r_height(HEIGHTS.iter().position(|h| *h == r.video.height).unwrap_or(0) as i32);
    g.set_r_abitrate(r.audio.bitrate.clone().into());
    g.set_r_arate(RATES.iter().position(|x| *x == r.audio.sample_rate).unwrap_or(0) as i32);
    g.set_r_achannels(r.audio.channels.min(2) as i32);
    g.set_r_bits(if r.audio.bits >= 24 { 1 } else { 0 });
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
    r.format = format(&g);
    apply_codecs(&g, r)?;
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

/// Picture and sound of the dialog → `r`. Settings of codecs not chosen
/// stay as they were.
fn apply_codecs(g: &Src, r: &mut RecordConfig) -> Result<(), String> {
    if !r.format.audio_only() {
        r.video.codec = picked_video(g);
    }
    if r.video.codec != RecordVideoCodec::Copy {
        r.video.height = HEIGHTS.get(g.get_r_height().max(0) as usize).copied().unwrap_or(0);
        r.video.preset = PRESETS.get(g.get_r_preset().max(0) as usize).unwrap_or(&"veryfast").to_string();
        if g.get_r_vmode() == 1 {
            let b = g.get_r_vbitrate().trim().to_string();
            if lb_core::config::parse_rate(&b).is_none_or(|v| v < 100_000.0) {
                return Err("Битрейт видео: например 4000k или 6M (не меньше 100k)".into());
            }
            r.video.bitrate = b;
        } else {
            r.video.bitrate.clear();
            let crf = g.get_r_crf().trim().to_string();
            r.video.crf = if crf.is_empty() {
                23
            } else {
                crf.parse::<u32>().ok().filter(|v| *v <= 51).ok_or("Качество: число от 0 до 51")?
            };
        }
    }
    r.audio.codec = picked_audio(g);
    if r.audio.codec.has_bitrate() {
        let b = g.get_r_abitrate().trim().to_string();
        let b = if b.is_empty() { "192k".to_string() } else { b };
        if lb_core::config::parse_rate(&b).is_none_or(|v| !(8_000.0..=640_000.0).contains(&v)) {
            return Err("Битрейт звука: от 8k до 640k, например 192k".into());
        }
        r.audio.bitrate = b;
    }
    if r.audio.codec.has_bits() {
        r.audio.bits = if g.get_r_bits() == 1 { 24 } else { 16 };
    }
    if r.audio.codec != RecordAudioCodec::Copy {
        r.audio.sample_rate = RATES.get(g.get_r_arate().max(0) as usize).copied().unwrap_or(0);
        r.audio.channels = g.get_r_achannels().clamp(0, 2) as u32;
    }
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
    // the recorder runs apart from the encoder: the broadcast goes on
    c.channel_config = serde_json::to_value(&cfg).ok();
    super::sources::refresh(c);
    c.save_doc("/config", cfg, "Настройки записи");
}

/// Facts of the recording card: settings plus what is being written now.
pub fn facts(cfg: &ChannelConfig, live: Option<&RecordingView>) -> Vec<(String, String)> {
    let r = &cfg.record;
    let mut v: Vec<(String, String)> = vec![("Запись".into(), if r.enabled { "включена" } else { "выключена" }.into())];
    if !r.enabled {
        return v;
    }
    let mut how = format!("{}, файлы по {} мин", r.format.extension().to_uppercase(), r.segment.ms() / 60_000);
    if let Some(v) = r.video_codec().filter(|v| *v != RecordVideoCodec::Copy) {
        how.push_str(&format!(", {}", vlabel(v)));
        if r.video.height > 0 {
            how.push_str(&format!(" {}p", r.video.height));
        }
    }
    let a = r.audio_codec();
    if a != RecordAudioCodec::Copy {
        how.push_str(&format!(", звук {}", alabel(a).split(' ').next().unwrap_or_default()));
        if a.has_bitrate() {
            how.push_str(&format!(" {}", r.audio.bitrate));
        }
        if r.audio.sample_rate > 0 {
            how.push_str(&format!(" {} Гц", r.audio.sample_rate));
        }
    }
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
            if let Some(e) = &l.error {
                v.push(("Ошибка записи".into(), crate::ru::server(e)));
            }
            if let Some(free) = l.free_bytes {
                v.push(("Свободно на диске".into(), bytes(free)));
            }
        }
        None => v.push(("Сейчас".into(), "не пишется (кодировщик остановлен)".into())),
    }
    v
}
