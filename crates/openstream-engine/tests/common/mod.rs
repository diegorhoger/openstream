//! Shared deterministic harness for engine contract tests.
//!
//! Everything runs on [`FakeClock`]: no real sleeps anywhere, and every
//! scheduled wakeup lands on an asserted virtual millisecond. Items here
//! are consumed across sibling test binaries; not every binary uses every
//! helper.

#![allow(dead_code)]

use openstream_domain::capability::Capability;
use openstream_domain::grant::{ConsentEvidence, ConsentKind, GrantLedger, SubjectRef};
use openstream_engine::{
    ActionRegistration, ActionRegistry, CancelSignal, Clock, DispatchUnavailable, EffectOutcome,
    EffectPort, EffectRequest, EffectResponse, ExecuteRequest, ExecutionReceipt, FailurePolicy,
    FakeClock, MessageId, RawGraph, RuntimeBuilder, SourceDeviceId, TimeControl, ValidatedGraph,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const WALL_START: i64 = 1_700_000_000_000;
pub const EXPIRY_MARGIN_MS: i64 = 60_000;

/// Every consent kind, so fixture grants satisfy any capability class.
pub fn all_consent() -> Vec<ConsentKind> {
    vec![
        ConsentKind::InstallReview,
        ConsentKind::FirstUse,
        ConsentKind::DestructiveArming,
        ConsentKind::ExplicitSelection,
        ConsentKind::ExactTupleReview,
    ]
}

pub fn subject() -> SubjectRef {
    SubjectRef::builtin("deck-actions").unwrap()
}

pub fn device() -> SourceDeviceId {
    SourceDeviceId::try_new("peer:test-device").unwrap()
}

pub fn midi(device_name: &str) -> Capability {
    Capability::MidiSend {
        device: device_name.to_string(),
    }
}

pub fn notify() -> Capability {
    Capability::NotificationShow
}

/// Ledger pre-consented for the fixture subject over `capabilities`.
pub fn ledger_with(capabilities: &[Capability]) -> Arc<Mutex<GrantLedger>> {
    let mut ledger = GrantLedger::new();
    let evidence = ConsentEvidence::try_new(all_consent(), WALL_START).unwrap();
    for capability in capabilities {
        ledger
            .create_grant(subject(), capability.clone(), evidence.clone(), WALL_START)
            .unwrap_or_else(|_| panic!("fixture grant must satisfy its consent class"));
    }
    Arc::new(Mutex::new(ledger))
}

/// Ordered cross-component event log proving admission ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Journal admitted a command with the given lifecycle token.
    Admit {
        lifecycle: &'static str,
        at_mono: u64,
    },
    /// Journal transitioned lifecycle.
    Lifecycle {
        lifecycle: &'static str,
        at_mono: u64,
    },
    /// Durable prepared record written before dispatch.
    Prepare {
        node: String,
        idempotency_key: String,
        at_mono: u64,
    },
    /// Prepared record resolved with terminal evidence.
    Resolve { node: String, at_mono: u64 },
    /// Adapter port actually invoked.
    Dispatch {
        node: String,
        idempotency_key: String,
        attempt: u32,
        is_compensation: bool,
        at_mono: u64,
    },
}

#[derive(Debug, Default)]
pub struct EventLog(pub Mutex<Vec<Event>>);

impl EventLog {
    pub fn push(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }

