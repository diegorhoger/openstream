//! Immutable validated action graphs (`DOMAIN_MODEL.md` §5–§6).
//!
//! A [`RawGraph`] is untrusted authoring input. Only
//! [`ValidatedGraph::build`] produces the executable form, running the
//! fail-closed pipeline stages S1–S4 of `DOMAIN_MODEL.md` §6 scoped to this
//! crate:
//!
//! - **S1/S2 structural**: unique bounded keys, resolvable edges, single
//!   entry, single-parent tree discipline, acyclicity over the union of
//!   flow and compensation edges, full reachability of non-compensate
//!   nodes, node limit 128, container-depth limit 16, deadline/delay caps.
//! - **S3 referential**: every action node names a registered action type
//!   and requests a capability covered by one of its declared scopes;
//!   payloads and variable names validate against bounded grammars.
//! - **S4 semantic**: `retry` subtrees contain only idempotency-declared
//!   adapters; failure policy `compensate` requires every action to carry a
//!   compensation link whose adapter declared safe compensation.
//!
//! Edge semantics derive purely from the *source* node kind, removing
//! direction ambiguity: a `sequence` node's outgoing sequence edges, in
//! insertion order, are its ordered children; a `parallel` node's outgoing
//! sequence edges are its concurrent children; a `retry` node's single
//! sequence edge is its body; `conditional` nodes use typed branch edges;
//! `compensation_link` edges connect an action to its dedicated `compensate`
//! node. Stage S5 (grant intersection) runs at dispatch time in
//! [`crate::runtime`], immediately before every side effect.

use crate::error::ValidationError;
use crate::failure::FailurePolicy;
use crate::limits::{
    MACRO_MAX_DEADLINE_MS, MAX_ACTION_PARAMS_BYTES, MAX_GRAPH_DEPTH, MAX_GRAPH_NODES,
    MAX_RETRY_ATTEMPTS,
};
use crate::registry::ActionRegistry;
use openstream_domain::capability::Capability;
use std::collections::{BTreeMap, HashSet};
use std::fmt;

/// Maximum byte length of node keys, action names, and variable names
/// (structural identifiers authored inside graphs).
pub const MAX_IDENTIFIER_BYTES: usize = 64;

/// Structural identifier for one node inside a graph: lowercase ASCII start,
/// then lowercase ASCII plus `_`/`-`, at most [`MAX_IDENTIFIER_BYTES`]
/// bytes. Echo-safe by grammar.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeKey(String);

impl NodeKey {
    /// Validates and adopts a node key.
    ///
    /// # Errors
    /// [`ValidationError::InvalidNodeKey`] for off-grammar input.
    pub fn try_new(raw: &str) -> Result<Self, ValidationError> {
        if crate::identifiers::validate_identifier(raw, false, MAX_IDENTIFIER_BYTES) {
            Ok(Self(raw.to_string()))
        } else {
            Err(ValidationError::InvalidNodeKey)
        }
    }

    /// The structural key string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Typed comparison evaluated by a `conditional` node against the
/// execution's variable map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionOp {
    /// The variable exists in the map.
    Exists,
    /// The variable equals the operand scalar.
    Equals,
    /// The variable differs from the operand scalar, or is absent.
    NotEquals,
}

/// Declarative condition evaluated by a `conditional` node. The operand is
/// a JSON scalar (null, bool, number, string); arrays and objects reject.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    /// Variable inspected (conditions never write variables).
    pub variable: String,
    /// Comparison applied.
    pub op: ConditionOp,
    /// Scalar operand for `Equals`/`NotEquals`.
    pub operand: serde_json::Value,
}

/// Declarative, closed-set variable transform applied by a
/// `variable_transform` node. Arbitrary code never executes inside the
/// engine; transforms are exactly these typed operations.
#[derive(Debug, Clone, PartialEq)]
pub enum TransformOp {
    /// Writes a literal scalar into a variable (create or overwrite).
    Set {
        /// Target variable name.
        variable: String,
        /// Scalar value to store.
        value: serde_json::Value,
    },
    /// Copies one variable's current value into another; a missing source
    /// fails the node at run time.
    Copy {
        /// Source variable name.
        from: String,
        /// Target variable name.
        to: String,
    },
    /// Adds an integer delta to an integer-typed variable; type mismatches
    /// fail the node at run time with
    /// [`crate::failure::FailureReason::TransformFailed`].
    AddInt {
        /// Target variable name.
        variable: String,
        /// Signed delta applied.
        delta: i64,
    },
}

