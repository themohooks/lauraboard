//! Command-line definition (clap).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "laurabrod", version, about = "Internet TV channel playout & automation", long_about = None)]
pub struct Cli {
    /// Project directory (contains laurabrod.toml).
    #[arg(short, long, global = true, env = "LAURABROD_PROJECT", default_value = ".")]
    pub project: PathBuf,
    /// Channel id (default: the first channel).
    #[arg(short, long, global = true, env = "LAURABROD_CHANNEL")]
    pub channel: Option<String>,
    /// API token for `ctl` commands (default: first admin token in config).
    #[arg(long, global = true, env = "LAURABROD_TOKEN", hide_env_values = true)]
    pub token: Option<String>,
    /// Output machine-readable JSON where supported.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Create a new project.
    Init {
        /// Initial channel id.
        #[arg(long, default_value = "main")]
        id: String,
        /// Fill the project with a demo channel (generated test media).
        #[arg(long)]
        demo: bool,
    },
    /// Run the playout engine without GUI (foreground; use for services).
    Run {
        /// Do not start the HTTP API.
        #[arg(long)]
        no_api: bool,
    },
    /// Open the graphical interface.
    #[cfg(feature = "gui")]
    Gui {
        /// Connect to a running instance (e.g. http://127.0.0.1:8787) instead
        /// of starting an embedded engine.
        #[arg(long)]
        connect: Option<String>,
    },
    /// Validate configuration and media (exit code 2 on errors).
    Validate {
        /// Skip media file checks.
        #[arg(long)]
        no_media: bool,
    },
    /// Preview the planned schedule.
    Plan(PlanArgs),
    /// Media library.
    #[command(subcommand)]
    Media(MediaCmd),
    /// Playlists.
    #[command(subcommand)]
    Playlist(PlaylistCmd),
    /// Broadcast grid.
    #[command(subcommand)]
    Schedule(ScheduleCmd),
    /// As-run log and statistics.
    Report(ReportArgs),
    /// Back up project configuration (no media) to a zip file.
    Backup {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Restore configuration from a backup zip into the project directory.
    Restore {
        file: PathBuf,
        /// Overwrite existing files.
        #[arg(long)]
        force: bool,
    },
    /// Control a running instance through its API.
    #[command(subcommand)]
    Ctl(CtlCmd),
    /// Install / remove a system service (Windows service or systemd).
    #[command(subcommand)]
    Service(ServiceCmd),
}

#[derive(Args, Debug)]
pub struct PlanArgs {
    /// Start time `YYYY-MM-DDTHH:MM:SS` (default: now).
    #[arg(long)]
    pub from: Option<chrono::NaiveDateTime>,
    /// Hours to plan.
    #[arg(long, default_value_t = 6)]
    pub hours: i64,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Table)]
    pub format: Format,
    /// Only main programme items.
    #[arg(long)]
    pub programs_only: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Table,
    Json,
    Csv,
}

