//! Deterministic action runtime: journal-first admission, bounded
//! execution, grants before dispatch, typed terminals.
//!
//! Pipeline per admitted command (`OSCP_MESSAGES.md` §6 adapted to the
//! engine boundary):
//!
//! 1. Dedupe-window pruning (oldest-first; `outcome_unknown` exempt).
//! 2. Envelope expiry against the injected **wall** clock — expired
//!    commands journal `expired` and are never queued (`ADR-0005` item 3).
//! 3. Admission dedupe over `(source_device_id, message_id)` — duplicates
//!    cite the original execution and never re-run.
//! 4. Durable `accepted` record (**before** anything else can happen).
//! 5. Deterministic scheduling of the immutable validated graph:
//!    containers, delays, conditionals, transforms, bounded concurrency
//!    ([`MAX_CONCURRENCY_PER_ACTION`] / [`MAX_CONCURRENCY_GLOBAL`]), retry
//!    with deterministic backoff, failure policies
//!    `stop`/`continue`/`compensate`, cancellation propagation, monotonic
//!    deadlines (default 30 s, macro cap 10 min).
//! 6. Per effect, in order: grant intersection (`CAPABILITY_DENIED` on
//!    denial) → durable `prepared` record → adapter dispatch → durable
//!    resolution. A refused journal write aborts the execution closed
//!    **before** dispatch; unresolved prepared records surface
//!    `outcome_unknown` via [`ActionRuntime::recover_outcome_unknown`].
//!
//! Every timing decision derives from the injected [`Clock`] (crate::clock);
//! behavior is reproducible under [`FakeClock`](crate::clock::FakeClock)
//! given identical inputs.

use crate::clock::Clock;
use crate::domain_ids::ExecutionId;
use crate::error::{
    AdmissionRejection, ConfigError, EngineError, JournalError, ReplayRejection, VariableMap,
};
use crate::failure::{FailurePolicy, FailureReason};
use crate::graph::{NodeIdx, NodeKind, ValidatedGraph};
use crate::identifiers::SourceDeviceId;
use crate::journal::{
    AdmissionEntry, DedupeKey, ExecutionJournal, JournalLifecycle, MemoryJournal, PreparedEntry,
};
use crate::limits::{
    DEDUPE_DEFAULT_RETENTION_MS, DEDUPE_MAX_RETENTION_MS, DEDUPE_MIN_RETENTION_MS,
    MAX_CONCURRENCY_GLOBAL, MAX_CONCURRENCY_PER_ACTION, MAX_VARIABLE_VALUE_BYTES, MAX_VARIABLES,
    RETRY_BACKOFF_BASE_MS,
};
use crate::port::{DispatchUnavailable, EffectOutcome, EffectRequest, EffectResponse};
use crate::registry::{ActionRegistration, ActionRegistry, IdempotencyClass};
use crate::{MessageId, graph::NodeKey};
use openstream_domain::capability::Capability;
use openstream_domain::grant::{CapabilityRequest, Decision, GrantLedger};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

/// Runtime configuration validated at construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// Dedupe retention window in wall-clock milliseconds; bounded by
    /// `[DEDUPE_MIN_RETENTION_MS, DEDUPE_MAX_RETENTION_MS]` per `ADR-0005`
    /// decision item 3.
    pub dedupe_retention_ms: i64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            dedupe_retention_ms: DEDUPE_DEFAULT_RETENTION_MS,
        }
    }
}

/// Cooperative cancellation signal attached to an execution; flipping the
/// flag requests propagation to all pending work at the next safe point.
#[derive(Debug, Default, Clone)]
pub struct CancelSignal(Arc<std::sync::atomic::AtomicBool>);

impl CancelSignal {
    /// Fresh unset signal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// One command admitted for execution.
#[derive(Debug)]
pub struct ExecuteRequest {
    /// Trusted source identity (dedupe key half).
    pub source_device_id: SourceDeviceId,
    /// Globally unique envelope message id (dedupe key half).
    pub message_id: MessageId,
    /// Subject whose grant intersection authorizes each effect.
    pub subject: openstream_domain::grant::SubjectRef,
    /// Immutable validated graph to run.
    pub graph: Arc<ValidatedGraph>,
    /// Initial variable map (bounded).
    pub variables: VariableMap,
    /// Command expiry horizon in wall-clock epoch millis.
    pub expires_at_wall_ms: i64,
    /// Optional cooperative cancellation signal.
    pub cancel: Option<CancelSignal>,
}

/// Terminal states reported on receipts: exactly the authoritative terminal
/// subset of `PROTOCOL.md`; `accepted`/`running` are journaled internally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalState {
    /// All scheduled work completed without failure.
    Succeeded,
    /// Typed failure surfaced with its reason.
    Failed {
        /// Why the execution failed.
        reason: FailureReason,
    },
    /// Cancellation propagated before completion.
    Cancelled,
    /// Monotonic deadline elapsed before completion.
    Expired,
    /// Crash-window gap around an effect; honest unknown superseded only by
    /// explicit reconciliation/replay.
    OutcomeUnknown,
}

impl TerminalState {
    fn to_lifecycle(&self) -> JournalLifecycle {
        match self {
            Self::Succeeded => JournalLifecycle::Succeeded,
            Self::Failed { reason } => JournalLifecycle::Failed {
                token: reason.token().to_string(),
            },
            Self::Cancelled => JournalLifecycle::Cancelled,
            Self::Expired => JournalLifecycle::Expired,
            Self::OutcomeUnknown => JournalLifecycle::OutcomeUnknown,
        }
    }

    /// Canonical lowercase state token.
    #[must_use]
    pub fn token(&self) -> &str {
        self.to_lifecycle().to_execution_state().as_str()
    }
}

/// Evidence row for one dispatched effect attempt (redaction-safe:
/// identifiers, tokens, adapter codes only — never payloads or secrets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRecord {
    /// Graph node that performed the effect.
    pub node_key: NodeKey,
    /// Registered action type name.
    pub action_type: String,
    /// Attempt counter within the owning retry scope.
    pub attempt: u32,
    /// Whether this invocation was compensation unwinding.
    pub is_compensation: bool,
    /// Outcome token (`succeeded` / `failed` / `unknown`).
    pub outcome: String,
    /// Adapter failure code when applicable.
    pub failure_code: Option<String>,
    /// Monotonic milliseconds when the outcome was observed.
    pub observed_at_monotonic_ms: u64,
}

/// Final result of one executed command.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReceipt {
    /// Execution id assigned at admission (replays reuse it).
    pub execution_id: ExecutionId,
    /// Terminal state.
    pub state: TerminalState,
    /// Final variable map.
    pub variables: VariableMap,
    /// Ordered effect evidence (completion order).
    pub effects: Vec<EffectRecord>,
}

/// Builder for [`ActionRuntime`].
#[derive(Default)]
pub struct RuntimeBuilder {
    clock: Option<Arc<dyn Clock>>,
    time_control: Option<Arc<dyn TimeControl>>,
    journal: Option<Box<dyn ExecutionJournal>>,
    registry: ActionRegistry,
    ledger: Option<Arc<Mutex<GrantLedger>>>,
    config: RuntimeConfig,
}

