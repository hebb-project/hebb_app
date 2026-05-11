//! Shared axum state. Cloneable; everything inside is `Arc` or already cheap.

use std::path::PathBuf;
use std::sync::Arc;

use crate::db::PgPool;
use crate::engine::SimHandle;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub engine: SimHandle,
    pub vault_path: Arc<PathBuf>,
}
