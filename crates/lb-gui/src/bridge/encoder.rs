//! Encoder settings dialog on the Sources screen: fills the fields from the
//! channel settings, checks them and saves (the encoder restarts).

use lb_core::config::{ChannelConfig, RateControl};
use slint::{ComponentHandle, SharedString};

use super::{Ctx, vec_model, with};
use crate::shell::{AppWindow, Src};

/// `(ffmpeg name, label)`.
const CODECS: [(&str, &str); 10] = [
    ("auto", "Авто — лучший доступный"),
    ("libx264", "H.264 — процессор (libx264)"),
    ("libx265", "HEVC — процессор (libx265)"),
    ("h264_nvenc", "H.264 — NVIDIA (NVENC)"),
    ("hevc_nvenc", "HEVC — NVIDIA (NVENC)"),
    ("h264_qsv", "H.264 — Intel (Quick Sync)"),
    ("h264_amf", "H.264 — AMD (AMF)"),
    ("h264_vaapi", "H.264 — VAAPI (Linux)"),
    ("h264_videotoolbox", "H.264 — Apple (VideoToolbox)"),
    ("hevc_videotoolbox", "HEVC — Apple (VideoToolbox)"),
];
const X26_PRESETS: [&str; 9] = ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];
const NVENC_PRESETS: [&str; 7] = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"];
const AMF_PRESETS: [&str; 3] = ["speed", "balanced", "quality"];
/// "" = none.
const TUNES: [&str; 7] = ["", "zerolatency", "film", "animation", "grain", "stillimage", "fastdecode"];
/// "" = encoder default.
const PROFILES: [&str; 4] = ["", "baseline", "main", "high"];
const PIX_FMTS: [&str; 4] = ["yuv420p", "yuv422p", "yuv444p", "nv12"];
const ACODECS: [(&str, &str); 4] = [("aac", "AAC"), ("libopus", "Opus"), ("libmp3lame", "MP3"), ("ac3", "AC-3 (Dolby Digital)")];
const RATES: [u32; 2] = [44_100, 48_000];
const CHANNELS: [u32; 3] = [1, 2, 6];

pub fn connect(ui: &AppWindow) {
    let g = ui.global::<Src>();
    g.set_k_codecs(vec_model(CODECS.iter().map(|(_, l)| SharedString::from(*l)).collect()));
    g.set_k_tunes(vec_model(TUNES.iter().map(|t| SharedString::from(if t.is_empty() { "нет" } else { t })).collect()));
    g.set_k_profiles(vec_model(PROFILES.iter().map(|p| SharedString::from(if p.is_empty() { "авто" } else { p })).collect()));
    g.set_k_pix_fmts(vec_model(
        PIX_FMTS.iter().map(|p| SharedString::from(if *p == "yuv420p" { "yuv420p — совместимый" } else { p })).collect(),
    ));
    g.set_k_acodecs(vec_model(ACODECS.iter().map(|(_, l)| SharedString::from(*l)).collect()));
    g.on_encoder_edit(|| with(open));
    g.on_encoder_codec_changed(|| {
        with(|c| {
            let Some(ui) = c.window() else { return };
            let g = ui.global::<Src>();
            // keep the preset when the new family has one of that name
            let old = presets(codec_at(g.get_k_codec())).get(g.get_k_preset().max(0) as usize).copied().unwrap_or("");
            set_presets(&ui, codec_at(g.get_k_codec()), old);
        })
    });
    g.on_encoder_save(|| with(save));
}

fn codec_at(i: i32) -> &'static str {
    CODECS.get(i.max(0) as usize).map(|(c, _)| *c).unwrap_or("auto")
}

/// Speed presets of the codec family.
fn presets(codec: &str) -> &'static [&'static str] {
    if codec.contains("nvenc") {
        &NVENC_PRESETS
    } else if codec.contains("amf") {
        &AMF_PRESETS
    } else if codec.starts_with("libx26") || codec.contains("qsv") || codec == "auto" {
        &X26_PRESETS
    } else {
        &[]
    }
}

/// Fill the preset list for `codec` and select `current` (or a sensible default).
fn set_presets(ui: &AppWindow, codec: &str, current: &str) {
    let g = ui.global::<Src>();
    let list = presets(codec);
    let default = if codec.contains("nvenc") {
        "p4"
    } else if codec.contains("amf") {
        "speed"
    } else {
        "veryfast"
    };
    let i = list.iter().position(|p| *p == current).or_else(|| list.iter().position(|p| *p == default)).map_or(-1, |i| i as i32);
    g.set_k_presets(vec_model(list.iter().map(|p| SharedString::from(*p)).collect()));
    g.set_k_preset(i);
    g.set_k_has_tune(codec.starts_with("libx26") || codec == "auto");
}