impl RuntimeBuilder {
    /// Starts a builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Injects the time source (required). Real monotonic sources wire in
    /// at the composition root (#16); deterministic hosts use a fake.
    #[must_use]
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Injects the durability boundary (defaults to [`MemoryJournal`]).
    #[must_use]
    pub fn journal(mut self, journal: Box<dyn ExecutionJournal>) -> Self {
        self.journal = Some(journal);
        self
    }

    /// Sets the action registry (registrations happen before build).
    #[must_use]
    pub fn registry(mut self, registry: ActionRegistry) -> Self {
        self.registry = registry;
        self
    }

    /// Shares the deny-by-default grant ledger so external revocation
    /// applies at the very next evaluation.
    #[must_use]
    pub fn grant_ledger(mut self, ledger: Arc<Mutex<GrantLedger>>) -> Self {
        self.ledger = Some(ledger);
        self
    }

    /// Wires controlled-time advancement so the scheduler can move to its
    /// next scheduled event (`FakeClock` implements [`TimeControl`]).
    /// Without it, steps report stalls once work awaits virtual time.
    #[must_use]
    pub fn time_control(mut self, control: Arc<dyn TimeControl>) -> Self {
        self.time_control = Some(control);
        self
    }

    /// Overrides the dedupe retention window (bounds-checked at build).
    #[must_use]
    pub fn dedupe_retention_ms(mut self, retention_ms: i64) -> Self {
        self.config.dedupe_retention_ms = retention_ms;
        self
    }

    /// Validates configuration and constructs the runtime.
    ///
    /// # Errors
    /// [`ConfigError::MissingClock`] without a time source;
    /// [`ConfigError::RetentionOutOfBounds`] outside the ADR-fixed bounds.
    pub fn build(self) -> Result<ActionRuntime, EngineError> {
        let Some(clock) = self.clock else {
            return Err(ConfigError::MissingClock.into());
        };
        let config = self.config;
        if !(DEDUPE_MIN_RETENTION_MS..=DEDUPE_MAX_RETENTION_MS)
            .contains(&config.dedupe_retention_ms)
        {
            return Err(ConfigError::RetentionOutOfBounds {
                requested_ms: config.dedupe_retention_ms,
                min_ms: DEDUPE_MIN_RETENTION_MS,
                max_ms: DEDUPE_MAX_RETENTION_MS,
            }
            .into());
        }
        Ok(ActionRuntime {
            clock,
            time_control: self.time_control,
            journal: self
                .journal
                .unwrap_or_else(|| Box::new(MemoryJournal::new())),
            registry: self.registry,
            ledger: self
                .ledger
                .unwrap_or_else(|| Arc::new(Mutex::new(GrantLedger::new()))),
            config,
            active: None,
        })
    }
}

/// Deterministic engine core. One resumable execution is active at a time;
/// concurrent executions compose at the host layer (#16+).
pub struct ActionRuntime {
    clock: Arc<dyn Clock>,
    time_control: Option<Arc<dyn TimeControl>>,
    journal: Box<dyn ExecutionJournal>,
    registry: ActionRegistry,
    ledger: Arc<Mutex<GrantLedger>>,
    config: RuntimeConfig,
    active: Option<ActiveRun>,
}

impl ActionRuntime {
    /// Executes a command synchronously to its terminal state.
    ///
    /// # Errors
    /// [`AdmissionRejection`] for expiry, duplicates, refused evidence
    /// writes, or variable-bound violations. Nothing dispatches on any
    /// rejection path.
    pub fn execute(
        &mut self,
        request: ExecuteRequest,
    ) -> Result<ExecutionReceipt, AdmissionRejection> {
        let mut handle = self.begin(request)?;
        handle.run_to_completion()
    }

    /// Admits a command and parks it for stepwise driving; the durable
    /// `accepted` record exists before this call returns.
    ///
    /// # Errors
    /// [`AdmissionRejection`] variants; nothing has been dispatched.
    pub fn begin(
        &mut self,
        request: ExecuteRequest,
    ) -> Result<ExecutionHandle<'_>, AdmissionRejection> {
        if self.active.is_some() {
            return Err(AdmissionRejection::RuntimeBusy);
        }
        Self::validate_variables(&request.variables)?;

        self.journal
            .prune(self.clock.wall_now_ms(), self.config.dedupe_retention_ms);
        let execution_id = ExecutionId::generate();
        let dedupe = DedupeKey::new(request.source_device_id.clone(), request.message_id);

        // Expiry first (pipeline order): expired commands journal `expired`
        // and are never queued for later execution.
        if request.expires_at_wall_ms <= self.clock.wall_now_ms() {
            self.journal
                .admit(AdmissionEntry {
                    key: dedupe,
                    execution_id,
                    accepted_at_wall_ms: self.clock.wall_now_ms(),
                    expires_at_wall_ms: request.expires_at_wall_ms,
                    lifecycle: JournalLifecycle::Expired,
                })
                .map_err(|source| AdmissionRejection::JournalRefused { source })?;
            return Err(AdmissionRejection::Expired { execution_id });
        }

        // Dedupe: duplicates cite the original; effects never re-run.
        if let Some(existing) = self.journal.lookup(&dedupe) {
            return Err(AdmissionRejection::DuplicateSuppressed {
                original_execution_id: existing.execution_id,
                original_state: existing.lifecycle,
            });
        }

        // Journal-first admission: durable `accepted` BEFORE any effect.
        self.journal
            .admit(AdmissionEntry {
                key: dedupe.clone(),
                execution_id,
                accepted_at_wall_ms: self.clock.wall_now_ms(),
                expires_at_wall_ms: request.expires_at_wall_ms,
                lifecycle: JournalLifecycle::Accepted,
            })
            .map_err(|source| AdmissionRejection::JournalRefused { source })?;

