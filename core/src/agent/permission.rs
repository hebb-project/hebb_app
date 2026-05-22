//! Permission levels for agent sessions.
//!
//! See [[agent-harness-permissions]] in the vault for the full design. Each
//! level is a strict superset of the previous; a session at `TopologyWrite`
//! can invoke any tool requiring `ReadOnly`, `ParameterWrite`, or
//! `TopologyWrite` but not `Stimulate` or `FilePersistence`.
//!
//! Levels are ordered by `u8` discriminant so `>=` checks are the natural
//! authorization primitive.

use serde::{Deserialize, Serialize};

/// The five session permission levels, ordered from least to most powerful.
///
/// The numeric values are stable (treated as part of the audit log format)
/// and must not be reordered without a format-version bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Permission {
    /// Inspect topology, weights, params, current open folder, recent
    /// spikes/events. Never audited — read-only access cannot harm the
    /// network. Tools at this level may be invoked from any session.
    ReadOnly = 0,
    /// Mutate neuron or synapse parameters via `set_param`. Audited.
    ParameterWrite = 1,
    /// Add or remove neurons and synapses; apply seeds. Audited.
    TopologyWrite = 2,
    /// Inject current or force spikes into the running engine. Audited.
    Stimulate = 3,
    /// Flush weights, save state, rename network, switch open folder.
    /// Includes irreversible file mutations — requires explicit per-session
    /// confirmation in the desktop UX. Audited.
    FilePersistence = 4,
}

impl Permission {
    /// True if a session at `self` is allowed to invoke a tool that requires
    /// `required`. Equivalent to `self >= required`; kept as a named method
    /// because the call-site `session.allows(tool.permission)` reads more
    /// clearly than a bare comparison.
    pub fn allows(self, required: Permission) -> bool {
        self >= required
    }

    /// True if a tool call at this permission level must be appended to
    /// `events.jsonl`. ReadOnly is never audited; everything else is.
    pub fn is_audited(self) -> bool {
        self >= Permission::ParameterWrite
    }

    /// Stable lowercase tag for logs and UI. Matches the serde repr.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::ParameterWrite => "parameter_write",
            Self::TopologyWrite => "topology_write",
            Self::Stimulate => "stimulate",
            Self::FilePersistence => "file_persistence",
        }
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_ordering_matches_design() {
        assert!(Permission::ReadOnly < Permission::ParameterWrite);
        assert!(Permission::ParameterWrite < Permission::TopologyWrite);
        assert!(Permission::TopologyWrite < Permission::Stimulate);
        assert!(Permission::Stimulate < Permission::FilePersistence);
    }

    #[test]
    fn higher_session_allows_lower_tool() {
        assert!(Permission::TopologyWrite.allows(Permission::ReadOnly));
        assert!(Permission::TopologyWrite.allows(Permission::ParameterWrite));
        assert!(Permission::TopologyWrite.allows(Permission::TopologyWrite));
    }

    #[test]
    fn lower_session_denies_higher_tool() {
        assert!(!Permission::ReadOnly.allows(Permission::ParameterWrite));
        assert!(!Permission::ParameterWrite.allows(Permission::TopologyWrite));
        assert!(!Permission::TopologyWrite.allows(Permission::Stimulate));
        assert!(!Permission::Stimulate.allows(Permission::FilePersistence));
    }

    #[test]
    fn read_only_is_not_audited() {
        assert!(!Permission::ReadOnly.is_audited());
        assert!(Permission::ParameterWrite.is_audited());
        assert!(Permission::FilePersistence.is_audited());
    }

    #[test]
    fn serde_round_trips_via_snake_case() {
        let json = serde_json::to_string(&Permission::TopologyWrite).unwrap();
        assert_eq!(json, r#""topology_write""#);
        let back: Permission = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Permission::TopologyWrite);
    }

    #[test]
    fn as_str_matches_serde() {
        for p in [
            Permission::ReadOnly,
            Permission::ParameterWrite,
            Permission::TopologyWrite,
            Permission::Stimulate,
            Permission::FilePersistence,
        ] {
            let via_serde = serde_json::to_value(p).unwrap();
            assert_eq!(via_serde.as_str().unwrap(), p.as_str());
        }
    }
}