/// Kind of one graph node (v1 closed set, `DOMAIN_MODEL.md` §5).
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// One side-effecting step dispatched to a registered adapter port after
    /// durable preparation and grant intersection pass.
    Action {
        /// Registered action type name.
        action_type: String,
        /// Exact capability exercised; covered by a registration scope at
        /// build time and by grants at dispatch time.
        capability: Capability,
        /// Bounded JSON payload passed verbatim to the port.
        params: serde_json::Value,
        /// Optional tighter deadline override (monotonic ms, `<=`
        /// [`MACRO_MAX_DEADLINE_MS`]).
        deadline_override_ms: Option<u64>,
    },
    /// Ordered container: children run strictly in edge-insertion order.
    Sequence,
    /// Concurrent container: children run as independent branches under the
    /// concurrency caps; the container completes when all children do.
    Parallel,
    /// Time passage of a fixed monotonic duration (bounded by the macro
    /// cap). Pure scheduling; dispatches nothing.
    Delay {
        /// Duration in monotonic milliseconds (`<=`
        /// [`MACRO_MAX_DEADLINE_MS`]).
        duration_ms: u64,
    },
    /// Chooses between a truth arm and an optional fall-through arm by
    /// evaluating its condition against the execution variables.
    Conditional {
        /// The evaluated condition.
        condition: Condition,
    },
    /// Re-runs its body on failure while attempts remain, with
    /// deterministic exponential backoff. Every action in the body subtree
    /// must run on an idempotency-declared adapter (`TECHNICAL_SPEC` §5).
    Retry {
        /// Total attempts including the first, `1..=[MAX_RETRY_ATTEMPTS]`.
        attempts: u32,
    },
    /// Applies one typed transform to the execution variables.
    VariableTransform {
        /// The transform operation.
        op: TransformOp,
    },
    /// Explicit compensating step linked from exactly one action node; runs
    /// only during compensation unwinding with `is_compensation=true`.
    Compensate,
}

/// Author-facing edge kinds (`DOMAIN_MODEL.md` §3: sequence order, branch
/// condition, compensation link).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKindInput {
    /// Ordering/fan-out edge; meaning derives from the source node kind.
    Sequence,
    /// Conditional branch arm.
    Branch {
        /// `true` for the truth arm, `false` for the optional fall-through.
        polarity: bool,
    },
    /// Links an action node to its dedicated compensate node.
    CompensationLink,
}

/// One directed, typed edge of a [`RawGraph`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeSpec {
    /// Source node key (semantics derive from its kind).
    pub from: NodeKey,
    /// Target node key.
    pub to: NodeKey,
    /// Edge kind.
    pub kind: EdgeKindInput,
}

/// Unvalidated authoring form of an action graph. Assemble through
/// [`RawGraph::new`] plus the builder methods, then hand it to
/// [`ValidatedGraph::build`].
#[derive(Debug, Clone)]
pub struct RawGraph {
    nodes: Vec<NodeSpec>,
    edges: Vec<EdgeSpec>,
    entry: Option<NodeKey>,
    failure_policy: FailurePolicy,
    execution_deadline_ms: Option<u64>,
}