        let now = self.clock.monotonic_ms();
        let deadline_mono = now.saturating_add(request.graph.execution_deadline_ms());
        self.active = Some(ActiveRun {
            run: ExecutionRun::new(
                execution_id,
                Arc::clone(&request.graph),
                request.subject,
                dedupe,
                request.variables,
                deadline_mono,
                request.cancel,
            ),
        });
        Ok(ExecutionHandle { runtime: self })
    }

    fn validate_variables(vars: &VariableMap) -> Result<(), AdmissionRejection> {
        if vars.len() > MAX_VARIABLES {
            return Err(AdmissionRejection::VariablesRejected);
        }
        for value in vars.values() {
            let size = serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len());
            if size > MAX_VARIABLE_VALUE_BYTES {
                return Err(AdmissionRejection::VariablesRejected);
            }
        }
        Ok(())
    }

    /// Scans the journal for prepared-without-terminal records (the crash
    /// window) and closes each with corrective `outcome_unknown` evidence.
    /// Never infers success; never schedules retries.
    ///
    /// # Errors
    /// Journal refusals surface fail-closed; scanning stops.
    pub fn recover_outcome_unknown(&mut self) -> Result<Vec<ExecutionId>, JournalError> {
        let orphans = self.journal.unresolved_prepared();
        let mut touched: BTreeSet<ExecutionId> = BTreeSet::new();
        for orphan in orphans {
            self.journal
                .resolve_prepared(orphan.execution_id, &orphan.node_key, orphan.attempt)?;
            touched.insert(orphan.execution_id);
        }
        for execution_id in &touched {
            self.journal
                .set_lifecycle(*execution_id, JournalLifecycle::OutcomeUnknown)?;
        }
        Ok(touched.into_iter().collect())
    }

    /// Replays an execution stuck in `outcome_unknown`, permitted only when
    /// every action of the replacement graph declares idempotency so the
    /// derived adapter-facing keys stay stable (`OSCP_MESSAGES.md` §7).
    /// The corrective terminal record links the SAME execution id.
    ///
    /// # Errors
    /// [`ReplayRejection`] variants; nothing dispatches on refusal.
    pub fn replay(
        &mut self,
        source_device_id: SourceDeviceId,
        message_id: MessageId,
        subject: openstream_domain::grant::SubjectRef,
        graph: Arc<ValidatedGraph>,
        variables: VariableMap,
        expires_at_wall_ms: i64,
    ) -> Result<ExecutionReceipt, ReplayRejection> {
        Self::validate_variables(&variables).map_err(|_| ReplayRejection::InvalidVariables)?;
        self.journal
            .prune(self.clock.wall_now_ms(), self.config.dedupe_retention_ms);
        let dedupe = DedupeKey::new(source_device_id, message_id);
        let Some(existing) = self.journal.lookup(&dedupe) else {
            return Err(ReplayRejection::UnknownKey);
        };
        if existing.lifecycle != JournalLifecycle::OutcomeUnknown {
            return Err(ReplayRejection::NotOutcomeUnknown);
        }
        if expires_at_wall_ms <= self.clock.wall_now_ms() {
            return Err(ReplayRejection::Expired);
        }
        for (_, _, kind) in graph.iter_nodes() {
            if let NodeKind::Action { action_type, .. } = kind {
                let declared = self
                    .registry
                    .lookup(action_type)
                    .is_some_and(|registration| {
                        registration.idempotency() == IdempotencyClass::Idempotent
                    });
                if !declared {
                    return Err(ReplayRejection::RequiresIdempotentAdapters);
                }
            }
        }
        self.journal
            .set_lifecycle(existing.execution_id, JournalLifecycle::Running)
            .map_err(|source| ReplayRejection::JournalRefused { source })?;

        let now = self.clock.monotonic_ms();
        let deadline_mono = now.saturating_add(graph.execution_deadline_ms());
        let mut run = ExecutionRun::new(
            existing.execution_id,
            graph,
            subject,
            dedupe,
            variables,
            deadline_mono,
            None,
        );
        // The corrective path resumes from `running` by construction.
        run.running_marked = true;
        self.active = Some(ActiveRun { run });
        let mut handle = ExecutionHandle { runtime: self };
        match handle.run_to_completion() {
            Ok(receipt) => Ok(receipt),
            Err(AdmissionRejection::JournalRefused { source }) => {
                Err(ReplayRejection::JournalRefused { source })
            }
            Err(_) => Err(ReplayRejection::JournalRefused {
                source: JournalError::Refused,
            }),
        }
    }
}

/// Stepwise driver over one active execution. Deterministic tests advance
/// the fake clock between steps or call [`Self::run_to_completion`].
pub struct ExecutionHandle<'r> {
    runtime: &'r mut ActionRuntime,
}

impl<'r> ExecutionHandle<'r> {
    /// The execution id assigned at admission (while the run is parked).
    #[must_use]
    pub fn execution_id(&self) -> Option<ExecutionId> {
        self.runtime
            .active
            .as_ref()
            .map(|active| active.run.execution_id)
    }

    /// Whether the run reached a terminal decision.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.runtime
            .active
            .as_ref()
            .is_some_and(|active| active.run.terminal.is_some())
    }

    /// Requests cooperative cancellation; propagation happens at the next
    /// safe point, including across parallel branches.
    pub fn cancel(&mut self) {
        let Some(active) = self.runtime.active.as_mut() else {
            return;
        };
        if let Some(signal) = active.run.cancel.as_ref() {
            signal.cancel();
        }
        active.run.cancel_requested = true;
    }

    /// Advances the scheduler one pass. Returns whether any work was
    /// performed (including controlled time advancement).
    pub fn step(&mut self) -> bool {
        if !self.runtime.has_active() {
            return false;
        }
        self.runtime.drive_once()
    }

    /// Drives the scheduler to the terminal decision, persists terminal
    /// evidence, and returns the receipt.
    ///
    /// # Errors
    /// [`AdmissionRejection::JournalRefused`] when durable evidence could
    /// not be written (the recovery scan then reports `outcome_unknown`);
    /// [`AdmissionRejection::RuntimeBusy`] when time cannot be advanced on
    /// this runtime (no [`TimeControl`] wired) and work remains.
    pub fn run_to_completion(&mut self) -> Result<ExecutionReceipt, AdmissionRejection> {
        while !self.is_terminal() {
            if !self.step() {
                return Err(AdmissionRejection::RuntimeBusy);
            }
        }
        let Some(mut active) = self.runtime.active.take() else {
            return Err(AdmissionRejection::RuntimeBusy);
        };
        if let Some(fault) = active.run.journal_fault.take() {
            return Err(AdmissionRejection::JournalRefused { source: fault });
        }
        let terminal = active
            .run
            .terminal
            .clone()
            .unwrap_or(TerminalState::Failed {
                reason: FailureReason::InternalError,
            });
        self.runtime
            .journal
            .set_lifecycle(active.run.execution_id, terminal.to_lifecycle())
            .map_err(|source| AdmissionRejection::JournalRefused { source })?;
        Ok(ExecutionReceipt {
            execution_id: active.run.execution_id,
            state: terminal,
            variables: active.run.vars.clone(),
            effects: active.run.evidence.clone(),
        })
    }
}

/// Controlled-time surface for deterministic hosts: lets the scheduler move
/// the injected clock forward to the next scheduled event. Real monotonic
/// sources wire their own timer-driven advancement at #16 and simply omit
/// this capability (steps then report stalls instead of inventing time).
pub trait TimeControl: Send + Sync {
    /// Advances the clock by `delta_ms`.
    fn advance_ms(&self, delta_ms: u64);
}

impl TimeControl for crate::clock::FakeClock {
    fn advance_ms(&self, delta_ms: u64) {
        crate::clock::FakeClock::advance(self, delta_ms);
    }
}

struct DriverCtx<'a> {
    clock: &'a dyn Clock,
    time_control: Option<&'a dyn TimeControl>,
    journal: &'a mut dyn ExecutionJournal,
    registry: &'a ActionRegistry,
    ledger: &'a Mutex<GrantLedger>,
}

struct ActiveRun {
    run: ExecutionRun,
}