fn index_of(list: &[&str], v: &str) -> i32 {
    list.iter().position(|x| *x == v).map_or(0, |i| i as i32)
}

fn open(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let Some(cfg) = super::sources::config(c) else {
        c.toast("Настройки канала недоступны: нужна роль администратора", true);
        return;
    };
    let g = ui.global::<Src>();
    let (e, v, a) = (&cfg.encoder, &cfg.video, &cfg.audio);
    let codec = CODECS.iter().position(|(id, _)| *id == e.video_codec).unwrap_or(0);
    g.set_k_codec(codec as i32);
    set_presets(&ui, CODECS[codec].0, &e.preset);
    g.set_k_profile(index_of(&PROFILES, &e.profile));
    g.set_k_level(e.level.clone().into());
    g.set_k_tune(index_of(&TUNES, &e.tune));
    g.set_k_rc(match e.rate_control {
        RateControl::Cbr => 0,
        RateControl::Vbr => 1,
        RateControl::Quality => 2,
    });
    g.set_k_bitrate(e.video_bitrate.clone().into());
    g.set_k_maxrate(e.maxrate.clone().unwrap_or_default().into());
    g.set_k_bufsize(e.bufsize.clone().unwrap_or_default().into());
    g.set_k_crf(e.crf as f32);
    g.set_k_gop(num(e.gop_seconds).into());
    g.set_k_bframes(e.bframes.map(|b| b.to_string()).unwrap_or_default().into());
    g.set_k_scene_cut(e.scene_cut);
    g.set_k_threads(if e.threads == 0 { String::new() } else { e.threads.to_string() }.into());
    g.set_k_pix_fmt(index_of(&PIX_FMTS, &e.pix_fmt));
    g.set_k_width(v.width.to_string().into());
    g.set_k_height(v.height.to_string().into());
    g.set_k_fps(num(v.fps).into());
    g.set_k_acodec(ACODECS.iter().position(|(id, _)| *id == e.audio_codec).unwrap_or(0) as i32);
    g.set_k_abitrate(e.audio_bitrate.clone().into());
    g.set_k_rate(RATES.iter().position(|r| *r == a.sample_rate).unwrap_or(1) as i32);
    g.set_k_channels(CHANNELS.iter().position(|ch| *ch == a.channels).unwrap_or(1) as i32);
    g.set_k_volume(if a.volume_db == 0.0 { String::new() } else { num(a.volume_db as f64) }.into());
    g.set_k_normalize(a.normalize);
    g.set_k_extra(e.extra_args.join(" ").into());
    g.set_k_error(SharedString::new());
    g.set_dialog("encoder".into());
}

