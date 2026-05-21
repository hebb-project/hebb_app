//! HTTP + WebSocket surface.

pub mod chat;
pub mod cortex;
pub mod graph;
pub mod health;
pub mod search;
pub mod stimulate;
pub mod state;
pub mod vault;
pub mod weights;
pub mod ws;

pub use state::AppState;

use axum::routing::{get, post, delete};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/health", get(health::get_health))
        .route("/api/graph", get(graph::get_full_graph))
        .route("/api/graph/nodes", get(graph::list_nodes).post(graph::create_node))
        .route("/api/graph/nodes/:id", get(graph::get_node).delete(graph::delete_node))
        .route("/api/graph/edges", get(graph::list_edges).post(graph::create_edge))
        .route("/api/graph/edges/:id", delete(graph::delete_edge))
        .route("/api/search", get(search::search_nodes))
        .route("/api/stimulate", post(stimulate::post_stimulate))
        .route("/api/chat", post(chat::post_chat))
        .route("/api/cortex", get(cortex::get_current).post(cortex::post_configure))
        .route("/api/cortex/open", post(cortex::post_open))
        .route("/api/cortex/folder", get(cortex::get_folder))
        .route("/api/vault/ingest", post(vault::post_ingest))
        .route("/api/graph/weights", get(weights::get_weights_snapshot))
        .route("/ws/spikes", get(ws::ws_spikes))
        .route("/ws/weights", get(weights::ws_weights))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Standard success envelope used by all REST handlers.
pub fn ok<T: serde::Serialize>(data: T) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "data": data, "error": serde_json::Value::Null }))
}