struct ExecutionRun {
    execution_id: ExecutionId,
    graph: Arc<ValidatedGraph>,
    subject: openstream_domain::grant::SubjectRef,
    dedupe: DedupeKey,
    vars: VariableMap,
    deadline_mono: u64,
    cancel: Option<CancelSignal>,
    cancel_requested: bool,
    branches: Vec<Branch>,
    inflight: Vec<InFlight>,
    slot_waiters: VecDeque<SlotWaiter>,
    used_global: usize,
    used_per_action: BTreeMap<String, usize>,
    evidence: Vec<EffectRecord>,
    compensation_targets: Vec<CompensationTarget>,
    terminal: Option<TerminalState>,
    first_failure: Option<FailureReason>,
    compensating: bool,
    running_marked: bool,
    journal_fault: Option<JournalError>,
}

struct SlotWaiter {
    branch: usize,
    node: NodeIdx,
    action_type: String,
    attempt: u32,
    idempotency_key: String,
    node_deadline_abs: Option<u64>,
}

struct CompensationTarget {
    node: NodeIdx,
    compensation_node: NodeIdx,
    action_type: String,
    capability: Capability,
    params: serde_json::Value,
    idempotency_key: String,
}

impl Clone for CompensationTarget {
    fn clone(&self) -> Self {
        Self {
            node: self.node,
            compensation_node: self.compensation_node,
            action_type: self.action_type.clone(),
            capability: self.capability.clone(),
            params: self.params.clone(),
            idempotency_key: self.idempotency_key.clone(),
        }
    }
}

struct Branch {
    stack: Vec<Frame>,
    state: BranchState,
    join: Option<JoinContext>,
}

