//! WebSocket event stream: `GET /api/v1/events` (all channels) or
//! `/api/v1/channels/{id}/events`. Each message is a JSON [`Event`]; a
//! `status` message with the full channel status is sent every second.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;

use lb_core::config::Role;
use lb_engine::{ChannelHandle, Event};

use crate::AppState;
use crate::auth::Caller;
use crate::error::ApiResult;

pub async fn events_all(State(s): State<AppState>, c: Caller, ws: WebSocketUpgrade) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let chans: Vec<ChannelHandle> = s.engine.channels();
    Ok(ws.on_upgrade(move |sock| stream(sock, chans)))
}

pub async fn events_channel(State(s): State<AppState>, c: Caller, Path(id): Path<String>, ws: WebSocketUpgrade) -> ApiResult<Response> {
    c.require(Role::Viewer)?;
    let ch = s.engine.channel(&id)?;
    Ok(ws.on_upgrade(move |sock| stream(sock, vec![ch])))
}

async fn stream(sock: WebSocket, chans: Vec<ChannelHandle>) {
    let (mut tx, mut rx) = sock.split();
    let (fan_tx, mut fan_rx) = tokio::sync::mpsc::channel::<String>(256);
    let mut tasks = Vec::new();
    for ch in &chans {
        let mut sub: broadcast::Receiver<Event> = ch.subscribe();
        let out = fan_tx.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                match sub.recv().await {
                    Ok(ev) => {
                        if let Ok(j) = serde_json::to_string(&ev)
                            && out.send(j).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        }));
    }
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tokio::select! {
            Some(msg) = fan_rx.recv() => {
                if tx.send(Message::Text(msg.into())).await.is_err() { break; }
            }
            _ = tick.tick() => {
                for ch in &chans {
                    let j = serde_json::json!({ "type": "status", "channel": ch.id(), "status": ch.status() });
                    if tx.send(Message::Text(j.to_string().into())).await.is_err() { break; }
                }
            }
            incoming = rx.next() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
        }
    }
    for t in tasks {
        t.abort();
    }
}