    pub fn snapshot(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

/// One scripted response consumed per invocation; an exhausted script
/// repeats its final step so simple success paths need one entry.
#[derive(Debug, Clone)]
pub enum Step {
    Ok,
    Fail(&'static str),
    Unknown,
    Delay(u64),
    DelayFail(u64, &'static str),
    Unavailable,
}

/// Scripted effect port recording every dispatch into the shared log.
#[derive(Debug)]
pub struct ScriptedPort {
    script: Mutex<VecDeque<Step>>,
    events: Arc<EventLog>,
    clock: Arc<dyn Clock>,
}

impl ScriptedPort {
    pub fn new(steps: Vec<Step>, events: Arc<EventLog>, clock: Arc<dyn Clock>) -> Arc<Self> {
        let mut queue: VecDeque<Step> = steps.into_iter().collect();
        if queue.is_empty() {
            queue.push_back(Step::Ok);
        }
        Arc::new(Self {
            script: Mutex::new(queue),
            events,
            clock,
        })
    }

    fn next_step(&self) -> Step {
        let mut script = self.script.lock().unwrap();
        match script.pop_front() {
            Some(step) => {
                script.push_back(step.clone());
                step
            }
            None => unreachable!("queue never empties"),
        }
    }
}

impl EffectPort for ScriptedPort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        match self.next_step() {
            Step::Unavailable => Err(DispatchUnavailable),
            step => {
                self.events.push(Event::Dispatch {
                    node: request.node_key.to_string(),
                    idempotency_key: request.idempotency_key.clone(),
                    attempt: request.attempt,
                    is_compensation: request.is_compensation,
                    at_mono: self.clock.monotonic_ms(),
                });
                Ok(match step {
                    Step::Ok => EffectResponse::Immediate(EffectOutcome::Succeeded),
                    Step::Fail(code) => EffectResponse::Immediate(EffectOutcome::Failed {
                        code: code.to_string(),
                    }),
                    Step::Unknown => EffectResponse::Immediate(EffectOutcome::Unknown),
                    Step::Delay(ms) => EffectResponse::Delayed {
                        duration_ms: ms,
                        outcome: EffectOutcome::Succeeded,
                    },
                    Step::DelayFail(ms, code) => EffectResponse::Delayed {
                        duration_ms: ms,
                        outcome: EffectOutcome::Failed {
                            code: code.to_string(),
                        },
                    },
                    Step::Unavailable => unreachable!("handled above"),
                })
            }
        }
    }
}

/// Concurrency-observing port: records `[start, start+duration)` intervals
/// so tests can assert peak simultaneous in-flight effects exactly.
#[derive(Debug)]
pub struct IntervalPort {
    default_duration: u64,
    events: Arc<EventLog>,
    clock: Arc<dyn Clock>,
    intervals: Mutex<Vec<(u64, u64)>>,
}

impl IntervalPort {
    pub fn new(default_duration: u64, events: Arc<EventLog>, clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            default_duration,
            events,
            clock,
            intervals: Mutex::new(Vec::new()),
        })
    }

    pub fn interval_count(&self) -> usize {
        self.intervals.lock().unwrap().len()
    }

    /// Observed maximum number of overlapping effect intervals.
    pub fn peak_concurrent(&self) -> usize {
        let intervals = self.intervals.lock().unwrap().clone();
        let mut points: Vec<(u64, i32)> = Vec::with_capacity(intervals.len() * 2);
        for (start, end) in intervals {
            points.push((start, 1));
            points.push((end, -1));
        }
        // Ends sort before starts at identical stamps (slot freed then
        // reused), matching the scheduler's release-then-acquire order.
        points.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut active = 0i32;
        let mut peak = 0i32;
        for (_, delta) in points {
            active += delta;
            peak = peak.max(active);
        }
        usize::try_from(i64::from(peak)).unwrap_or(0)
    }
}

impl EffectPort for IntervalPort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        let start = self.clock.monotonic_ms();
        self.intervals
            .lock()
            .unwrap()
            .push((start, start + self.default_duration));
        self.events.push(Event::Dispatch {
            node: request.node_key.to_string(),
            idempotency_key: request.idempotency_key.clone(),
            attempt: request.attempt,
            is_compensation: request.is_compensation,
            at_mono: start,
        });
        Ok(EffectResponse::Delayed {
            duration_ms: self.default_duration,
            outcome: EffectOutcome::Succeeded,
        })
    }
}