/// 2.0 → "2", 29.97 → "29.97".
fn num(x: f64) -> String {
    let s = format!("{x:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// "3000k", "2.5M", "800000" → normalised text, or `None`.
fn rate(s: &str) -> Option<String> {
    let s = s.trim().replace(',', ".");
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(s.len()));
    let ok = n.parse::<f64>().is_ok_and(|x| x > 0.0) && matches!(unit, "" | "k" | "K" | "m" | "M");
    ok.then(|| format!("{n}{}", unit.to_lowercase().replace('m', "M")))
}

fn opt_rate(s: &str, what: &str) -> Result<Option<String>, String> {
    if s.trim().is_empty() {
        return Ok(None);
    }
    rate(s).map(Some).ok_or_else(|| format!("{what}: число с k или M, например 4500k"))
}

fn parse<T: std::str::FromStr>(s: &str, what: &str) -> Result<T, String> {
    s.trim().replace(',', ".").parse().map_err(|_| format!("{what}: нужно число"))
}

/// Read the dialog into `cfg`; `Err` names the first bad field.
fn apply(ui: &AppWindow, cfg: &mut ChannelConfig) -> Result<(), String> {
    let g = ui.global::<Src>();
    let codec = codec_at(g.get_k_codec());
    let e = &mut cfg.encoder;
    e.video_codec = codec.into();
    if let Some(p) = presets(codec).get(g.get_k_preset().max(0) as usize) {
        e.preset = (*p).into();
    }
    e.profile = PROFILES.get(g.get_k_profile().max(0) as usize).copied().unwrap_or("").into();
    e.level = g.get_k_level().trim().into();
    e.tune = TUNES.get(g.get_k_tune().max(0) as usize).copied().unwrap_or("").into();
    e.rate_control = match g.get_k_rc() {
        1 => RateControl::Vbr,
        2 => RateControl::Quality,
        _ => RateControl::Cbr,
    };
    if e.rate_control != RateControl::Quality {
        e.video_bitrate = rate(&g.get_k_bitrate()).ok_or("Битрейт видео: число с k или M, например 3000k")?;
    }
    e.maxrate = opt_rate(&g.get_k_maxrate(), "Максимум")?;
    e.bufsize = opt_rate(&g.get_k_bufsize(), "Буфер")?;
    e.crf = g.get_k_crf().round().clamp(0.0, 51.0) as u32;
    e.gop_seconds = if g.get_k_gop().trim().is_empty() { 2.0 } else { parse(&g.get_k_gop(), "Ключевой кадр")? };
    if !(0.1..=20.0).contains(&e.gop_seconds) {
        return Err("Ключевой кадр: от 0,1 до 20 секунд".into());
    }
    e.bframes = if g.get_k_bframes().trim().is_empty() { None } else { Some(parse(&g.get_k_bframes(), "B-кадры")?) };
    if e.bframes.is_some_and(|b| b > 16) {
        return Err("B-кадры: от 0 до 16".into());
    }
    e.threads = if g.get_k_threads().trim().is_empty() { 0 } else { parse(&g.get_k_threads(), "Потоки")? };
    if e.threads > 64 {
        return Err("Потоки: от 0 до 64".into());
    }
    e.scene_cut = g.get_k_scene_cut();
    e.pix_fmt = PIX_FMTS.get(g.get_k_pix_fmt().max(0) as usize).copied().unwrap_or("yuv420p").into();
    e.audio_codec = ACODECS.get(g.get_k_acodec().max(0) as usize).map_or("aac", |(c, _)| *c).into();
    e.audio_bitrate = rate(&g.get_k_abitrate()).ok_or("Битрейт звука: число с k, например 160k")?;
    e.extra_args = g.get_k_extra().split_whitespace().map(String::from).collect();

    let v = &mut cfg.video;
    v.width = parse(&g.get_k_width(), "Ширина")?;
    v.height = parse(&g.get_k_height(), "Высота")?;
    if v.width < 16 || v.height < 16 || !v.width.is_multiple_of(2) || !v.height.is_multiple_of(2) {
        return Err("Ширина и высота — чётные числа не меньше 16".into());
    }
    v.fps = parse(&g.get_k_fps(), "Кадров в секунду")?;
    if !(1.0..=120.0).contains(&v.fps) {
        return Err("Кадров в секунду: от 1 до 120".into());
    }
    let a = &mut cfg.audio;
    a.sample_rate = RATES.get(g.get_k_rate().max(0) as usize).copied().unwrap_or(48_000);
    a.channels = CHANNELS.get(g.get_k_channels().max(0) as usize).copied().unwrap_or(2);
    a.volume_db = if g.get_k_volume().trim().is_empty() { 0.0 } else { parse(&g.get_k_volume(), "Громкость")? };
    if !(-30.0..=20.0).contains(&a.volume_db) {
        return Err("Громкость: от −30 до +20 дБ".into());
    }
    a.normalize = g.get_k_normalize();
    Ok(())
}

fn save(c: &mut Ctx) {
    let Some(ui) = c.window() else { return };
    let Some(mut cfg) = super::sources::config(c) else { return };
    let before = cfg.clone();
    if let Err(e) = apply(&ui, &mut cfg) {
        ui.global::<Src>().set_k_error(e.into());
        return;
    }
    ui.global::<Src>().set_dialog(SharedString::new());
    if cfg == before {
        return;
    }
    let commit = move |c: &mut Ctx| {
        c.channel_config = serde_json::to_value(&cfg).ok();
        super::sources::refresh(c);
        c.save_doc("/config", cfg, "Настройки кодировщика");
    };
    if c.model.status.output.running {
        c.ask(
            "Перезапустить кодировщик?",
            "Новые настройки вступят в силу после перезапуска — в эфире будет обрыв на несколько секунд.",
            "Сохранить",
            commit,
        );
    } else {
        commit(c);
    }
}

#[cfg(test)]
mod tests {
    use super::{num, rate};

    #[test]
    fn rates_and_numbers() {
        assert_eq!(rate("3000k").as_deref(), Some("3000k"));
        assert_eq!(rate(" 2,5m ").as_deref(), Some("2.5M"));
        assert_eq!(rate("800000").as_deref(), Some("800000"));
        assert_eq!(rate("fast"), None);
        assert_eq!(rate("0k"), None);
        assert_eq!(num(2.0), "2");
        assert_eq!(num(29.97), "29.97");
    }
}
