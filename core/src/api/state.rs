//! Shared axum state. Cloneable; everything inside is `Arc` or already cheap.

use std::path::PathBuf;
use std::sync::Arc;

use crate::chat::ArcEncoder;
use crate::db::PgPool;
use crate::engine::SimHandle;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub engine: SimHandle,
    pub vault_path: Arc<PathBuf>,
    /// Encoder used by `POST /api/chat`. Picked at startup via
    /// `chat::make_encoder()` based on env (defaults to lexical so a
    /// zero-config desktop install still answers chat).
    pub chat_encoder: ArcEncoder,
}