/// Registration helper binding one port under a name with declarations.
pub fn register_action(
    registry: &mut ActionRegistry,
    name: &str,
    scopes: Vec<Capability>,
    idempotent: bool,
    safe_compensation: bool,
    port: Arc<dyn EffectPort>,
) {
    let idempotency = if idempotent {
        openstream_engine::IdempotencyClass::Idempotent
    } else {
        openstream_engine::IdempotencyClass::NonIdempotent
    };
    let registration =
        ActionRegistration::try_new(name, scopes, idempotency, safe_compensation, port).unwrap();
    registry.register(registration).unwrap();
}

pub struct Harness {
    pub clock: Arc<FakeClock>,
    pub events: Arc<EventLog>,
}

impl Harness {
    pub fn new() -> Self {
        Self {
            clock: Arc::new(FakeClock::new(WALL_START, 0)),
            events: Arc::new(EventLog::default()),
        }
    }

    pub fn runtime(
        &self,
        registry: ActionRegistry,
        ledger: Arc<Mutex<GrantLedger>>,
    ) -> openstream_engine::ActionRuntime {
        RuntimeBuilder::new()
            .clock(self.clock.clone())
            .time_control(self.clock.clone() as Arc<dyn TimeControl>)
            .registry(registry)
            .grant_ledger(ledger)
            .build()
            .unwrap()
    }

    pub fn runtime_with_journal(
        &self,
        registry: ActionRegistry,
        ledger: Arc<Mutex<GrantLedger>>,
        journal: Box<dyn openstream_engine::ExecutionJournal>,
    ) -> openstream_engine::ActionRuntime {
        RuntimeBuilder::new()
            .clock(self.clock.clone())
            .time_control(self.clock.clone() as Arc<dyn TimeControl>)
            .journal(journal)
            .registry(registry)
            .grant_ledger(ledger)
            .build()
            .unwrap()
    }

    pub fn expires_at(&self) -> i64 {
        WALL_START + EXPIRY_MARGIN_MS
    }

    pub fn now(&self) -> u64 {
        self.clock.monotonic_ms()
    }

