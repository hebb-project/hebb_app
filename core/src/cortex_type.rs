//! The user-facing cortex-type taxonomy.
//!
//! A *cortex type* is the answer to "what kind of network is this?"
//! that the desktop create-network screen offers (knowledge graph vs.
//! LIF spiking vs. Hodgkin-Huxley spiking). It also lives verbatim in
//! `.cortex/metadata.json` so opening a folder picks the right
//! visualization + simulator behavior.
//!
//! `CortexType` is *not* the same thing as [`cortex_snn::NeuronKind`].
//! Some cortex types share an underlying neuron model — both
//! `knowledge-graph` and `lif` cortexes run on LIF neurons today; the
//! difference is provenance (vault-ingested vs. user-built). The
//! mapping function [`CortexType::neuron_kind`] collapses the
//! distinction at the simulator boundary.

use serde::{Deserialize, Serialize};

use cortex_snn::{HhConfig, NeuronKind};

/// The type a user picks when creating a network. Serialized as a
/// tagged-kebab-case enum so it lines up with `.cortex/metadata.json`
/// and with the request bodies the desktop UI posts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CortexType {
    /// Obsidian-style knowledge graph. Notes become nodes; the engine
    /// runs LIF neurons over the imported topology.
    KnowledgeGraph,
    /// Fresh spiking network using leaky integrate-and-fire neurons.
    /// Cheap to scale; what the simulator has shipped since M0.
    Lif,
    /// Biophysical Hodgkin-Huxley neurons. Richer dynamics, higher
    /// per-tick cost. `config` carries integrator + HK1952 defaults.
    Hh {
        #[serde(default)]
        config: HhConfig,
    },
}

impl CortexType {
    /// Translate to the simulator-level neuron kind. Knowledge-graph
    /// and LIF cortexes both run LIF neurons; HH cortexes carry their
    /// config straight through.
    pub fn neuron_kind(&self) -> NeuronKind {
        match self {
            Self::KnowledgeGraph | Self::Lif => NeuronKind::Lif,
            Self::Hh { config } => NeuronKind::Hh(config.clone()),
        }
    }

    /// Short stable identifier — what we persist into `metadata.json`'s
    /// `cortex_type` discriminator and surface to logs.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::KnowledgeGraph => "knowledge-graph",
            Self::Lif => "lif",
            Self::Hh { .. } => "hh",
        }
    }

    /// Reconstruct a `CortexType` from the slug + optional HH config
    /// blob persisted in `metadata.json`. Returns `None` for unknown
    /// slugs — caller decides whether to default-fallback or surface
    /// the error.
    pub fn from_slug_and_config(slug: &str, hh_config: Option<&serde_json::Value>) -> Option<Self> {
        match slug {
            "knowledge-graph" => Some(Self::KnowledgeGraph),
            "lif" => Some(Self::Lif),
            "hh" => {
                let config = match hh_config {
                    Some(v) => serde_json::from_value(v.clone()).ok()?,
                    None => HhConfig::default(),
                };
                Some(Self::Hh { config })
            }
            _ => None,
        }
    }
}

impl Default for CortexType {
    fn default() -> Self {
        Self::Lif
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_snn::HhIntegrator;

    #[test]
    fn lif_serializes_as_kebab_kind() {
        let v = serde_json::to_value(CortexType::Lif).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "lif"}));
    }

    #[test]
    fn hh_round_trips_with_config() {
        let original = CortexType::Hh {
            config: HhConfig {
                integrator: HhIntegrator::Rk4,
                ..HhConfig::default()
            },
        };
        let v = serde_json::to_value(&original).unwrap();
        let back: CortexType = serde_json::from_value(v).unwrap();
        match back {
            CortexType::Hh { config } => assert_eq!(config.integrator, HhIntegrator::Rk4),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn hh_slug_accepts_partial_metadata_config() {
        let ct = CortexType::from_slug_and_config(
            "hh",
            Some(&serde_json::json!({ "integrator": "rk4" })),
        )
        .expect("partial HH metadata config should overlay defaults");
        match ct {
            CortexType::Hh { config } => {
                assert_eq!(config.integrator, HhIntegrator::Rk4);
                assert_eq!(config.c_m, HhConfig::default().c_m);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn hh_neuron_kind_carries_config() {
        let cfg = HhConfig {
            integrator: HhIntegrator::Rk4,
            ..HhConfig::default()
        };
        let ct = CortexType::Hh { config: cfg };
        match ct.neuron_kind() {
            NeuronKind::Hh(c) => assert_eq!(c.integrator, HhIntegrator::Rk4),
            _ => panic!("HH cortex did not map to HH neuron kind"),
        }
    }

    #[test]
    fn kg_maps_to_lif_neurons() {
        assert!(matches!(
            CortexType::KnowledgeGraph.neuron_kind(),
            NeuronKind::Lif
        ));
    }
}
