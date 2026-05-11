//! `/ws/weights` — subscribe to STDP weight deltas.
//! `/api/graph/weights` — one-shot snapshot for frontend hydration.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use super::{ok, AppState};
use crate::engine::{WeightDelta, WeightFrame};
use crate::error::{CoreError, CoreResult};

pub async fn ws_weights(
    ws: WebSocketUpgrade,
    State(s): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| weights_socket(socket, s))
}

async fn weights_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.engine.weights.subscribe();

    // Send a full snapshot on connect so the frontend starts coherent
    // without waiting for the next forced-full broadcast.
    if let Ok(snap) = state.engine.weight_snapshot().await {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let deltas: Vec<WeightDelta> = snap.into_iter()
            .map(|(edge_id, w)| WeightDelta { edge_id, w })
            .collect();
        let frame = WeightFrame::snapshot(now, deltas);
        if let Ok(payload) = serde_json::to_string(&frame) {
            let _ = sender.send(Message::Text(payload)).await;
        }
    }

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
                    if sender.send(Message::Text(payload)).await.is_err() { break; }
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::debug!(lagged = n, "ws weight receiver lagged; continuing");
                    continue;
                }
                Err(RecvError::Closed) => break,
            }
        }
    });

    let _ = tokio::join!(recv_task, send_task);
}

pub async fn get_weights_snapshot(
    State(s): State<AppState>,
) -> CoreResult<Json<serde_json::Value>> {
    let snap = s.engine.weight_snapshot().await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    let payload: Vec<_> = snap.into_iter()
        .map(|(edge_id, w)| serde_json::json!({ "edge_id": edge_id, "w": w }))
        .collect();
    Ok(ok(serde_json::json!({ "weights": payload })))
}
