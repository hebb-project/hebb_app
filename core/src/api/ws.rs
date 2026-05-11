//! `/ws/spikes` — subscribe to the spike broadcast bus.
//!
//! Frame format (versioned):
//! ```json
//! { "v": 1, "t_ms": 1234.5, "events": [{ "node_id": "uuid", "t_ms": 1234.5 }, ...] }
//! ```
//!
//! Lagged receivers are dropped silently (broadcast channel semantics) —
//! that's the right behavior for a viz stream.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use super::AppState;

pub async fn ws_spikes(
    ws: WebSocketUpgrade,
    State(s): State<AppState>,
) -> impl IntoResponse {
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
