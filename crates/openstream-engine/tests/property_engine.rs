//! Property tests for engine validation and execution invariants
//! (`TECHNICAL_SPEC` §10: Rust property tests with fake clocks).

mod common;

use common::*;
use openstream_domain::capability::Capability;
use openstream_engine::{
    ActionRegistry, EdgeKindInput, FailurePolicy, MessageId, NodeKind, RawGraph, ValidatedGraph,
};
use proptest::prelude::*;
use std::sync::Arc;

/// Deterministic LCG so every case is reproducible from its seed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }
}

fn fixture_registry(harness: &Harness) -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        true,
        true,
        ScriptedPort::new(
            vec![common::Step::Ok],
            Arc::clone(&harness.events),
            harness.clock.clone(),
        ),
    );
    registry
}

/// Builds a pseudo-random container tree of `size` leaves.
fn random_tree(seed: u64, size: usize) -> (RawGraph, Vec<String>, Vec<String>) {
    let mut rng = Lcg(seed);
    let mut raw = RawGraph::new(FailurePolicy::Stop);
    let mut leaves: Vec<String> = Vec::new();
    let mut containers: Vec<String> = Vec::new();
    let mut counter = 0usize;

    // Root container.
    let root_kind = if rng.below(2) == 0 {
        NodeKind::Sequence
    } else {
        NodeKind::Parallel
    };
    raw.add_node(node_key("root"), root_kind).unwrap();
    containers.push("root".to_string());
    let mut frontier: Vec<String> = vec!["root".to_string()];
    let mut placed = 0usize;

    while !frontier.is_empty() && placed < size {
        placed += 1;
        let slot = rng.below(frontier.len() as u64) as usize;
        let parent_key = frontier.swap_remove(slot);
        let key = format!("n{counter}");
        counter += 1;
        // Children always attach to their container (edge semantics derive
        // from the source node kind).
        raw.add_edge(
            node_key(&parent_key),
            node_key(&key),
            EdgeKindInput::Sequence,
        );
        match rng.below(3) {
            1 => {
                let kind = if rng.below(2) == 0 {
                    NodeKind::Sequence
                } else {
                    NodeKind::Parallel
                };
                raw.add_node(node_key(&key), kind).unwrap();
                containers.push(key.clone());
                frontier.push(key.clone());
            }
            _ => {
                raw.add_node(
                    node_key(&key),
                    NodeKind::Action {
                        action_type: "midi.tap".to_string(),
                        capability: Capability::MidiSend {
                            device: "stagepad".to_string(),
                        },
                        params: serde_json::Value::Null,
                        deadline_override_ms: None,
                    },
                )
                .unwrap();
                leaves.push(key);
            }
        }
    }
    (raw, leaves, containers)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_trees_validate_and_run_to_terminal(seed in any::<u64>(), size in 1usize..12) {
        let harness = Harness::new();
        let registry = fixture_registry(&harness);
        let (mut raw, _leaves, _containers) = random_tree(seed, size);

        raw.entry(node_key("root"));

        match ValidatedGraph::build(&raw, &registry) {
            Ok(graph) => {
                let mut runtime =
                    harness.runtime(registry.clone(), ledger_with(&[midi("stagepad")]));
                let request = openstream_engine::ExecuteRequest {
                    source_device_id: device(),
                    message_id: MessageId::generate(),
                    subject: subject(),
                    graph: Arc::new(graph),
                    variables: Default::default(),
                    expires_at_wall_ms: harness.expires_at(),
                    cancel: None::<openstream_engine::CancelSignal>,
                };
                let receipt = runtime.execute(request).unwrap();
                assert!(matches!(
                    receipt.state.token(),
                    "succeeded" | "failed" | "cancelled" | "expired" | "outcome_unknown"
                ));
                let node_total = receipt.effects.len();
                prop_assert!(node_total <= openstream_engine::MAX_GRAPH_NODES);
            }
            Err(error) => {
                // Rejection must be structural, never silent success.
                let text = error.to_string();
                prop_assert!(!text.is_empty());
            }
        }
    }
    #[test]
    fn back_edges_always_reject_as_cycles(seed in any::<u64>()) {
        let harness = Harness::new();
        let registry = fixture_registry(&harness);
        let (mut raw, _leaves, containers) = random_tree(seed, 4);

        raw.entry(node_key("root"));

        // Wire a nested container back into the root over a legal-shaped
        // sequence edge: the union graph now cycles unconditionally.
        if let Some(container) = containers.iter().rev().find(|name| name != &"root") {
            raw.add_edge(node_key(container), node_key("root"), EdgeKindInput::Sequence);
            assert!(matches!(
                ValidatedGraph::build(&raw, &registry),
                Err(openstream_engine::ValidationError::CycleDetected)
            ));
        }
    }

    #[test]
    fn node_limit_is_exact(limit in 127usize..130usize) {
        let harness = Harness::new();
        let mut registry = ActionRegistry::new();
        register_action(
            &mut registry,
            "midi.tap",
            vec![midi("stagepad")],
            false,
            false,
            ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone()),
        );
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(node_key("fan"), NodeKind::Parallel).unwrap();
        let total = limit - 1; // + parallel root
        for index in 0..total {
            let key = format!("n{index}");
            raw.add_node(node_key(&key), NodeKind::Action {
                action_type: "midi.tap".to_string(),
                capability: midi("stagepad"),
                params: serde_json::Value::Null,
                deadline_override_ms: None,
            }).unwrap();
            raw.add_edge(node_key("fan"), node_key(&key), EdgeKindInput::Sequence);
        }
        raw.entry(node_key("fan"));
        let built = ValidatedGraph::build(&raw, &registry);
        if total < openstream_engine::MAX_GRAPH_NODES {
            prop_assert!(built.is_ok());
        } else {
            assert!(matches!(
                built,
                Err(openstream_engine::ValidationError::NodeLimitExceeded { .. })
            ));
        }
    }

    #[test]
    fn depth_limit_is_exact(containers in 15usize..18usize) {
        let harness = Harness::new();
        let mut registry = ActionRegistry::new();
        register_action(
            &mut registry,
            "midi.tap",
            vec![midi("stagepad")],
            false,
            false,
            ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone()),
        );
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        // Chain of sequence containers; the innermost holds one action.
        for index in 0..containers {
            let key = format!("s{index}");
            raw.add_node(node_key(&key), NodeKind::Sequence).unwrap();
            if index > 0 {
                let parent = format!("s{}", index - 1);
                // Each container holds the next container plus nothing else;
                // single-child sequences are valid.
                raw.add_edge(node_key(&parent), node_key(&key), EdgeKindInput::Sequence);
            }
        }
        let leaf_key = format!("a{containers}");
        raw.add_node(node_key(&leaf_key), NodeKind::Action {
            action_type: "midi.tap".to_string(),
            capability: midi("stagepad"),
            params: serde_json::Value::Null,
            deadline_override_ms: None,
        }).unwrap();
        let last = format!("s{}", containers - 1);
        raw.add_edge(node_key(&last), node_key(&leaf_key), EdgeKindInput::Sequence);
        raw.entry(node_key("s0"));

        let built = ValidatedGraph::build(&raw, &registry);
        if containers <= openstream_engine::MAX_GRAPH_DEPTH {
            prop_assert!(built.is_ok());
        } else {
            assert!(matches!(
                built,
                Err(openstream_engine::ValidationError::DepthLimitExceeded { .. })
            ));
        }
    }
}
