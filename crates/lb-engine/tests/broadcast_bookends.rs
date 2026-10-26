//! Real decoder check using synthetic colour scenes and a null output.

use lb_core::{
    Dur,
    media::{MediaItem, MediaKind, Scene},
    planner::PlanMode,
    playlist::Playlist,
    project::Project,
};
use lb_engine::{
    Engine,
    channel::{Command, EventKind},
};

#[tokio::test]
#[ignore = "requires FFmpeg with libx264"]
async fn opening_and_closing_scenes_play_before_the_output_stops() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = Project::init(dir.path(), "main").unwrap();
    let channel = project.channel_mut("main").unwrap();
    channel.config.autostart = false;
    channel.config.video.width = 160;
    channel.config.video.height = 90;
    channel.config.encoder.video_codec = "libx264".into();
    channel.config.encoder.preset = "ultrafast".into();
    channel.config.encoder.threads = 1;
    channel.config.preview.enabled = false;
    channel.config.graphics.enabled = false;
    channel.config.planning.mode = PlanMode::Sequential { playlist: "program".into() };
    for (id, kind, seconds) in [("open", MediaKind::Jingle, 2), ("program", MediaKind::Program, 15), ("close", MediaKind::Jingle, 2)] {
        channel.library.upsert(MediaItem::scene(id, kind, Dur::secs(seconds), Scene::default()));
    }
    channel.playlists.push(Playlist::new("program").with_media(&["program"]));
    channel.rules = serde_json::from_value(serde_json::json!({"rules":[
        {"id":"open", "trigger":{"type":"broadcast_start"}, "action":{"type":"scene", "items":["open"]}},
        {"id":"close", "trigger":{"type":"broadcast_end"}, "action":{"type":"scene", "items":["close"]}}
    ]}))
    .unwrap();
    channel.save().unwrap();
    let engine = Engine::start(dir.path()).unwrap();
    let channel = engine.channel("main").unwrap();
    let mut events = channel.subscribe();
    channel.send(Command::Start).await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let mut opened = false;
        loop {
            if let EventKind::ItemStarted { item } = events.recv().await.unwrap().kind {
                if item.media == "open" {
                    opened = true;
                }
                if item.media == "program" {
                    assert!(opened);
                    break;
                }
            }
        }
        channel.send(Command::Stop).await.unwrap();
        let (mut closing, mut completed) = (false, false);
        loop {
            match events.recv().await.unwrap().kind {
                EventKind::ItemStarted { item } if item.media == "close" => {
                    closing = true;
                    assert!(channel.status().output.running);
                }
                EventKind::ItemEnded { record } if record.media == "close" => {
                    completed = record.status == lb_core::history::AsRunStatus::Completed;
                }
                EventKind::Stopped => {
                    assert!(closing && completed, "closing scene must finish before output disconnects");
                    break;
                }
                _ => {}
            }
        }
    })
    .await;
    engine.shutdown().await;
    result.expect("broadcast lifecycle timed out");
}
