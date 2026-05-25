//! `/ws/spikes` — subscribe to the spike broadcast bus.
//!
//! Frame format (versioned):
//! ```json
//! { "v": 1, "t_ms": 1234.5, "events": [{ "node_id": "uuid", "t_ms": 1234.5 }, ...] }
//! ```
//!
//! Lagged receivers are dropped silently (broadcast channel semantics) —
//! that's the right behavior for a viz stream.
//!
//! `/ws/voltage` — sampled membrane-potential stream. Unlike spikes
//! (event-driven, broadcast to all), voltage is polled on a per-socket
//! timer and filtered to the neurons named in the `?nodes=` query, so
//! each connection only pays for the traces it's actually drawing.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use super::AppState;
use cortex_snn::{VoltageFrame, VoltageSample};

pub async fn ws_spikes(ws: WebSocketUpgrade, State(s): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| spike_socket(socket, s))
}

async fn spike_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.engine.spikes.subscribe();

    // Drain inbound (we don't accept client commands on this socket).
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => break,
                _ => continue,
            }
        }
    });

    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    let payload = match serde_json::to_string(&frame) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    if sender.send(Message::Text(payload)).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::debug!(lagged = n, "ws spike receiver lagged; continuing");
                    continue;
                }
                Err(RecvError::Closed) => break,
            }
        }
    });

    let _ = tokio::join!(recv_task, send_task);
}

/// Default voltage sampling rate. 30 Hz reads smooth on a scrolling
/// trace without flooding the socket or hammering the engine actor.
const VOLTAGE_SAMPLE_HZ: u64 = 30;

/// Hard cap on how many neurons a single voltage socket will sample when
/// no `?nodes=` filter is given. Bounds the per-frame payload so an
/// unfiltered subscriber on a large network can't stall the actor.
const VOLTAGE_UNFILTERED_CAP: usize = 64;

#[derive(serde::Deserialize)]
pub struct VoltageQuery {
    /// Comma-separated neuron UUIDs to sample. Omitted ⇒ sample all
    /// (capped at [`VOLTAGE_UNFILTERED_CAP`]).
    nodes: Option<String>,
}

pub async fn ws_voltage(
    ws: WebSocketUpgrade,
    Query(q): Query<VoltageQuery>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let nodes = q.nodes.as_deref().map(parse_node_ids);
    ws.on_upgrade(move |socket| voltage_socket(socket, s, nodes))
}

/// Parse a comma-separated UUID list, dropping anything unparseable.
fn parse_node_ids(raw: &str) -> Vec<Uuid> {
    raw.split(',')
        .filter_map(|s| Uuid::parse_str(s.trim()).ok())
        .collect()
}

async fn voltage_socket(socket: WebSocket, state: AppState, nodes: Option<Vec<Uuid>>) {
    let (mut sender, mut receiver) = socket.split();

    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => break,
                _ => continue,
            }
        }
    });

    let send_task = tokio::spawn(async move {
        let mut ticker =
            tokio::time::interval(Duration::from_millis(1000 / VOLTAGE_SAMPLE_HZ.max(1)));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let (t_ms, mut pairs) = match state.engine.voltage_snapshot(nodes.clone()).await {
                Ok(v) => v,
                // Engine actor gone (shutdown) — close the socket.
                Err(_) => break,
            };
            if nodes.is_none() && pairs.len() > VOLTAGE_UNFILTERED_CAP {
                pairs.truncate(VOLTAGE_UNFILTERED_CAP);
            }
            let samples = pairs
                .into_iter()
                .map(|(node_id, v_mv)| VoltageSample { node_id, v_mv })
                .collect();
            let frame = VoltageFrame::new(t_ms, samples);
            let payload = match serde_json::to_string(&frame) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if sender.send(Message::Text(payload)).await.is_err() {
                break;
            }
        }
    });

    let _ = tokio::join!(recv_task, send_task);
}
