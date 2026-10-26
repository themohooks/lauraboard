//! WebSocket event stream (`/api/v1/events`), reconnecting on failure.

use std::time::Duration;

use serde_json::Value;

use super::api::Api;
use super::{Msg, Wake};

pub fn start(api: &Api, tx: std::sync::mpsc::Sender<Msg>, wake: Wake) {
    let url = api.ws_url("/events");
    std::thread::spawn(move || {
        loop {
            match tungstenite::connect(url.as_str()) {
                Ok((mut ws, _)) => loop {
                    match ws.read() {
                        Ok(tungstenite::Message::Text(t)) => {
                            if let Ok(v) = serde_json::from_str::<Value>(t.as_str()) {
                                // periodic status snapshots are handled by the poller
                                if v["type"] != "status" {
                                    if tx.send(Msg::Event(v)).is_err() {
                                        return;
                                    }
                                    wake();
                                }
                            }
                        }
                        Ok(tungstenite::Message::Close(_)) | Err(_) => break,
                        Ok(_) => {}
                    }
                },
                Err(_) => std::thread::sleep(Duration::from_secs(3)),
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}
