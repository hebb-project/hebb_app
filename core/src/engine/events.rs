//! Wire-format types for spike events broadcast to WS subscribers.
//!
//! The frame envelope is versioned (`v: 1`) so we can swap to binary
//! (postcard / msgpack) under the same shape without a breaking change
//! on the frontend.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpikeEvent {
    pub node_id: Uuid,
    pub t_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpikeFrame {
    pub v: u8,
    pub t_ms: f64,
    pub events: Vec<SpikeEvent>,
}

impl SpikeFrame {
    pub fn new(t_ms: f64, events: Vec<SpikeEvent>) -> Self {
        Self { v: 1, t_ms, events }
    }
}