    pub fn advance(&self, ms: u64) {
        self.clock.advance(ms);
    }
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard execute request against `graph`.
pub fn request_for(harness: &Harness, graph: &Arc<ValidatedGraph>) -> ExecuteRequest {
    ExecuteRequest {
        source_device_id: device(),
        message_id: MessageId::generate(),
        subject: subject(),
        graph: Arc::clone(graph),
        variables: Default::default(),
        expires_at_wall_ms: harness.expires_at(),
        cancel: None::<CancelSignal>,
    }
}

/// Runs one command synchronously, panicking on rejection.
pub fn run_ok(
    runtime: &mut openstream_engine::ActionRuntime,
    harness: &Harness,
    graph: &Arc<ValidatedGraph>,
) -> ExecutionReceipt {
    runtime.execute(request_for(harness, graph)).unwrap()
}

/// Builds and validates a single-action graph (`entry` node only).
pub fn single_action_graph(
    action_type: &str,
    capability: Capability,
    policy: FailurePolicy,
    registry: &ActionRegistry,
) -> Arc<ValidatedGraph> {
    let mut raw = RawGraph::new(policy);
    raw.add_node(
        node_key("a"),
        openstream_engine::NodeKind::Action {
            action_type: action_type.to_string(),
            capability,
            params: serde_json::Value::Null,
            deadline_override_ms: None,
        },
    )
    .unwrap();
    let raw = raw.entry(node_key("a"));
    Arc::new(ValidatedGraph::build(raw, registry).unwrap())
}

pub fn node_key(raw: &str) -> openstream_engine::NodeKey {
    openstream_engine::NodeKey::try_new(raw).unwrap()
}

/// In-memory journal instrumented into the shared event log so tests can
/// prove durable-evidence ordering relative to adapter dispatch.
#[derive(Debug)]
pub struct InstrumentedJournal {
    inner: openstream_engine::MemoryJournal,
    events: Arc<EventLog>,
    clock: Arc<dyn Clock>,
    refuse_prepare: std::sync::atomic::AtomicBool,
}

impl InstrumentedJournal {
    pub fn new(events: Arc<EventLog>, clock: Arc<dyn Clock>) -> Box<Self> {
        Box::new(Self {
            inner: openstream_engine::MemoryJournal::new(),
            events,
            clock,
            refuse_prepare: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn set_refuse_prepare(&self, refused: bool) {
        self.refuse_prepare
            .store(refused, std::sync::atomic::Ordering::SeqCst);
    }
}

impl openstream_engine::ExecutionJournal for InstrumentedJournal {
    fn admit(
        &mut self,
        entry: openstream_engine::AdmissionEntry,
    ) -> Result<(), openstream_engine::JournalError> {
        let lifecycle = match &entry.lifecycle {
            openstream_engine::JournalLifecycle::Accepted => "accepted",
            openstream_engine::JournalLifecycle::Running => "running",
            openstream_engine::JournalLifecycle::Succeeded => "succeeded",
            openstream_engine::JournalLifecycle::Failed { .. } => "failed",
            openstream_engine::JournalLifecycle::Cancelled => "cancelled",
            openstream_engine::JournalLifecycle::Expired => "expired",
            openstream_engine::JournalLifecycle::OutcomeUnknown => "outcome_unknown",
        };
        let at = self.clock.monotonic_ms();
        self.inner.admit(entry)?;
        self.events.push(Event::Admit {
            lifecycle,
            at_mono: at,
        });
        Ok(())
    }

    fn lookup(
        &self,
        key: &openstream_engine::DedupeKey,
    ) -> Option<openstream_engine::AdmissionEntry> {
        self.inner.lookup(key)
    }

    fn set_lifecycle(
        &mut self,
        execution_id: openstream_engine::ExecutionId,
        lifecycle: openstream_engine::JournalLifecycle,
    ) -> Result<(), openstream_engine::JournalError> {
        let token = match &lifecycle {
            openstream_engine::JournalLifecycle::Accepted => "accepted",
            openstream_engine::JournalLifecycle::Running => "running",
            openstream_engine::JournalLifecycle::Succeeded => "succeeded",
            openstream_engine::JournalLifecycle::Failed { .. } => "failed",
            openstream_engine::JournalLifecycle::Cancelled => "cancelled",
            openstream_engine::JournalLifecycle::Expired => "expired",
            openstream_engine::JournalLifecycle::OutcomeUnknown => "outcome_unknown",
        };
        self.inner.set_lifecycle(execution_id, lifecycle)?;
        self.events.push(Event::Lifecycle {
            lifecycle: token,
            at_mono: self.clock.monotonic_ms(),
        });
        Ok(())
    }

    fn prepare(
        &mut self,
        entry: openstream_engine::PreparedEntry,
    ) -> Result<(), openstream_engine::JournalError> {
        if self
            .refuse_prepare
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(openstream_engine::JournalError::Refused);
        }
        let node = entry.node_key.to_string();
        let key = entry.idempotency_key.clone();
        self.inner.prepare(entry)?;
        self.events.push(Event::Prepare {
            node,
            idempotency_key: key,
            at_mono: self.clock.monotonic_ms(),
        });
        Ok(())
    }

    fn resolve_prepared(
        &mut self,
        execution_id: openstream_engine::ExecutionId,
        node_key: &openstream_engine::NodeKey,
        attempt: u32,
    ) -> Result<(), openstream_engine::JournalError> {
        let node = node_key.to_string();
        self.inner
            .resolve_prepared(execution_id, node_key, attempt)?;
        self.events.push(Event::Resolve {
            node,
            at_mono: self.clock.monotonic_ms(),
        });
        Ok(())
    }

    fn unresolved_prepared(&self) -> Vec<openstream_engine::PreparedEntry> {
        self.inner.unresolved_prepared()
    }

    fn prune(&mut self, now_wall_ms: i64, retention_ms: i64) {
        self.inner.prune(now_wall_ms, retention_ms);
    }

    fn snapshot_admissions(&self) -> Vec<openstream_engine::AdmissionEntry> {
        self.inner.snapshot_admissions()
    }
}
