//! Ordered graceful-shutdown sequencer (issue #16).
//!
//! Quitting (tray menu, OS session end) runs a fixed task order exactly
//! once. Individual failures NEVER abort the remaining steps and never
//! prevent exit: every task is best-effort by design because the durable
//! journal already guarantees committed evidence survives process death
//! (`WAL` + `synchronous=FULL`, issue #15). The returned report states the
//! truth about which steps completed and which refused.

/// Fixed shutdown steps in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownStep {
    /// Flip the tray to its shutting-down presentation.
    MarkShellShuttingDown,
    /// Drop the composed engine runtime (closes the SQLite connection it
    /// owns; SQLite auto-checkpoints the WAL on clean close).
    CloseEngineRuntime,
    /// Explicit WAL checkpoint through the shared store handle before the
    /// final handle drops.
    CheckpointExecutionJournal,
    /// Drop the shell's store handle (last reference closes the database).
    ReleaseJournalStore,
    /// Release the single-instance lock file.
    ReleaseInstanceLock,
}

/// Typed step refusal; closed vocabulary, no OS text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownFailure;

impl core::fmt::Display for ShutdownFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("refused")
    }
}

/// One ordered shutdown unit of work.
pub trait ShutdownTask: core::fmt::Debug {
    /// Which fixed step this task performs.
    fn step(&self) -> ShutdownStep;

    /// Performs the step.
    ///
    /// # Errors
    /// [`ShutdownFailure`] when the step could not complete; the sequencer
    /// records the refusal and continues with the remaining steps.
    fn run(&mut self) -> Result<(), ShutdownFailure>;
}

/// Honest record of one graceful shutdown: steps in order, failures stated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Steps that completed, in execution order.
    pub completed: Vec<ShutdownStep>,
    /// Steps that refused, paired with their failure, in execution order.
    pub failed: Vec<(ShutdownStep, ShutdownFailure)>,
}

impl ShutdownReport {
    /// True when no step refused.
    #[must_use]
    pub const fn fully_clean(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Runs every task exactly once, in the given order, regardless of
/// individual failures. The caller supplies the canonical order; the
/// composition root passes the fixed [`ShutdownStep`] sequence above.
#[must_use]
pub fn execute_graceful_shutdown(tasks: &mut [Box<dyn ShutdownTask + '_>]) -> ShutdownReport {
    let mut completed = Vec::with_capacity(tasks.len());
    let mut failed = Vec::new();
    for task in tasks.iter_mut() {
        match task.run() {
            Ok(()) => completed.push(task.step()),
            Err(failure) => failed.push((task.step(), failure)),
        }
    }
    ShutdownReport { completed, failed }
}

#[cfg(test)]
mod tests {
    use super::{
        ShutdownFailure, ShutdownReport, ShutdownStep, ShutdownTask, execute_graceful_shutdown,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct ScriptedTask {
        step: ShutdownStep,
        fails: bool,
        log: Rc<RefCell<Vec<ShutdownStep>>>,
    }

    impl ScriptedTask {
        fn new(step: ShutdownStep, fails: bool, log: &Rc<RefCell<Vec<ShutdownStep>>>) -> Self {
            Self {
                step,
                fails,
                log: Rc::clone(log),
            }
        }
    }

    impl ShutdownTask for ScriptedTask {
        fn step(&self) -> ShutdownStep {
            self.step
        }

        fn run(&mut self) -> Result<(), ShutdownFailure> {
            self.log.borrow_mut().push(self.step);
            if self.fails {
                Err(ShutdownFailure)
            } else {
                Ok(())
            }
        }
    }

    const ORDER: [ShutdownStep; 5] = [
        ShutdownStep::MarkShellShuttingDown,
        ShutdownStep::CloseEngineRuntime,
        ShutdownStep::CheckpointExecutionJournal,
        ShutdownStep::ReleaseJournalStore,
        ShutdownStep::ReleaseInstanceLock,
    ];

    #[test]
    fn all_steps_run_in_order_when_healthy() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut tasks: Vec<Box<dyn ShutdownTask>> = ORDER
            .iter()
            .map(|step| Box::new(ScriptedTask::new(*step, false, &log)) as Box<dyn ShutdownTask>)
            .collect();

        let report = execute_graceful_shutdown(&mut tasks);
        assert!(report.fully_clean());
        assert_eq!(report.completed, ORDER.to_vec());
        assert_eq!(*log.borrow(), ORDER.to_vec());
    }

    #[test]
    fn failures_never_abort_remaining_steps() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let failing_middle = ShutdownStep::CloseEngineRuntime;
        let mut tasks: Vec<Box<dyn ShutdownTask>> = ORDER
            .iter()
            .map(|step| {
                let fails = *step == failing_middle;
                Box::new(ScriptedTask::new(*step, fails, &log)) as Box<dyn ShutdownTask>
            })
            .collect();

        let report = execute_graceful_shutdown(&mut tasks);
        assert_eq!(report.completed.len(), 4);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, failing_middle);
        assert_eq!(
            *log.borrow(),
            ORDER.to_vec(),
            "every later step still ran after the mid-sequence refusal"
        );
    }

    #[test]
    fn sequencer_is_stateless_each_invocation_runs_its_slice() {
        // Exactly-once-per-process gating lives at the composition layer
        // (an atomic guard in main.rs), not in this stateless sequencer.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut tasks: Vec<Box<dyn ShutdownTask>> = ORDER
            .iter()
            .map(|step| Box::new(ScriptedTask::new(*step, false, &log)) as Box<dyn ShutdownTask>)
            .collect();

        let first = execute_graceful_shutdown(&mut tasks);
        assert!(first.fully_clean());
        assert_eq!(*log.borrow(), ORDER.to_vec());

        let second = execute_graceful_shutdown(&mut tasks);
        assert!(second.fully_clean());
        assert_eq!(
            *log.borrow(),
            ORDER
                .iter()
                .chain(ORDER.iter())
                .copied()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn duplicate_steps_still_run_in_the_given_order() {
        // The caller owns the canonical order; the sequencer neither
        // deduplicates nor reorders.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut tasks: Vec<Box<dyn ShutdownTask>> = vec![
            Box::new(ScriptedTask::new(
                ShutdownStep::MarkShellShuttingDown,
                false,
                &log,
            )),
            Box::new(ScriptedTask::new(
                ShutdownStep::MarkShellShuttingDown,
                false,
                &log,
            )),
            Box::new(ScriptedTask::new(
                ShutdownStep::CloseEngineRuntime,
                false,
                &log,
            )),
        ];

        let report = execute_graceful_shutdown(&mut tasks);
        assert_eq!(report.completed.len(), 3);
        assert_eq!(
            *log.borrow(),
            vec![
                ShutdownStep::MarkShellShuttingDown,
                ShutdownStep::MarkShellShuttingDown,
                ShutdownStep::CloseEngineRuntime
            ]
        );
    }

    #[test]
    fn empty_task_list_yields_empty_clean_report() {
        let mut empty: Vec<Box<dyn ShutdownTask>> = Vec::new();
        let report: ShutdownReport = execute_graceful_shutdown(&mut empty);
        assert!(report.fully_clean());
        assert!(report.completed.is_empty());
    }
}