#[derive(Clone)]
enum Frame {
    Root,
    Seq {
        container: NodeIdx,
        cursor: usize,
    },
    ParJoin {
        container: NodeIdx,
        remaining: usize,
    },
    Retry {
        node: NodeIdx,
        attempts_total: u32,
        attempts_done: u32,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WakeNext {
    Complete(NodeIdx),
    Enter(NodeIdx),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BranchState {
    Ready(NodeIdx),
    Sleeping { until_mono: u64, next: WakeNext },
    WaitingSlot { node: NodeIdx },
    WaitingEffect,
    WaitingJoin,
    Exited,
}

#[derive(Clone, Copy)]
struct JoinContext {
    parent_branch: usize,
    parallel_node: NodeIdx,
}

struct InFlight {
    branch: usize,
    node: NodeIdx,
    action_type: String,
    attempt: u32,
    is_compensation: bool,
    wake_at_mono: u64,
    outcome: EffectOutcome,
    node_deadline_abs: Option<u64>,
}

impl ExecutionRun {
    fn new(
        execution_id: ExecutionId,
        graph: Arc<ValidatedGraph>,
        subject: openstream_domain::grant::SubjectRef,
        dedupe: DedupeKey,
        vars: VariableMap,
        deadline_mono: u64,
        cancel: Option<CancelSignal>,
    ) -> Self {
        let entry = graph.entry();
        Self {
            execution_id,
            graph,
            subject,
            dedupe,
            vars,
            deadline_mono,
            cancel,
            cancel_requested: false,
            branches: vec![Branch {
                stack: vec![Frame::Root],
                state: BranchState::Ready(entry),
                join: None,
            }],
            inflight: Vec::new(),
            slot_waiters: VecDeque::new(),
            used_global: 0,
            used_per_action: BTreeMap::new(),
            evidence: Vec::new(),
            compensation_targets: Vec::new(),
            terminal: None,
            first_failure: None,
            compensating: false,
            running_marked: false,
            journal_fault: None,
        }
    }

    fn now(&self, ctx: &DriverCtx<'_>) -> u64 {
        ctx.clock.monotonic_ms()
    }

    fn node_key(&self, node: NodeIdx) -> NodeKey {
        self.graph.key(node).clone()
    }

    fn action_spec(
        &self,
        node: NodeIdx,
    ) -> Option<(String, Capability, serde_json::Value, Option<u64>)> {
        match self.graph.kind(node) {
            NodeKind::Action {
                action_type,
                capability,
                params,
                deadline_override_ms,
            } => Some((
                action_type.clone(),
                capability.clone(),
                params.clone(),
                *deadline_override_ms,
            )),
            _ => None,
        }
    }

    fn idempotency_key(&self, node_key: &NodeKey, attempt: u32, compensation: bool) -> String {
        // Deterministic derivation from (source_device_id, message_id) per
        // OSCP_MESSAGES.md §7, disambiguated per node/attempt so parallel
        // effects inside one command collapse correctly adapter-side.
        let mut key = format!(
            "{}:{}:{}:{}",
            self.dedupe.source_device_id, self.dedupe.message_id, node_key, attempt
        );
        if compensation {
            key.push_str(":compensation");
        }
        key
    }

    fn fail_closed_journal(&mut self, source: JournalError) {
        if self.terminal.is_none() {
            self.terminal = Some(TerminalState::Failed {
                reason: FailureReason::JournalWriteRefused,
            });
        }
        if self.journal_fault.is_none() {
            self.journal_fault = Some(source);
        }
    }
}

impl ActionRuntime {
    fn has_active(&self) -> bool {
        self.active.is_some()
    }

    fn drive_once(&mut self) -> bool {
        let time_control = self.time_control.as_deref();
        let mut ctx = DriverCtx {
            clock: self.clock.as_ref(),
            time_control,
            journal: self.journal.as_mut(),
            registry: &self.registry,
            ledger: &self.ledger,
        };
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        scheduler_pass(&mut ctx, &mut active.run)
    }
}

fn scheduler_pass(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun) -> bool {
    if run.terminal.is_some() {
        return false;
    }
    sweep_flags(ctx, run);
    if run.terminal.is_some() {
        return true;
    }

    let mut progressed = wake_due(ctx, run);

    while run.terminal.is_none() {
        let Some(branch_idx) = run
            .branches
            .iter()
            .position(|branch| matches!(branch.state, BranchState::Ready(_)))
        else {
            break;
        };
        enter_current(ctx, run, branch_idx);
        progressed = true;
        if run.compensating {
            break;
        }
    }
    if run.terminal.is_some() {
        return true;
    }

    if quiescent(run) {
        finalize(ctx, run);
        return true;
    }

    if !progressed {
        match next_wake_mono(run) {
            Some(target) => {
                let now = ctx.clock.monotonic_ms();
                if let Some(control) = ctx.time_control {
                    if target > now {
                        control.advance_ms(target - now);
                    }
                    progressed = true;
                }
            }
            None => {
                // Live work exists but nothing can ever wake: invariant
                // violation surfaced conservatively, never as success.
                run.fail_closed_journal(JournalError::Refused);
                run.terminal = Some(TerminalState::Failed {
                    reason: FailureReason::InternalError,
                });
                progressed = true;
            }
        }
    }
    progressed
}

fn quiescent(run: &ExecutionRun) -> bool {
    run.inflight.is_empty()
        && run.slot_waiters.is_empty()
        && run
            .branches
            .iter()
            .all(|branch| matches!(branch.state, BranchState::Exited))
}

fn next_wake_mono(run: &ExecutionRun) -> Option<u64> {
    // The execution deadline always bounds waiting: the scheduler wakes at
    // the deadline exactly, never past it.
    let mut earliest = Some(run.deadline_mono);
    for branch in &run.branches {
        if let BranchState::Sleeping { until_mono, .. } = branch.state {
            earliest = Some(earliest.map_or(until_mono, |current| current.min(until_mono)));
        }
    }
    for effect in &run.inflight {
        earliest = Some(earliest.map_or(effect.wake_at_mono, |current| {
            current.min(effect.wake_at_mono)
        }));
    }
    earliest
}

fn sweep_flags(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun) {
    let now = ctx.clock.monotonic_ms();
    let cancelled = run.cancel_requested
        || run
            .cancel
            .as_ref()
            .is_some_and(|signal| signal.is_cancelled());
    let expired = now >= run.deadline_mono;
    if !cancelled && !expired {
        return;
    }
    let terminal = if cancelled {
        TerminalState::Cancelled
    } else {
        TerminalState::Expired
    };
    abort_all(ctx, run, terminal);
}

fn abort_all(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun, terminal: TerminalState) {
    // In-flight effects settle honestly: their outcomes were possibly
    // applied externally and are recorded as evidence even though the
    // execution terminates cancelled/expired (no success inference).
    let drained: Vec<InFlight> = std::mem::take(&mut run.inflight);
    for effect in drained {
        let now = ctx.clock.monotonic_ms();
        let _ = ctx.journal.resolve_prepared(
            run.execution_id,
            &run.node_key(effect.node),
            effect.attempt,
        );
        run.evidence.push(EffectRecord {
            node_key: run.node_key(effect.node),
            action_type: effect.action_type.clone(),
            attempt: effect.attempt,
            is_compensation: effect.is_compensation,
            outcome: effect.outcome.token().to_string(),
            failure_code: effect.outcome.failure_code().map(str::to_string),
            observed_at_monotonic_ms: now,
        });
        release_slots(run, &effect.action_type);
    }
    // Slot waiters never dispatched: close their prepared records.
    for waiter in std::mem::take(&mut run.slot_waiters) {
        let _ = ctx.journal.resolve_prepared(
            run.execution_id,
            &run.node_key(waiter.node),
            waiter.attempt,
        );
    }
    for branch in &mut run.branches {
        branch.state = BranchState::Exited;
    }
    run.terminal = Some(terminal);
}

fn release_slots(run: &mut ExecutionRun, action_type: &str) {
    run.used_global = run.used_global.saturating_sub(1);
    if let Some(count) = run.used_per_action.get_mut(action_type) {
        *count = count.saturating_sub(1);
    }
}

fn wake_due(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun) -> bool {
    let mut acted = false;
    let now = ctx.clock.monotonic_ms();

    // Due delayed effects, oldest-scheduled first (insertion order).
    let mut index = 0;
    while index < run.inflight.len() {
        if run.inflight[index].wake_at_mono <= now {
            let effect = run.inflight.remove(index);
            acted = true;
            deliver_effect(ctx, run, effect, now);
            index = 0; // indices shift; rescan deterministically
        } else {
            index += 1;
        }
    }
    if run.terminal.is_some() {
        return acted;
    }

    // Due sleepers (delays / retry backoff).
    for branch_idx in 0..run.branches.len() {
        let due = matches!(
            &run.branches[branch_idx].state,
            BranchState::Sleeping { until_mono, .. } if *until_mono <= now
        );
        if !due {
            continue;
        }
        let BranchState::Sleeping { next, .. } =
            std::mem::replace(&mut run.branches[branch_idx].state, BranchState::Exited)
        else {
            unreachable!("checked above");
        };
        acted = true;
        match next {
            WakeNext::Enter(node) => {
                run.branches[branch_idx].state = BranchState::Ready(node);
            }
            WakeNext::Complete(node) => {
                complete_node(run, branch_idx, node);
                if run.branches[branch_idx].state == BranchState::Exited {
                    on_branch_exit(ctx, run, branch_idx);
                }
            }
        }
        if run.terminal.is_some() {
            return acted;
        }
    }

    acted |= dispatch_slot_waiters(ctx, run);
    acted
}

fn deliver_effect(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun, effect: InFlight, now: u64) {
    let action_type = effect.action_type.clone();
    release_slots(run, &action_type);
    if ctx
        .journal
        .resolve_prepared(run.execution_id, &run.node_key(effect.node), effect.attempt)
        .is_err()
    {
        run.fail_closed_journal(JournalError::Refused);
        return;
    }
    run.evidence.push(EffectRecord {
        node_key: run.node_key(effect.node),
        action_type,
        attempt: effect.attempt,
        is_compensation: effect.is_compensation,
        outcome: effect.outcome.token().to_string(),
        failure_code: effect.outcome.failure_code().map(str::to_string),
        observed_at_monotonic_ms: now,
    });
    if run.compensating {
        run.branches[effect.branch].state = BranchState::Exited;
        return;
    }
    // Node-scoped deadline overrides turn late completions into typed
    // deadline failures regardless of the reported outcome.
    if effect
        .node_deadline_abs
        .is_some_and(|deadline| now > deadline)
    {
        node_failed(
            ctx,
            run,
            effect.branch,
            effect.node,
            FailureReason::DeadlineExceeded,
        );
        if run.branches[effect.branch].state == BranchState::Exited {
            on_branch_exit(ctx, run, effect.branch);
        }
        return;
    }
    match effect.outcome {
        EffectOutcome::Succeeded => {
            remember_compensation_target(run, effect.node);
            complete_node(run, effect.branch, effect.node);
            if run.branches[effect.branch].state == BranchState::Exited {
                on_branch_exit(ctx, run, effect.branch);
            }
        }
        EffectOutcome::Failed { code } => {
            node_failed(
                ctx,
                run,
                effect.branch,
                effect.node,
                FailureReason::AdapterFailed { code },
            );
            if run.branches[effect.branch].state == BranchState::Exited {
                on_branch_exit(ctx, run, effect.branch);
            }
        }
        EffectOutcome::Unknown => {
            // Crash-gap honesty: never inferred success, never retried.
            run.terminal = Some(TerminalState::OutcomeUnknown);
        }
    }
}

fn remember_compensation_target(run: &mut ExecutionRun, node: NodeIdx) {
    if run.graph.failure_policy() != FailurePolicy::Compensate {
        return;
    }
    let Some(compensation_node) = run.graph.compensate_of(node) else {
        return;
    };
    if let Some((action_type, capability, params, _)) = run.action_spec(node) {
        let node_key = run.node_key(node);
        run.compensation_targets.push(CompensationTarget {
            node,
            compensation_node,
            action_type,
            capability,
            params,
            idempotency_key: run.idempotency_key(&node_key, 0, true),
        });
    }
}

fn dispatch_slot_waiters(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun) -> bool {
    let mut acted = false;
    while run.terminal.is_none() {
        let now = ctx.clock.monotonic_ms();
        let candidate = run.slot_waiters.iter().position(|waiter| {
            run.used_global < MAX_CONCURRENCY_GLOBAL
                && run
                    .used_per_action
                    .get(&waiter.action_type)
                    .copied()
                    .unwrap_or(0)
                    < MAX_CONCURRENCY_PER_ACTION
        });
        let Some(position) = candidate else {
            break;
        };
        let waiter = run
            .slot_waiters
            .remove(position)
            .unwrap_or_else(|| unreachable!("position validated above"));
        *run.used_per_action
            .entry(waiter.action_type.clone())
            .or_insert(0) += 1;
        run.used_global += 1;
        acted = true;
        dispatch_effect(ctx, run, waiter, now);
    }
    acted
}

fn dispatch_effect(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun, waiter: SlotWaiter, now: u64) {
    let Some((action_type, capability, params, _)) = run.action_spec(waiter.node) else {
        run.fail_closed_journal(JournalError::Refused);
        return;
    };
    debug_assert_eq!(action_type, waiter.action_type);
    let Some(registration) = ctx.registry.lookup(&action_type) else {
        run.fail_closed_journal(JournalError::Refused);
        return;
    };
    let branch = waiter.branch;
    let node_deadline_abs = waiter.node_deadline_abs;
    let attempt = waiter.attempt;
    let node_key = run.node_key(waiter.node);
    let idempotency_key = waiter.idempotency_key.clone();
    let request = EffectRequest {
        execution_id: run.execution_id,
        node_key: node_key.clone(),
        action_type: action_type.clone(),
        capability,
        params,
        idempotency_key,
        attempt,
        is_compensation: false,
    };
    match registration.port().invoke(request) {
        Err(DispatchUnavailable) => {
            release_slots(run, &action_type);
            let _ = ctx
                .journal
                .resolve_prepared(run.execution_id, &node_key, attempt);
            run.evidence.push(EffectRecord {
                node_key,
                action_type: action_type.clone(),
                attempt,
                is_compensation: false,
                outcome: "failed".to_string(),
                failure_code: None,
                observed_at_monotonic_ms: now,
            });
            node_failed(
                ctx,
                run,
                branch,
                waiter.node,
                FailureReason::AdapterUnavailable,
            );
        }
        Ok(EffectResponse::Immediate(outcome)) => {
            release_slots(run, &action_type);
            let _ = ctx
                .journal
                .resolve_prepared(run.execution_id, &node_key, attempt);
            run.evidence.push(EffectRecord {
                node_key,
                action_type: action_type.clone(),
                attempt,
                is_compensation: false,
                outcome: outcome.token().to_string(),
                failure_code: outcome.failure_code().map(str::to_string),
                observed_at_monotonic_ms: now,
            });
            route_outcome(ctx, run, branch, waiter.node, outcome, node_deadline_abs);
        }
        Ok(EffectResponse::Delayed {
            duration_ms,
            outcome,
        }) => {
            run.inflight.push(InFlight {
                branch,
                node: waiter.node,
                action_type,
                attempt,
                is_compensation: false,
                wake_at_mono: now.saturating_add(duration_ms),
                outcome,
                node_deadline_abs,
            });
            run.branches[branch].state = BranchState::WaitingEffect;
        }
    }
    if run.branches[branch].state == BranchState::Exited {
        on_branch_exit(ctx, run, branch);
    }
}

fn route_outcome(
    ctx: &mut DriverCtx<'_>,
    run: &mut ExecutionRun,
    branch: usize,
    node: NodeIdx,
    outcome: EffectOutcome,
    node_deadline_abs: Option<u64>,
) {
    let now = ctx.clock.monotonic_ms();
    if node_deadline_abs.is_some_and(|deadline| now > deadline) {
        node_failed(ctx, run, branch, node, FailureReason::DeadlineExceeded);
        return;
    }
    match outcome {
        EffectOutcome::Succeeded => {
            remember_compensation_target(run, node);
            complete_node(run, branch, node);
        }
        EffectOutcome::Failed { code } => {
            node_failed(
                ctx,
                run,
                branch,
                node,
                FailureReason::AdapterFailed { code },
            );
        }
        EffectOutcome::Unknown => {
            run.terminal = Some(TerminalState::OutcomeUnknown);
        }
    }
}

fn enter_current(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun, branch_idx: usize) {
    let BranchState::Ready(node) = run.branches[branch_idx].state else {
        return;
    };
    match run.graph.kind(node) {
        NodeKind::Action { .. } => enter_action(ctx, run, branch_idx, node),
        NodeKind::Delay { duration_ms } => {
            let until = run.now(ctx).saturating_add(*duration_ms);
            run.branches[branch_idx].state = BranchState::Sleeping {
                until_mono: until,
                next: WakeNext::Complete(node),
            };
        }
        NodeKind::VariableTransform { op } => {
            let op = op.clone();
            if apply_transform(run, &op) {
                complete_node(run, branch_idx, node);
                if run.branches[branch_idx].state == BranchState::Exited {
                    on_branch_exit(ctx, run, branch_idx);
                }
            } else {
                node_failed(ctx, run, branch_idx, node, FailureReason::TransformFailed);
                if run.branches[branch_idx].state == BranchState::Exited {
                    on_branch_exit(ctx, run, branch_idx);
                }
            }
        }
        NodeKind::Conditional { condition } => {
            let truth = eval_condition(condition, &run.vars);
            let target = if truth {
                run.graph.branch_true(node)
            } else {
                run.graph.branch_false(node)
            };
            match target {
                Some(next) => run.branches[branch_idx].state = BranchState::Ready(next),
                None => {
                    complete_node(run, branch_idx, node);
                    if run.branches[branch_idx].state == BranchState::Exited {
                        on_branch_exit(ctx, run, branch_idx);
                    }
                }
            }
        }
        NodeKind::Sequence => {
            let kids = run.graph.children(node);
            let first = kids.first().copied();
            run.branches[branch_idx].stack.push(Frame::Seq {
                container: node,
                cursor: 0,
            });
            match first {
                Some(kid) => run.branches[branch_idx].state = BranchState::Ready(kid),
                None => {
                    // Validated graphs always have >=1 child.
                    run.terminal = Some(TerminalState::Failed {
                        reason: FailureReason::InternalError,
                    });
                }
            }
        }
        NodeKind::Parallel => {
            let kids: Vec<NodeIdx> = run.graph.children(node).to_vec();
            run.branches[branch_idx].stack.push(Frame::ParJoin {
                container: node,
                remaining: kids.len(),
            });
            run.branches[branch_idx].state = BranchState::WaitingJoin;
            for kid in kids {
                run.branches.push(Branch {
                    stack: vec![Frame::Root],
                    state: BranchState::Ready(kid),
                    join: Some(JoinContext {
                        parent_branch: branch_idx,
                        parallel_node: node,
                    }),
                });
            }
        }
        NodeKind::Retry { attempts } => {
            let body = run.graph.children(node)[0];
            run.branches[branch_idx].stack.push(Frame::Retry {
                node,
                attempts_total: *attempts,
                attempts_done: 0,
            });
            run.branches[branch_idx].state = BranchState::Ready(body);
        }
        // Compensate nodes hang off links only; flow edges into them are
        // rejected at validation, so reaching here is an invariant breach.
        NodeKind::Compensate => {
            run.terminal = Some(TerminalState::Failed {
                reason: FailureReason::InternalError,
            });
        }
    }
}

fn enter_action(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun, branch_idx: usize, node: NodeIdx) {
    let Some((action_type, capability, _params, override_ms)) = run.action_spec(node) else {
        run.terminal = Some(TerminalState::Failed {
            reason: FailureReason::InternalError,
        });
        return;
    };
    let Some(registration) = ctx.registry.lookup(&action_type) else {
        run.terminal = Some(TerminalState::Failed {
            reason: FailureReason::InternalError,
        });
        return;
    };

    // S5: grant intersection immediately before this side effect.
    if let Decision::Denied { reason } = ctx.authorize(&run.subject, &capability, registration) {
        node_failed(
            ctx,
            run,
            branch_idx,
            node,
            FailureReason::CapabilityDenied(reason),
        );
        if run.branches[branch_idx].state == BranchState::Exited {
            on_branch_exit(ctx, run, branch_idx);
        }
        return;
    }

    // Lifecycle transition accepted -> running at first effect attempt.
    if !run.running_marked {
        if ctx
            .journal
            .set_lifecycle(run.execution_id, JournalLifecycle::Running)
            .is_err()
        {
            run.fail_closed_journal(JournalError::Refused);
            return;
        }
        run.running_marked = true;
    }

    let attempt = current_attempt(run, branch_idx);
    let node_key = run.node_key(node);
    let idempotency_key = run.idempotency_key(&node_key, attempt, false);
    let now = ctx.clock.monotonic_ms();

    // Durable preparation strictly BEFORE dispatch.
    if ctx
        .journal
        .prepare(PreparedEntry {
            execution_id: run.execution_id,
            node_key: node_key.clone(),
            attempt,
            action_type: action_type.clone(),
            idempotency_key: idempotency_key.clone(),
            prepared_at_monotonic_ms: now,
        })
        .is_err()
    {
        run.fail_closed_journal(JournalError::Refused);
        return;
    }

    let slot_free = run.used_global < MAX_CONCURRENCY_GLOBAL
        && run.used_per_action.get(&action_type).copied().unwrap_or(0) < MAX_CONCURRENCY_PER_ACTION;
    if slot_free {
        *run.used_per_action.entry(action_type.clone()).or_insert(0) += 1;
        run.used_global += 1;
        dispatch_effect(
            ctx,
            run,
            SlotWaiter {
                branch: branch_idx,
                node,
                action_type,
                attempt,
                idempotency_key,
                node_deadline_abs: override_ms.map(|ms| now.saturating_add(ms)),
            },
            now,
        );
    } else {
        run.branches[branch_idx].state = BranchState::WaitingSlot { node };
        run.slot_waiters.push_back(SlotWaiter {
            branch: branch_idx,
            node,
            action_type,
            attempt,
            idempotency_key,
            node_deadline_abs: override_ms.map(|ms| now.saturating_add(ms)),
        });
    }
}

fn current_attempt(run: &ExecutionRun, branch_idx: usize) -> u32 {
    run.branches[branch_idx]
        .stack
        .iter()
        .rev()
        .find_map(|frame| match frame {
            Frame::Retry { attempts_done, .. } => Some(*attempts_done),
            _ => None,
        })
        .unwrap_or(0)
}

fn complete_node(run: &mut ExecutionRun, branch_idx: usize, _node: NodeIdx) {
    // Completion propagation unwinds the branch stack: sequence frames
    // advance to their next child, retry frames pop (body succeeded), and
    // the Root sentinel ends the branch. Joins are driven by child exits.
    loop {
        let Some(popped) = run.branches[branch_idx].stack.pop() else {
            run.branches[branch_idx].state = BranchState::Exited;
            return;
        };
        match popped {
            Frame::Root => {
                run.branches[branch_idx].state = BranchState::Exited;
                return;
            }
            Frame::Seq {
                container,
                mut cursor,
            } => {
                cursor += 1;
                let kids = run.graph.children(container);
                if let Some(next) = kids.get(cursor).copied() {
                    run.branches[branch_idx]
                        .stack
                        .push(Frame::Seq { container, cursor });
                    run.branches[branch_idx].state = BranchState::Ready(next);
                    return;
                }
            }
            Frame::Retry { .. } => {}
            Frame::ParJoin { .. } => {
                run.terminal = Some(TerminalState::Failed {
                    reason: FailureReason::InternalError,
                });
                return;
            }
        }
    }
}

fn node_failed(
    ctx: &mut DriverCtx<'_>,
    run: &mut ExecutionRun,
    branch_idx: usize,
    _node: NodeIdx,
    reason: FailureReason,
) {
    // Retry interception precedes policy: exhausted retries propagate.
    if let Some(retry_position) = run.branches[branch_idx]
        .stack
        .iter()
        .rposition(|frame| matches!(frame, Frame::Retry { .. }))
    {
        // The retried unit is the retry node's body, not the failing node,
        // so nested-container bodies re-enter from the body root.
        let retry_node = match run.branches[branch_idx].stack[retry_position] {
            Frame::Retry { node, .. } => node,
            _ => unreachable!("rposition matched Retry above"),
        };
        let body_root = run.graph.children(retry_node)[0];
        let body_ready = {
            let frame = &mut run.branches[branch_idx].stack[retry_position];
            if let Frame::Retry {
                attempts_total,
                attempts_done,
                ..
            } = frame
            {
                if *attempts_done + 1 < *attempts_total {
                    *attempts_done += 1;
                    let backoff =
                        RETRY_BACKOFF_BASE_MS << (*attempts_done - 1).min(u32::from(u8::MAX));
                    run.branches[branch_idx].state = BranchState::Sleeping {
                        until_mono: ctx.clock.monotonic_ms() + backoff,
                        next: WakeNext::Enter(body_root),
                    };
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if body_ready {
            return;
        }
        run.branches[branch_idx].stack.remove(retry_position);
    }

    match run.graph.failure_policy() {
        FailurePolicy::Stop => {
            run.terminal = Some(TerminalState::Failed {
                reason: reason.clone(),
            });
        }
        FailurePolicy::Continue => {
            if run.first_failure.is_none() {
                run.first_failure = Some(reason);
            }
            // The failed step is skipped for control flow; remaining
            // reachable work still runs and the terminal reports `failed`.
            complete_node(run, branch_idx, _node);
        }
        FailurePolicy::Compensate => {
            if run.first_failure.is_none() {
                run.first_failure = Some(reason);
            }
            begin_compensation_drain(run);
        }
    }
}

fn begin_compensation_drain(run: &mut ExecutionRun) {
    run.compensating = true;
    for branch in &mut run.branches {
        if !matches!(branch.state, BranchState::Exited) {
            branch.state = BranchState::Exited;
        }
    }
    // Slot waiters never dispatched; their preparations close cleanly.
    run.slot_waiters.clear();
}

fn on_branch_exit(_ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun, branch_idx: usize) {
    run.branches[branch_idx].state = BranchState::Exited;
    if run.compensating {
        return;
    }
    let Some(join) = run.branches[branch_idx].join.take() else {
        return;
    };
    let parent = join.parent_branch;
    let Some(position) = run.branches[parent]
        .stack
        .iter()
        .rposition(|frame| {
            matches!(frame, Frame::ParJoin { container, .. } if *container == join.parallel_node)
        })
    else {
        run.terminal = Some(TerminalState::Failed {
            reason: FailureReason::InternalError,
        });
        return;
    };
    let exhausted = {
        let frame = &mut run.branches[parent].stack[position];
        if let Frame::ParJoin { remaining, .. } = frame {
            *remaining = remaining.saturating_sub(1);
            *remaining == 0
        } else {
            false
        }
    };
    if exhausted {
        run.branches[parent].stack.remove(position);
        complete_node(run, parent, join.parallel_node);
        if run.branches[parent].state == BranchState::Exited && !run.compensating {
            on_branch_exit(_ctx, run, parent);
        }
    }
}

fn finalize(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun) {
    if run.compensating {
        run_compensations(ctx, run);
    }
    if run.terminal.is_none() {
        let reason = run.first_failure.clone();
        run.terminal = Some(match reason {
            Some(reason) => TerminalState::Failed { reason },
            None => TerminalState::Succeeded,
        });
    }
}

fn run_compensations(ctx: &mut DriverCtx<'_>, run: &mut ExecutionRun) {
    // Reverse completion order (`TECHNICAL_SPEC` §5 compensation unwind).
    for index in (0..run.compensation_targets.len()).rev() {
        let target = run.compensation_targets[index].clone();
        let Some(registration) = ctx.registry.lookup(&target.action_type) else {
            run.fail_closed_journal(JournalError::Refused);
            return;
        };
        if let Decision::Denied { .. } =
            ctx.authorize(&run.subject, &target.capability, registration)
        {
            run.evidence.push(EffectRecord {
                node_key: run.node_key(target.compensation_node),
                action_type: target.action_type,
                attempt: 0,
                is_compensation: true,
                outcome: "failed".to_string(),
                failure_code: None,
                observed_at_monotonic_ms: ctx.clock.monotonic_ms(),
            });
            continue;
        }
        if run.journal_fault.is_none()
            && ctx
                .journal
                .prepare(PreparedEntry {
                    execution_id: run.execution_id,
                    node_key: run.node_key(target.compensation_node),
                    attempt: 0,
                    action_type: target.action_type.clone(),
                    idempotency_key: target.idempotency_key.clone(),
                    prepared_at_monotonic_ms: ctx.clock.monotonic_ms(),
                })
                .is_err()
        {
            run.fail_closed_journal(JournalError::Refused);
            return;
        }
        if run.journal_fault.is_some() {
            return;
        }
        let request = EffectRequest {
            execution_id: run.execution_id,
            node_key: run.node_key(target.compensation_node),
            action_type: target.action_type.clone(),
            capability: target.capability.clone(),
            params: target.params.clone(),
            idempotency_key: target.idempotency_key,
            attempt: 0,
            is_compensation: true,
        };
        let now = ctx.clock.monotonic_ms();
        match registration.port().invoke(request) {
            Err(DispatchUnavailable) => {
                let _ = ctx.journal.resolve_prepared(
                    run.execution_id,
                    &run.node_key(target.compensation_node),
                    0,
                );
                run.evidence.push(EffectRecord {
                    node_key: run.node_key(target.compensation_node),
                    action_type: target.action_type,
                    attempt: 0,
                    is_compensation: true,
                    outcome: "failed".to_string(),
                    failure_code: None,
                    observed_at_monotonic_ms: now,
                });
            }
            Ok(EffectResponse::Immediate(outcome)) => {
                let _ = ctx.journal.resolve_prepared(
                    run.execution_id,
                    &run.node_key(target.compensation_node),
                    0,
                );
                run.evidence.push(EffectRecord {
                    node_key: run.node_key(target.compensation_node),
                    action_type: target.action_type,
                    attempt: 0,
                    is_compensation: true,
                    outcome: outcome.token().to_string(),
                    failure_code: outcome.failure_code().map(str::to_string),
                    observed_at_monotonic_ms: now,
                });
            }
            Ok(EffectResponse::Delayed {
                duration_ms,
                outcome,
            }) => {
                ctx.advance_clock(duration_ms);
                let settled = ctx.clock.monotonic_ms();
                let _ = ctx.journal.resolve_prepared(
                    run.execution_id,
                    &run.node_key(target.compensation_node),
                    0,
                );
                run.evidence.push(EffectRecord {
                    node_key: run.node_key(target.compensation_node),
                    action_type: target.action_type,
                    attempt: 0,
                    is_compensation: true,
                    outcome: outcome.token().to_string(),
                    failure_code: outcome.failure_code().map(str::to_string),
                    observed_at_monotonic_ms: settled,
                });
            }
        }
        if run.journal_fault.is_some() || run.terminal.is_some() {
            return;
        }
    }
}

impl DriverCtx<'_> {
    fn advance_clock(&mut self, delta_ms: u64) {
        if let Some(control) = self.time_control {
            control.advance_ms(delta_ms);
        }
    }

    fn authorize(
        &self,
        subject: &openstream_domain::grant::SubjectRef,
        capability: &Capability,
        registration: &ActionRegistration,
    ) -> Decision {
        let request = CapabilityRequest {
            subject: subject.clone(),
            capability: capability.clone(),
        };
        // Poisoning keeps current (deny-by-default) state: a panic elsewhere
        // must never upgrade authority.
        let ledger = self
            .ledger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ledger.evaluate(&request, registration.manifest())
    }
}

fn eval_condition(condition: &crate::graph::Condition, vars: &VariableMap) -> bool {
    use crate::graph::ConditionOp;
    let current = vars.get(&condition.variable);
    match condition.op {
        ConditionOp::Exists => current.is_some(),
        ConditionOp::Equals => current.is_some_and(|value| value == &condition.operand),
        ConditionOp::NotEquals => !current.is_some_and(|value| value == &condition.operand),
    }
}

fn apply_transform(run: &mut ExecutionRun, op: &crate::graph::TransformOp) -> bool {
    use crate::graph::TransformOp;
    match op {
        TransformOp::Set { variable, value } => {
            if !run.vars.contains_key(variable) && run.vars.len() >= MAX_VARIABLES {
                return false;
            }
            if serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() > MAX_VARIABLE_VALUE_BYTES) {
                return false;
            }
            run.vars.insert(variable.clone(), value.clone());
            true
        }
        TransformOp::Copy { from, to } => {
            let Some(value) = run.vars.get(from).cloned() else {
                return false;
            };
            if !run.vars.contains_key(to) && run.vars.len() >= MAX_VARIABLES {
                return false;
            }
            run.vars.insert(to.clone(), value);
            true
        }
        TransformOp::AddInt { variable, delta } => {
            let Some(value) = run.vars.get_mut(variable) else {
                return false;
            };
            let Some(current) = value.as_i64() else {
                return false;
            };
            match current.checked_add(*delta) {
                Some(total) => {
                    *value = serde_json::Value::Number(serde_json::Number::from(total));
                    true
                }
                None => false,
            }
        }
    }
}
