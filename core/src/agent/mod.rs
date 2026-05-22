//! Agent harness — provider-agnostic tool surface for external LLM agents.
//!
//! See the vault for the design:
//! - [[agent-harness-permissions]] — the 5-level permission model.
//! - [[agent-harness-transport]] — MCP-first in core, Tauri bridge as phase 2.
//! - [[agent-harness-tool-surface]] — the v1 tool catalog.
//!
//! This module ships the **skeleton** (types, traits, registry shell). Tool
//! implementations and the MCP transport land in follow-up PRs. The goal of
//! the skeleton is to lock the interface every later piece must conform to:
//!
//! - [`Permission`] — the level a session carries.
//! - [`AgentTool`] / [`ToolDescriptor`] — what a tool must implement.
//! - [`Registry`] — the single source of truth holding registered tools,
//!   running the permission check + input validation + audit append before
//!   `invoke()` ever sees the call.
//!
//! Nothing in this module performs I/O on its own. Tools call into the
//! existing `engine::SimHandle` and `Cortex` handle for that.

pub mod permission;
pub mod registry;
pub mod tool;

pub use permission::Permission;
pub use registry::{Registry, RegistryError};
pub use tool::{AgentTool, ToolContext, ToolDescriptor, ToolError};
