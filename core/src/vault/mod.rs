//! Vault ingestion: Obsidian-formatted markdown → graph (nodes + edges).
//!
//! Pure: takes a path, returns parsed structs. Persistence + engine
//! hookup live in the API layer.

pub mod obsidian;

pub use obsidian::{parse_vault, VaultParseSummary};