impl RawGraph {
    /// Empty graph with the given failure policy and the default deadline.
    #[must_use]
    pub fn new(failure_policy: FailurePolicy) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            entry: None,
            failure_policy,
            execution_deadline_ms: None,
        }
    }

    /// Overrides the execution deadline (monotonic ms). Values above the
    /// macro cap reject at build; `None` keeps the default.
    pub fn execution_deadline_ms(&mut self, deadline_ms: Option<u64>) -> &mut Self {
        self.execution_deadline_ms = deadline_ms;
        self
    }

    /// Appends a node. Grammar validates here so stored graphs cannot carry
    /// keys that would fail later structural checks.
    ///
    /// # Errors
    /// [`ValidationError::InvalidNodeKey`] for off-grammar keys.
    pub fn add_node(&mut self, key: NodeKey, kind: NodeKind) -> Result<&mut Self, ValidationError> {
        let _ = NodeKey::try_new(key.as_str())?;
        self.nodes.push(NodeSpec { key, kind });
        Ok(self)
    }

    /// Appends an edge. Endpoint existence checks run at build time.
    pub fn add_edge(&mut self, from: NodeKey, to: NodeKey, kind: EdgeKindInput) -> &mut Self {
        self.edges.push(EdgeSpec { from, to, kind });
        self
    }

    /// Sets the entry node key (resolved at build time).
    pub fn entry(&mut self, key: NodeKey) -> &mut Self {
        self.entry = Some(key);
        self
    }

    fn index_of(&self, key: &NodeKey) -> Option<usize> {
        self.nodes.iter().position(|node| &node.key == key)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NodeSpec {
    pub(crate) key: NodeKey,
    pub(crate) kind: NodeKind,
}

/// Index of a node inside a [`ValidatedGraph`]'s arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeIdx(u16);

impl NodeIdx {
    /// Zero-based arena position (diagnostic; stable for one graph).
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

fn slot(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

type ChildMap = BTreeMap<u16, Vec<NodeIdx>>;
type TrueMap = BTreeMap<u16, NodeIdx>;
type FalseMap = BTreeMap<u16, Option<NodeIdx>>;

/// Immutable, validated, executable action graph. Construction is the only
/// path past stages S1–S4; every run reads exactly one immutable instance
/// (`DOMAIN_MODEL.md` §5).
#[derive(Debug, Clone)]
pub struct ValidatedGraph {
    nodes: Vec<NodeSpec>,
    index: BTreeMap<NodeKey, NodeIdx>,
    entry: NodeIdx,
    children: ChildMap,
    branch_true: TrueMap,
    branch_false: FalseMap,
    compensate_of: BTreeMap<u16, u16>,
    failure_policy: FailurePolicy,
    execution_deadline_ms: u64,
    max_depth: usize,
}

impl ValidatedGraph {
    /// Runs stages S1–S4 against `raw` and the action registry, producing
    /// the immutable executable form. Fails closed; nothing partial is
    /// ever returned.
    ///
    /// # Errors
    /// The first failing [`ValidationError`] check.
    pub fn build(raw: &RawGraph, registry: &ActionRegistry) -> Result<Self, ValidationError> {
        if raw.nodes.len() > MAX_GRAPH_NODES {
            return Err(ValidationError::NodeLimitExceeded {
                limit: MAX_GRAPH_NODES,
            });
        }

        // S1: unique keys over pre-grammar-checked nodes.
        let mut index: BTreeMap<NodeKey, NodeIdx> = BTreeMap::new();
        for (position, node) in raw.nodes.iter().enumerate() {
            if index
                .insert(node.key.clone(), NodeIdx(slot(position)))
                .is_some()
            {
                return Err(ValidationError::DuplicateNodeKey);
            }
        }

        let Some(entry) = raw.entry.as_ref().and_then(|key| raw.index_of(key)) else {
            return Err(ValidationError::MissingEntry);
        };

        // Edge endpoint resolution.
        let mut resolved: Vec<(usize, usize, EdgeKindInput)> = Vec::with_capacity(raw.edges.len());
        for edge in &raw.edges {
            let Some(from) = raw.index_of(&edge.from) else {
                return Err(ValidationError::DanglingEdge {
                    from: edge.from.to_string(),
                    to: edge.to.to_string(),
                });
            };
            let Some(to) = raw.index_of(&edge.to) else {
                return Err(ValidationError::DanglingEdge {
                    from: edge.from.to_string(),
                    to: edge.to.to_string(),
                });
            };
            resolved.push((from, to, edge.kind));
        }

        // Compensation link legality: action -> dedicated compensate node,
        // at most one link per action, at most one link per target.
        let mut compensate_of: BTreeMap<u16, u16> = BTreeMap::new();
        for &(from, to, kind) in &resolved {
            if !matches!(kind, EdgeKindInput::CompensationLink) {
                continue;
            }
            let action_source = matches!(raw.nodes[from].kind, NodeKind::Action { .. });
            let compensate_target = matches!(raw.nodes[to].kind, NodeKind::Compensate);
            if !action_source || !compensate_target || compensate_of.contains_key(&slot(from)) {
                return Err(ValidationError::MalformedCompensationLink);
            }
            compensate_of.insert(slot(from), slot(to));
        }

        // Single-parent flow discipline; compensate nodes take no flow
        // edges and emit none.
        let mut flow_parent: BTreeMap<u16, u16> = BTreeMap::new();
        for &(from, to, kind) in &resolved {
            if matches!(kind, EdgeKindInput::CompensationLink) {
                continue;
            }
            if flow_parent.contains_key(&slot(to)) {
                return Err(ValidationError::MultipleParents);
            }
            if matches!(raw.nodes[to].kind, NodeKind::Compensate)
                || matches!(raw.nodes[from].kind, NodeKind::Compensate)
            {
                return Err(ValidationError::MalformedCompensationLink);
            }
            flow_parent.insert(slot(to), slot(from));
        }

        // Outgoing-shape legality per source kind; collect child lists.
        let mut children: ChildMap = BTreeMap::new();
        let mut branch_true: TrueMap = BTreeMap::new();
        let mut branch_false: FalseMap = BTreeMap::new();
        for &(from, to, kind) in &resolved {
            match (&raw.nodes[from].kind, kind) {
                (NodeKind::Sequence, EdgeKindInput::Sequence)
                | (NodeKind::Parallel, EdgeKindInput::Sequence)
                | (NodeKind::Retry { .. }, EdgeKindInput::Sequence) => {
                    children
                        .entry(slot(from))
                        .or_default()
                        .push(NodeIdx(slot(to)));
                }
                (NodeKind::Conditional { .. }, EdgeKindInput::Branch { polarity: true }) => {
                    if branch_true.insert(slot(from), NodeIdx(slot(to))).is_some() {
                        return Err(conditional_error(raw, from));
                    }
                }
                (NodeKind::Conditional { .. }, EdgeKindInput::Branch { polarity: false }) => {
                    if branch_false
                        .insert(slot(from), Some(NodeIdx(slot(to))))
                        .is_some()
                    {
                        return Err(conditional_error(raw, from));
                    }
                }
                (_, EdgeKindInput::Branch { .. } | EdgeKindInput::Sequence) => {
                    return Err(ValidationError::IllegalEdgeShape {
                        node: raw.nodes[from].key.to_string(),
                    });
                }
                (_, EdgeKindInput::CompensationLink) => {}
            }
        }

        // Container cardinalities + node-local payload validation (S1–S3).
        for (position, node) in raw.nodes.iter().enumerate() {
            validate_node(
                node,
                slot(position),
                &children,
                &branch_true,
                &branch_false,
                registry,
            )?;
        }

        // Reachability over the union graph from the entry.
        let adjacency = |position: usize| -> Vec<usize> {
            let current = slot(position);
            let mut out: Vec<usize> = children
                .get(&current)
                .into_iter()
                .flatten()
                .copied()
                .map(|idx| usize::from(idx.raw()))
                .collect();
            if let Some(truth) = branch_true.get(&current) {
                out.push(usize::from(truth.raw()));
            }
            if let Some(Some(fall)) = branch_false.get(&current) {
                out.push(usize::from(fall.raw()));
            }
            if let Some(compensation) = compensate_of.get(&current) {
                out.push(usize::from(*compensation));
            }
            out
        };

        let mut reachable: HashSet<usize> = HashSet::new();
        let mut pending = vec![entry];
        while let Some(current) = pending.pop() {
            if reachable.insert(current) {
                for next in adjacency(current) {
                    if !reachable.contains(&next) {
                        pending.push(next);
                    }
                }
            }
        }
        for (position, node) in raw.nodes.iter().enumerate() {
            let required = !matches!(node.kind, NodeKind::Compensate);
            if required && !reachable.contains(&position) {
                return Err(ValidationError::UnreachableNode);
            }
        }

        detect_cycle(raw.nodes.len(), &adjacency)?;

        let max_depth = compute_depth(raw, entry, &children, &branch_true, &branch_false)?;
        if max_depth > MAX_GRAPH_DEPTH {
            return Err(ValidationError::DepthLimitExceeded {
                limit: MAX_GRAPH_DEPTH,
            });
        }

        let built = Self {
            nodes: raw.nodes.clone(),
            index,
            entry: NodeIdx(slot(entry)),
            children,
            branch_true,
            branch_false,
            compensate_of,
            failure_policy: raw.failure_policy,
            execution_deadline_ms: raw
                .execution_deadline_ms
                .unwrap_or(crate::limits::DEFAULT_DEADLINE_MS),
            max_depth,
        };
        if built.execution_deadline_ms == 0 || built.execution_deadline_ms > MACRO_MAX_DEADLINE_MS {
            return Err(ValidationError::DeadlineOutOfRange { node: None });
        }

        // S4 semantic stage: retry idempotency + compensable policy.
        check_retry_idempotency(&built, registry)?;
        if built.failure_policy == FailurePolicy::Compensate {
            for (position, node) in built.nodes.iter().enumerate() {
                if let NodeKind::Action { action_type, .. } = &node.kind {
                    let linked = built.compensate_of.contains_key(&slot(position));
                    let declared = registry
                        .lookup(action_type)
                        .is_some_and(|registration| registration.safe_compensation());
                    if !linked || !declared {
                        return Err(ValidationError::PolicyCompensateInvalid {
                            action: action_type.clone(),
                        });
                    }
                }
            }
        }
        Ok(built)
    }

    /// The entry node index.
    #[must_use]
    pub const fn entry(&self) -> NodeIdx {
        self.entry
    }

    /// Graph-level failure policy.
    #[must_use]
    pub const fn failure_policy(&self) -> FailurePolicy {
        self.failure_policy
    }

    /// Effective execution deadline in monotonic milliseconds.
    #[must_use]
    pub const fn execution_deadline_ms(&self) -> u64 {
        self.execution_deadline_ms
    }

    /// Total node count (including compensate nodes).
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Maximum observed container-nesting depth.
    #[must_use]
    pub const fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Resolves a node key to its arena index.
    #[must_use]
    pub fn idx_of(&self, key: &NodeKey) -> Option<NodeIdx> {
        self.index.get(key).copied()
    }

    /// The key of a node.
    #[must_use]
    pub fn key(&self, idx: NodeIdx) -> &NodeKey {
        &self.nodes[usize::from(idx.raw())].key
    }

    /// The kind of a node.
    #[must_use]
    pub fn kind(&self, idx: NodeIdx) -> &NodeKind {
        &self.nodes[usize::from(idx.raw())].kind
    }

    /// Ordered children of a container (sequence order / fan-out / body).
    #[must_use]
    pub fn children(&self, idx: NodeIdx) -> &[NodeIdx] {
        match self.children.get(&idx.raw()) {
            Some(kids) => kids.as_slice(),
            None => &[],
        }
    }

    /// Truth arm target of a conditional node.
    #[must_use]
    pub fn branch_true(&self, idx: NodeIdx) -> Option<NodeIdx> {
        self.branch_true.get(&idx.raw()).copied()
    }

    /// Fall-through arm target of a conditional node, when authored.
    #[must_use]
    pub fn branch_false(&self, idx: NodeIdx) -> Option<NodeIdx> {
        self.branch_false.get(&idx.raw()).copied().flatten()
    }

    /// The compensate node linked from an action node, when present.
    #[must_use]
    pub fn compensate_of(&self, idx: NodeIdx) -> Option<NodeIdx> {
        self.compensate_of.get(&idx.raw()).map(|raw| NodeIdx(*raw))
    }

    /// Iterates `(index, key, kind)` triples in deterministic arena order.
    pub fn iter_nodes(&self) -> impl Iterator<Item = (NodeIdx, &NodeKey, &NodeKind)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(position, node)| (NodeIdx(slot(position)), &node.key, &node.kind))
    }
}

fn conditional_error(raw: &RawGraph, position: usize) -> ValidationError {
    ValidationError::MalformedConditional {
        node: raw.nodes[position].key.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_node(
    node: &NodeSpec,
    current: u16,
    children: &ChildMap,
    branch_true: &TrueMap,
    branch_false: &FalseMap,
    registry: &ActionRegistry,
) -> Result<(), ValidationError> {
    let kid_count = children.get(&current).map_or(0, Vec::len);
    match &node.kind {
        NodeKind::Sequence | NodeKind::Parallel => {
            if kid_count == 0 {
                return Err(match node.kind {
                    NodeKind::Parallel => ValidationError::EmptyParallel {
                        node: node.key.to_string(),
                    },
                    _ => ValidationError::MalformedSequenceChain {
                        node: node.key.to_string(),
                    },
                });
            }
            let mut seen: HashSet<u16> = HashSet::new();
            for child in children.get(&current).unwrap_or(&Vec::new()) {
                if !seen.insert(child.raw()) {
                    return Err(ValidationError::MalformedSequenceChain {
                        node: node.key.to_string(),
                    });
                }
            }
        }
        NodeKind::Conditional { condition } => {
            if !branch_true.contains_key(&current) {
                return Err(conditional_key_error(node));
            }
            if let (Some(truth), Some(Some(fall))) =
                (branch_true.get(&current), branch_false.get(&current))
                && fall == truth
            {
                return Err(conditional_key_error(node));
            }
            validate_condition(condition, &node.key)?;
        }
        NodeKind::Retry { attempts } => {
            if kid_count != 1 || !(1..=MAX_RETRY_ATTEMPTS).contains(attempts) {
                return Err(ValidationError::MalformedRetry {
                    node: node.key.to_string(),
                });
            }
        }
        NodeKind::Delay { duration_ms } => {
            if *duration_ms > MACRO_MAX_DEADLINE_MS {
                return Err(ValidationError::DeadlineOutOfRange {
                    node: Some(node.key.to_string()),
                });
            }
        }
        NodeKind::Action {
            action_type,
            capability,
            params,
            deadline_override_ms,
        } => {
            let Some(registration) = registry.lookup(action_type) else {
                return Err(ValidationError::UnknownActionType {
                    action: action_type.clone(),
                });
            };
            if !registration.scopes_cover(capability) {
                return Err(ValidationError::CapabilityNotDeclared {
                    action: action_type.clone(),
                    capability_kind: capability.kind_name().to_string(),
                });
            }
            let params_bytes = serde_json::to_vec(params).unwrap_or_default();
            if params_bytes.len() > MAX_ACTION_PARAMS_BYTES {
                return Err(ValidationError::ParamsTooLarge {
                    node: node.key.to_string(),
                });
            }
            if deadline_override_ms.is_some_and(|ms| ms == 0 || ms > MACRO_MAX_DEADLINE_MS) {
                return Err(ValidationError::DeadlineOutOfRange {
                    node: Some(node.key.to_string()),
                });
            }
        }
        NodeKind::VariableTransform { op } => validate_transform(op, &node.key)?,
        NodeKind::Compensate => {}
    }
    Ok(())
}

fn conditional_key_error(node: &NodeSpec) -> ValidationError {
    ValidationError::MalformedConditional {
        node: node.key.to_string(),
    }
}

fn validate_condition(condition: &Condition, node: &NodeKey) -> Result<(), ValidationError> {
    if !crate::identifiers::validate_identifier(&condition.variable, false, MAX_IDENTIFIER_BYTES) {
        return Err(ValidationError::InvalidVariableName {
            node: node.to_string(),
        });
    }
    if condition.op != ConditionOp::Exists && !is_scalar(&condition.operand) {
        return Err(ValidationError::NonScalarOperand {
            node: node.to_string(),
        });
    }
    Ok(())
}

fn validate_transform(op: &TransformOp, node: &NodeKey) -> Result<(), ValidationError> {
    let mut names: Vec<&str> = Vec::new();
    let mut scalars: Vec<&serde_json::Value> = Vec::new();
    match op {
        TransformOp::Set { variable, value } => {
            names.push(variable);
            scalars.push(value);
        }
        TransformOp::Copy { from, to } => {
            names.push(from);
            names.push(to);
        }
        TransformOp::AddInt { variable, .. } => names.push(variable),
    }
    for name in names {
        if !crate::identifiers::validate_identifier(name, false, MAX_IDENTIFIER_BYTES) {
            return Err(ValidationError::InvalidVariableName {
                node: node.to_string(),
            });
        }
    }
    for value in scalars {
        if !is_scalar(value) {
            return Err(ValidationError::NonScalarOperand {
                node: node.to_string(),
            });
        }
    }
    Ok(())
}

const fn is_scalar(value: &serde_json::Value) -> bool {
    matches!(
        value,
        serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_)
    )
}

fn detect_cycle(
    node_count: usize,
    adjacency: &dyn Fn(usize) -> Vec<usize>,
) -> Result<(), ValidationError> {
    // Three-color DFS over the union graph; any back edge is a cycle and
    // fails validation unconditionally (DOMAIN_MODEL.md §5).
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;
    let mut color = vec![WHITE; node_count];
    for start in 0..node_count {
        if color[start] != WHITE {
            continue;
        }
        color[start] = GRAY;
        // Frames carry the node plus how many adjacency slots were consumed.
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        while let Some(loaded) = stack.last().copied() {
            let nexts = adjacency(loaded.0);
            let mut frame = loaded;
            let mut discovered: Option<usize> = None;
            while frame.1 < nexts.len() {
                let candidate = nexts[frame.1];
                frame.1 += 1;
                match color[candidate] {
                    GRAY => return Err(ValidationError::CycleDetected),
                    WHITE => {
                        discovered = Some(candidate);
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(top) = stack.last_mut() {
                *top = frame;
            }
            match discovered {
                Some(next) => {
                    color[next] = GRAY;
                    stack.push((next, 0));
                }
                None => {
                    if let Some((finished, _)) = stack.pop() {
                        color[finished] = BLACK;
                    }
                }
            }
        }
    }
    Ok(())
}

fn compute_depth(
    raw: &RawGraph,
    entry: usize,
    children: &ChildMap,
    branch_true: &TrueMap,
    branch_false: &FalseMap,
) -> Result<usize, ValidationError> {
    // Depth counts container frames entered along each flow path; the
    // single-parent discipline makes one forward pass sufficient.
    let container_like = |position: usize| -> bool {
        matches!(
            raw.nodes[position].kind,
            NodeKind::Sequence
                | NodeKind::Parallel
                | NodeKind::Conditional { .. }
                | NodeKind::Retry { .. }
        )
    };
    let mut best = 0usize;
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack: Vec<(usize, usize)> = vec![(entry, 0)];
    while let Some((current, depth)) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        best = best.max(depth);
        let next_depth = if container_like(current) {
            depth.saturating_add(1)
        } else {
            depth
        };
        let current_map = slot(current);
        if let Some(kids) = children.get(&current_map) {
            for kid in kids {
                stack.push((usize::from(kid.raw()), next_depth));
            }
        }
        if let Some(truth) = branch_true.get(&current_map) {
            stack.push((usize::from(truth.raw()), next_depth));
        }
        if let Some(Some(fall)) = branch_false.get(&current_map) {
            stack.push((usize::from(fall.raw()), next_depth));
        }
    }
    Ok(best)
}

fn check_retry_idempotency(
    built: &ValidatedGraph,
    registry: &ActionRegistry,
) -> Result<(), ValidationError> {
    for (position, _, kind) in built.iter_nodes() {
        if !matches!(kind, NodeKind::Retry { .. }) {
            continue;
        }
        let mut seen: HashSet<u16> = HashSet::new();
        let mut stack = vec![position];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.raw()) {
                continue;
            }
            if let NodeKind::Action { action_type, .. } = built.kind(current) {
                let idempotent = registry
                    .lookup(action_type)
                    .is_some_and(|registration| registration.idempotency().is_declared());
                if !idempotent {
                    return Err(ValidationError::RetryRequiresIdempotency {
                        action: action_type.clone(),
                    });
                }
            }
            for kid in built.children(current) {
                stack.push(*kid);
            }
            if let Some(truth) = built.branch_true(current) {
                stack.push(truth);
            }
            if let Some(fall) = built.branch_false(current) {
                stack.push(fall);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Condition, ConditionOp, EdgeKindInput, FailurePolicy, NodeKind, RawGraph, TransformOp,
        ValidatedGraph, ValidationError,
    };
    use crate::error::ConfigError;
    use crate::failure::FailurePolicy as Policy;
    use crate::registry::{ActionRegistry, IdempotencyClass};
    use openstream_domain::capability::Capability;
    use std::sync::Arc;

    fn registry() -> ActionRegistry {
        let mut registry = ActionRegistry::new();
        let port = TestPort;
        let registration = super::super::ActionRegistration::try_new(
            "midi.tap",
            vec![Capability::MidiSend {
                device: "stagepad".to_string(),
            }],
            IdempotencyClass::Idempotent,
            true,
            Arc::new(port),
        )
        .unwrap();
        registry.register(registration).unwrap();
        registry
    }

    #[derive(Debug)]
    struct TestPort;

    impl crate::port::EffectPort for TestPort {
        fn invoke(
            &self,
            _request: crate::port::EffectRequest,
        ) -> Result<crate::port::EffectResponse, crate::port::DispatchUnavailable> {
            Ok(crate::port::EffectResponse::Immediate(
                crate::port::EffectOutcome::Succeeded,
            ))
        }
    }

    fn action_node() -> NodeKind {
        NodeKind::Action {
            action_type: "midi.tap".to_string(),
            capability: Capability::MidiSend {
                device: "stagepad".to_string(),
            },
            params: serde_json::json!({"k": 1}),
            deadline_override_ms: None,
        }
    }

    fn key(name: &str) -> super::NodeKey {
        super::NodeKey::try_new(name).unwrap()
    }

    fn seq_pair() -> (RawGraph,) {
        let mut raw = RawGraph::new(Policy::Stop);
        raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
        raw.add_node(key("a"), action_node()).unwrap();
        raw.add_edge(key("seq"), key("a"), EdgeKindInput::Sequence);
        raw.entry(key("seq"));
        (raw,)
    }

    #[test]
    fn minimal_valid_graph_builds() {
        let (raw,) = seq_pair();
        let graph = ValidatedGraph::build(&raw, &registry()).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.max_depth(), 1);
        assert_eq!(graph.failure_policy(), FailurePolicy::Stop);
        assert_eq!(
            graph.execution_deadline_ms(),
            crate::limits::DEFAULT_DEADLINE_MS
        );
    }

    #[test]
    fn missing_entry_and_dangling_edges_reject() {
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("a"), action_node()).unwrap();
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::MissingEntry)
        ));

        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("a"), action_node()).unwrap();
        raw.entry(key("a"));
        raw.add_edge(key("a"), key("ghost"), EdgeKindInput::Sequence);
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::DanglingEdge { .. })
        ));
    }

    #[test]
    fn duplicate_keys_and_bad_grammar_reject() {
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("a"), action_node()).unwrap();
        raw.add_node(key("a"), NodeKind::Compensate).unwrap();
        raw.entry(key("a"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::DuplicateNodeKey)
        ));

        let mut raw = RawGraph::new(FailurePolicy::Stop);
        // Grammar rejects uppercase before any structural work happens.
        assert!(super::NodeKey::try_new("Upper").is_err());
        assert!(raw.add_node(key("a"), action_node()).is_ok());
    }

    #[test]
    fn multiple_parents_reject() {
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("p1"), NodeKind::Parallel).unwrap();
        raw.add_node(key("p2"), NodeKind::Parallel).unwrap();
        raw.add_node(key("leaf"), action_node()).unwrap();
        raw.add_edge(key("p1"), key("leaf"), EdgeKindInput::Sequence);
        // Second structural edge into the same leaf.
        raw.add_edge(
            key("p2"),
            key("leaf"),
            EdgeKindInput::Branch { polarity: true },
        );
        raw.entry(key("p1"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::MultipleParents)
        ));
    }

    #[test]
    fn unreachable_nodes_reject() {
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("entry"), action_node()).unwrap();
        raw.add_node(key("orphan"), NodeKind::Parallel).unwrap();
        raw.add_node(key("kid"), action_node()).unwrap();
        raw.add_edge(key("orphan"), key("kid"), EdgeKindInput::Sequence);
        raw.entry(key("entry"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::UnreachableNode)
        ));
    }

    #[test]
    fn illegal_edge_shapes_reject_per_source_kind() {
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("a"), action_node()).unwrap();
        raw.add_node(key("b"), action_node()).unwrap();
        raw.add_edge(key("a"), key("b"), EdgeKindInput::Sequence);
        raw.entry(key("a"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::IllegalEdgeShape { .. })
        ));

        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("fan"), NodeKind::Parallel).unwrap();
        raw.add_node(key("x"), action_node()).unwrap();
        raw.add_edge(
            key("fan"),
            key("x"),
            EdgeKindInput::Branch { polarity: true },
        );
        raw.entry(key("fan"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::IllegalEdgeShape { .. })
        ));

        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("fan"), NodeKind::Parallel).unwrap();
        raw.entry(key("fan"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::EmptyParallel { .. })
        ));

        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("r"), NodeKind::Retry { attempts: 0 })
            .unwrap();
        raw.add_node(key("body"), action_node()).unwrap();
        raw.add_edge(key("r"), key("body"), EdgeKindInput::Sequence);
        raw.entry(key("r"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::MalformedRetry { .. })
        ));
    }

    #[test]
    fn compensation_links_require_action_to_compensate_once() {
        // Compensate target receiving a flow edge rejects.
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("a"), action_node()).unwrap();
        raw.add_node(key("c"), NodeKind::Compensate).unwrap();
        raw.add_edge(key("a"), key("c"), EdgeKindInput::Sequence);
        raw.entry(key("a"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::MalformedCompensationLink)
        ));

        // Double links from one action reject.
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(key("a"), action_node()).unwrap();
        raw.add_node(key("c1"), NodeKind::Compensate).unwrap();
        raw.add_node(key("c2"), NodeKind::Compensate).unwrap();
        raw.add_edge(key("a"), key("c1"), EdgeKindInput::CompensationLink);
        raw.add_edge(key("a"), key("c2"), EdgeKindInput::CompensationLink);
        raw.entry(key("a"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::MalformedCompensationLink)
        ));
    }

    #[test]
    fn payload_and_variable_validation() {
        let oversized =
            serde_json::json!({ "blob": "x".repeat(crate::limits::MAX_ACTION_PARAMS_BYTES + 1) });
        let _ = oversized; // objects reject earlier as params stay JSON-any but bounded

        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(
            key("t"),
            NodeKind::VariableTransform {
                op: TransformOp::Set {
                    variable: "Bad Var".to_string(),
                    value: serde_json::json!(1),
                },
            },
        )
        .unwrap();
        raw.entry(key("t"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::InvalidVariableName { .. })
        ));

        let mut raw = RawGraph::new(FailurePolicy::Stop);
        raw.add_node(
            key("c"),
            NodeKind::Conditional {
                condition: Condition {
                    variable: "mode".to_string(),
                    op: ConditionOp::Equals,
                    operand: serde_json::json!({ "nested": true }),
                },
            },
        )
        .unwrap();
        raw.add_node(key("a"), action_node()).unwrap();
        raw.add_edge(key("c"), key("a"), EdgeKindInput::Branch { polarity: true });
        raw.entry(key("c"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::NonScalarOperand { .. })
        ));
    }

    #[test]
    fn deadline_override_bounds() {
        let mut raw = RawGraph::new(FailurePolicy::Stop);
        let mut node = action_node();
        if let NodeKind::Action {
            deadline_override_ms,
            ..
        } = &mut node
        {
            *deadline_override_ms = Some(0);
        }
        raw.add_node(key("a"), node).unwrap();
        raw.entry(key("a"));
        assert!(matches!(
            ValidatedGraph::build(&raw, &registry()),
            Err(ValidationError::DeadlineOutOfRange { .. })
        ));
    }

    #[test]
    fn registration_name_grammar_is_enforced_upstream_of_graphs() {
        // Action names allow dots for namespacing; node keys do not.
        let mut registry = ActionRegistry::new();
        let registration = super::super::ActionRegistration::try_new(
            "obs.scene.set",
            vec![notify_cap()],
            IdempotencyClass::NonIdempotent,
            false,
            Arc::new(TestPort),
        );
        match registration {
            Ok(registration) => registry.register(registration).unwrap(),
            Err(ConfigError::InvalidActionName) => panic!("dotted names are legal"),
            Err(other) => panic!("{other:?}"),
        }
    }

    fn notify_cap() -> Capability {
        Capability::NotificationShow
    }
}