#[derive(Subcommand, Debug)]
pub enum MediaCmd {
    /// List library items.
    List {
        #[arg(long)]
        kind: Option<String>,
    },
    /// Add files, directories or URLs (probed with ffprobe).
    Add {
        paths: Vec<String>,
        /// program | jingle | ad | promo | filler | image | stream
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(short, long)]
        recursive: bool,
    },
    /// Remove an item.
    Remove { id: String },
    /// Re-probe all files; report missing / damaged ones.
    Check,
    /// Set fields of an item.
    Set {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        category: Option<String>,
        /// Comma separated tags.
        #[arg(long)]
        tags: Option<String>,
        #[arg(long)]
        volume_db: Option<f32>,
        /// Promo target block id.
        #[arg(long)]
        promo_block: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum PlaylistCmd {
    List,
    Show {
        id: String,
    },
    Create {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        no_loop: bool,
        #[arg(long)]
        shuffle: bool,
    },
    /// Append media ids.
    Add {
        id: String,
        media: Vec<String>,
        #[arg(long)]
        pinned: bool,
    },
    /// Remove the entry at a (0-based) position.
    RemoveEntry {
        id: String,
        index: usize,
    },
    /// Move an entry.
    Move {
        id: String,
        from: usize,
        to: usize,
    },
    /// Delete a playlist (asks for confirmation unless --yes).
    Delete {
        id: String,
        #[arg(long)]
        yes: bool,
    },
    /// Remove all entries (asks for confirmation unless --yes).
    Clear {
        id: String,
        #[arg(long)]
        yes: bool,
    },
    Import {
        id: String,
        file: PathBuf,
    },
    Export {
        id: String,
        /// Output file (default: stdout).
        file: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScheduleCmd {
    /// List schedules and templates.
    List,
    /// Assign a day template to weekdays (copy a day to other days).
    Assign {
        #[arg(long)]
        schedule: Option<String>,
        template: String,
        /// mon,tue,wed,thu,fri,sat,sun | weekdays | weekend | all
        days: String,
    },
    /// Duplicate a day template.
    Duplicate {
        #[arg(long)]
        schedule: Option<String>,
        from: String,
        to: String,
    },
    /// Use another template on a specific date.
    Exception {
        #[arg(long)]
        schedule: Option<String>,
        date: chrono::NaiveDate,
        /// Template id, or `none` for no grid that day.
        template: String,
    },
    /// Export the schedule set as JSON.
    Export { file: Option<PathBuf> },
    /// Import a schedule set from JSON (replaces it).
    Import { file: PathBuf },
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// asrun | stats
    #[arg(value_enum, default_value_t = ReportKind::Stats)]
    pub kind: ReportKind,
    #[arg(long)]
    pub from: Option<chrono::NaiveDate>,
    #[arg(long)]
    pub to: Option<chrono::NaiveDate>,
    #[arg(long, value_enum, default_value_t = Format::Table)]
    pub format: Format,
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind {
    Asrun,
    Stats,
}

#[derive(Subcommand, Debug)]
pub enum CtlCmd {
    /// Show on-air status.
    Status,
    /// Follow status every second.
    Watch,
    Start,
    /// Stop broadcasting (asks for confirmation unless --yes).
    Stop {
        #[arg(long)]
        yes: bool,
    },
    Skip,
    Restart,
    /// Rewind the current item: `+30` / `-10` seconds from the current
    /// position, or a position `00:12:30` from its start.
    Seek {
        #[arg(allow_hyphen_values = true)]
        position: String,
    },
    /// Pause the schedule (manual control) or resume it.
    Manual {
        #[arg(value_enum)]
        state: OnOff,
    },
    /// Play a media item now.
    Play {
        media: String,
    },
    /// Queue a media item after the current one.
    Queue {
        media: String,
    },
    ClearQueue,
    /// Insert an ad break.
    Adbreak {
        id: String,
        #[arg(long)]
        now: bool,
    },
    /// Show / hide a graphic element, or return it to rule control.
    Graphic {
        element: String,
        #[arg(value_enum)]
        action: GfxAction,
    },
    /// Set (or clear when no value) a graphics variable.
    Var {
        key: String,
        value: Option<String>,
    },
    /// Switch schedule (`auto` = by activation time).
    Schedule {
        id: String,
    },
    /// Reload configuration from disk.
    Reload,
    /// Upcoming items.
    Rundown {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Application log tail.
    Logs {
        #[arg(long, default_value_t = 100)]
        lines: usize,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnOff {
    On,
    Off,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GfxAction {
    Show,
    Hide,
    Auto,
}

#[derive(Subcommand, Debug)]
pub enum ServiceCmd {
    /// Register the service (runs `laurabrod run` for this project).
    Install {
        #[arg(long, default_value = "laurabrod")]
        name: String,
    },
    Uninstall {
        #[arg(long, default_value = "laurabrod")]
        name: String,
    },
    /// Entry point used by the Windows service manager.
    #[command(hide = true)]
    Run {
        #[arg(long, default_value = "laurabrod")]
        name: String,
    },
}
