//! Main window: top status bar, navigation, central view, dialogs.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, TextureHandle, Ui};
use serde_json::{Value, json};

use crate::GuiOptions;
use crate::model::{LogLine, Model};
use crate::net::api::Api;
use crate::net::poller::Poller;
use crate::net::worker::Worker;
use crate::net::{Msg, events};
use crate::views::{self, Tab, Views};
use crate::widgets::{Confirm, confirm_modal, hms, pill};

pub struct App {
    pub worker: Worker,
    rx: Receiver<Msg>,
    poller: Poller,
    pub model: Model,
    pub tab: Tab,
    pub preview: Option<TextureHandle>,
    pub views: Views,
    confirm: Option<Confirm>,
    pub dark: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: GuiOptions) -> Self {
        let api = Api::new(&opts.base_url, opts.token.clone());
        let (worker, rx) = Worker::new(api.clone(), cc.egui_ctx.clone());
        let poller = Poller::start(api.clone(), worker.sender(), cc.egui_ctx.clone());
        events::start(&api, worker.sender(), cc.egui_ctx.clone());
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let app = App {
            worker,
            rx,
            poller,
            model: Model { channel: opts.channel.clone(), ..Default::default() },
            tab: Tab::OnAir,
            preview: None,
            views: Views::default(),
            confirm: None,
            dark: true,
        };
        app.load_channels();
        app
    }

    // ------------------------------------------------------------ requests

    pub fn load_channels(&self) {
        self.worker.spawn(|api| {
            let list: Vec<Value> = api.get("/channels")?;
            Ok(Msg::Channels(list.iter().filter_map(|c| c["channel"].as_str().map(str::to_string)).collect()))
        });
    }

    /// Reload all editable documents of the channel.
    pub fn load_all(&self) {
        let ch = self.model.ch();
        if ch.is_empty() {
            return;
        }
        let c = ch.clone();
        self.worker.spawn(move |api| Ok(Msg::Library(api.get(&format!("/channels/{c}/media"))?)));
        let c = ch.clone();
        self.worker.spawn(move |api| {
            let list: Vec<Value> = api.get(&format!("/channels/{c}/playlists"))?;
            let mut full = Vec::new();
            for p in list {
                full.push(api.get(&format!("/channels/{c}/playlists/{}", p["id"].as_str().unwrap_or("")))?);
            }
            Ok(Msg::Playlists(full))
        });
        let c = ch.clone();
        self.worker.spawn(move |api| Ok(Msg::Schedules(api.get(&format!("/channels/{c}/schedules"))?)));
        let c = ch.clone();
        self.worker.spawn(move |api| Ok(Msg::Rules(api.get(&format!("/channels/{c}/rules"))?)));
        let c = ch;
        self.worker.spawn(move |api| Ok(Msg::Graphics(api.get(&format!("/channels/{c}/graphics"))?)));
    }

    /// Send a channel command (`{"cmd": ...}`).
    pub fn command(&self, cmd: Value) {
        let ch = self.model.ch();
        self.worker.spawn(move |api| {
            let r = api.post(&format!("/channels/{ch}/command"), &cmd)?;
            Ok(Msg::Done(r["message"].as_str().unwrap_or("ok").to_string()))
        });
    }

    /// Ask before running a dangerous action.
    pub fn ask(&mut self, text: impl Into<String>, action: impl FnOnce(&mut App) + 'static) {
        self.confirm = Some(Confirm { text: text.into(), action: Box::new(action) });
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.model.toast = Some((text.into(), error, Instant::now()));
    }

    // ------------------------------------------------------------ messages

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Channels(list) => {
                    if self.model.channel.as_ref().is_none_or(|c| !list.contains(c)) {
                        self.model.channel = list.first().cloned();
                    }
                    self.model.channels = list;
                    if let Some(c) = &self.model.channel {
                        self.poller.set_channel(c);
                    }
                    self.load_all();
                }
                Msg::Status(s) => self.model.status = *s,
                Msg::Rundown(r) => self.model.rundown = r,
                Msg::Plan(r) => self.model.plan = r,
                Msg::Library(l) => self.model.library = l,
                Msg::Playlists(p) => {
                    self.model.playlists = p;
                    self.views.playlists.on_reload(&self.model.playlists);
                }
                Msg::Schedules(s) => self.model.schedules = s,
                Msg::Rules(r) => self.model.rules = r,
                Msg::Graphics(g) => self.model.graphics = g,
                Msg::Stats(s) => self.model.stats = s,
                Msg::Preview(img) => match &mut self.preview {
                    Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                    None => self.preview = Some(ctx.load_texture("preview", img, egui::TextureOptions::LINEAR)),
                },
                Msg::Event(v) => self.model.push_log(LogLine::from_event(&v)),
                Msg::Done(text) => self.toast(text, false),
                Msg::Error(e) => {
                    self.model.push_log(LogLine {
                        at: chrono::Local::now().format("%H:%M:%S").to_string(),
                        level: "warning".into(),
                        text: e.clone(),
                    });
                    self.toast(e, true);
                }
                Msg::Connection(ok) => {
                    self.model.online = ok;
                    if ok && self.model.channels.is_empty() {
                        self.load_channels();
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ layout

    fn top_bar(&mut self, ui: &mut Ui) {
        let s = self.model.status.clone();
        ui.horizontal(|ui| {
            ui.heading(RichText::new("LAURABROD").strong());
            ui.separator();
            let current = self.model.channel.clone().unwrap_or_default();
            let mut selected = current.clone();
            egui::ComboBox::from_id_salt("channel").selected_text(format!("{} {}", current, s.name)).show_ui(ui, |ui| {
                for c in &self.model.channels {
                    ui.selectable_value(&mut selected, c.clone(), c);
                }
            });
            if selected != current {
                self.model.channel = Some(selected.clone());
                self.poller.set_channel(&selected);
                self.load_all();
            }
            ui.separator();
            let (txt, col) = match (self.model.online, s.state.as_str()) {
                (false, _) => ("НЕТ СВЯЗИ", Color32::from_rgb(255, 90, 90)),
                (_, "on_air") => ("В ЭФИРЕ", Color32::from_rgb(90, 220, 120)),
                (_, "starting") => ("ЗАПУСК", Color32::from_rgb(255, 210, 90)),
                (_, "reconnecting") => ("ПЕРЕПОДКЛЮЧЕНИЕ", Color32::from_rgb(255, 160, 60)),
                (_, "error") => ("ОШИБКА", Color32::from_rgb(255, 90, 90)),
                _ => ("ОСТАНОВЛЕН", Color32::GRAY),
            };
            pill(ui, txt, col);
            if s.manual {
                pill(ui, "РУЧНОЙ РЕЖИМ", Color32::from_rgb(255, 210, 90));
            }
            let enc = &s.output;
            let enc_col = if enc.running { Color32::from_rgb(90, 220, 120) } else { Color32::from_rgb(255, 90, 90) };
            pill(ui, &format!("ENC {}", if enc.video_codec.is_empty() { "—" } else { &enc.video_codec }), enc_col);
            let rate = if enc.stats.bitrate_kbps > 0.0 { format!("{:.0} кбит/с  ", enc.stats.bitrate_kbps) } else { String::new() };
            ui.label(format!(
                "{rate}{:.1} fps  {:.2}x  потеряно кадров: {}  перезапусков: {}",
                enc.stats.fps, enc.stats.speed, enc.stats.drop_frames, enc.restarts
            ));
            if s.plan_warnings > 0 {
                ui.colored_label(Color32::from_rgb(255, 200, 80), format!("⚠ предупреждений плана: {}", s.plan_warnings));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let now =
                    s.now.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_else(|| chrono::Local::now().format("%H:%M:%S").to_string());
                ui.label(RichText::new(now).monospace().size(22.0));
                if let Some(t) = s.time_to_next_program_ms {
                    ui.label(format!("до след. передачи {}", hms(t.max(0))));
                }
            });
        });
    }

    fn nav(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        for t in Tab::ALL {
            if ui.selectable_label(self.tab == t, RichText::new(t.title()).size(16.0)).clicked() && self.tab != t {
                self.tab = t;
                views::on_open(self, t);
            }
        }
        ui.add_space(12.0);
        ui.separator();
        if ui.button("⟳ Обновить данные").clicked() {
            self.load_all();
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        if let Some((text, err, at)) = &self.model.toast {
            if at.elapsed() > Duration::from_secs(5) {
                self.model.toast = None;
                return;
            }
            let col = if *err { Color32::from_rgb(120, 30, 30) } else { Color32::from_rgb(30, 90, 50) };
            egui::Area::new(egui::Id::unique("toast")).anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -16.0]).show(ctx, |ui| {
                egui::Frame::new().fill(col).corner_radius(6.0).inner_margin(10).show(ui, |ui| {
                    ui.set_max_width(460.0);
                    ui.label(RichText::new(text.as_str()).color(Color32::WHITE));
                });
            });
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            ui.add_space(4.0);
        });
        egui::Panel::left("nav").resizable(false).exact_size(190.0).show(ui, |ui| self.nav(ui));
        egui::CentralPanel::default().show(ui, |ui| views::show(self, ui));
        if let Some(c) = &self.confirm {
            match confirm_modal(&ctx, c) {
                Some(true) => {
                    let c = self.confirm.take().unwrap();
                    (c.action)(self);
                }
                Some(false) => self.confirm = None,
                None => {}
            }
        }
        self.toast_ui(&ctx);
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

/// Convenience for views: JSON command.
pub fn cmd(name: &str) -> Value {
    json!({ "cmd": name })
}
