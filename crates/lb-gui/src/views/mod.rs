//! Screens. Each view keeps its own UI state in [`Views`].

mod air;
mod graphics;
mod library;
mod log;
mod playlists;
mod reports;
mod rules;
mod rundown;
mod schedule;
mod settings;

use eframe::egui::Ui;

use crate::app::App;
use crate::net::Msg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    OnAir,
    Rundown,
    Playlists,
    Library,
    Schedule,
    Rules,
    Graphics,
    Reports,
    Log,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 10] = [
        Tab::OnAir,
        Tab::Rundown,
        Tab::Playlists,
        Tab::Library,
        Tab::Schedule,
        Tab::Rules,
        Tab::Graphics,
        Tab::Reports,
        Tab::Log,
        Tab::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::OnAir => "📺 Эфир",
            Tab::Rundown => "🕒 Рундаун",
            Tab::Playlists => "📃 Плейлисты",
            Tab::Library => "🎞 Медиатека",
            Tab::Schedule => "📅 Сетка вещания",
            Tab::Rules => "⚙ Правила вставок",
            Tab::Graphics => "🖼 Графика",
            Tab::Reports => "📊 Статистика",
            Tab::Log => "📝 Журнал",
            Tab::Settings => "🔧 Настройки",
        }
    }
}

/// Per-view UI state.
#[derive(Default)]
pub struct Views {
    pub air: air::AirView,
    pub playlists: playlists::PlaylistsView,
    pub library: library::LibraryView,
    pub schedule: schedule::ScheduleView,
    pub rules: rules::RulesView,
    pub graphics: graphics::GraphicsView,
    pub reports: reports::ReportsView,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    match app.tab {
        Tab::OnAir => air::show(app, ui),
        Tab::Rundown => rundown::show(app, ui),
        Tab::Playlists => playlists::show(app, ui),
        Tab::Library => library::show(app, ui),
        Tab::Schedule => schedule::show(app, ui),
        Tab::Rules => rules::show(app, ui),
        Tab::Graphics => graphics::show(app, ui),
        Tab::Reports => reports::show(app, ui),
        Tab::Log => log::show(app, ui),
        Tab::Settings => settings::show(app, ui),
    }
}

/// Data refresh when a tab is opened.
pub fn on_open(app: &mut App, tab: Tab) {
    match tab {
        Tab::Schedule => schedule::load_plan(app),
        Tab::Reports => reports::load(app),
        Tab::Playlists | Tab::Library | Tab::Rules | Tab::Graphics => app.load_all(),
        _ => {}
    }
}

/// Save a whole document with PUT and reload everything afterwards.
pub fn save_doc<T: serde::Serialize + Send + 'static>(app: &App, path: &str, doc: T, what: &'static str) {
    let ch = app.model.ch();
    let path = format!("/channels/{ch}{path}");
    app.worker.spawn(move |api| {
        api.put(&path, &doc)?;
        Ok(Msg::Done(format!("{what}: сохранено")))
    });
    app.load_all();
}
