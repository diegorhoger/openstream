//! Deterministic clock port.
//!
//! Deadline measurement switches to monotonic runtime clocks at admission
//! (`ADR-0005` decision item 3; `OSCP_MESSAGES.md` §5), and every test in
//! this crate drives time through the injected [`Clock`] so behavior is
//! fully deterministic: no real sleeps, no wall-clock reads anywhere in the
//! scheduler.
//!
//! No system-clock implementation ships in this milestone deliberately:
//! the desktop composition root (#16) owns real monotonic/wall sources,
//! and shipping a `std::time`-based default here would invite
//! non-deterministic test paths. [`FakeClock`] is the deterministic
//! instrument for this crate's contract tests and for embedding hosts that
//! drive virtual time.

use core::fmt;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// Time source used by the runtime. Monotonic milliseconds drive deadlines,
/// scheduling, and effect completion; wall-clock epoch milliseconds are
/// consulted only for envelope expiry at admission (never after).
pub trait Clock: fmt::Debug + Send + Sync {
    /// Current monotonic clock reading in milliseconds. Must never go
    /// backwards within one runtime instance.
    fn monotonic_ms(&self) -> u64;

    /// Current UTC wall-clock time as epoch milliseconds. Used only for the
    /// admission-time expiry check against `expires_at`.
    fn wall_now_ms(&self) -> i64;
}

/// Fully controllable fake clock: both counters advance only when a test
/// moves them, so every scheduled wakeup, deadline, and backoff lands on an
/// exact asserted value.
#[derive(Debug, Default)]
pub struct FakeClock {
    monotonic_ms: AtomicU64,
    wall_now_ms: AtomicI64,
}

impl FakeClock {
    /// Creates a clock starting at the given wall/monotonic pair.
    #[must_use]
    pub fn new(wall_start_ms: i64, monotonic_start_ms: u64) -> Self {
        Self {
            monotonic_ms: AtomicU64::new(monotonic_start_ms),
            wall_now_ms: AtomicI64::new(wall_start_ms),
        }
    }

    /// Advances monotonic and wall time together by `delta_ms` (the common
    /// case: virtual time passes uniformly).
    pub fn advance(&self, delta_ms: u64) {
        let delta = i64::try_from(delta_ms).unwrap_or(i64::MAX);
        self.monotonic_ms.fetch_add(delta_ms, Ordering::SeqCst);
        self.wall_now_ms.fetch_add(delta, Ordering::SeqCst);
    }

    /// Moves only the monotonic counter forward. Deadlines shift without
    /// changing envelope-expiry answers.
    pub fn advance_monotonic(&self, delta_ms: u64) {
        self.monotonic_ms.fetch_add(delta_ms, Ordering::SeqCst);
    }

    /// Overwrites the wall-clock reading (simulating skew without touching
    /// running deadlines — wall skew cannot extend a running effect).
    pub fn set_wall(&self, wall_now_ms: i64) {
        self.wall_now_ms.store(wall_now_ms, Ordering::SeqCst);
    }

    /// Current monotonic reading (test-side mirror of [`Clock`]).
    #[must_use]
    pub fn monotonic(&self) -> u64 {
        self.monotonic_ms.load(Ordering::SeqCst)
    }
}

impl Clock for FakeClock {
    fn monotonic_ms(&self) -> u64 {
        self.monotonic()
    }

    fn wall_now_ms(&self) -> i64 {
        self.wall_now_ms.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, FakeClock};

    #[test]
    fn advance_moves_both_counters() {
        let clock = FakeClock::new(1_000, 0);
        assert_eq!(clock.wall_now_ms(), 1_000);
        assert_eq!(clock.monotonic_ms(), 0);
        clock.advance(250);
        assert_eq!(clock.wall_now_ms(), 1_250);
        assert_eq!(clock.monotonic_ms(), 250);
    }

    #[test]
    fn monotonic_only_and_wall_only_paths_are_independent() {
        let clock = FakeClock::new(10_000, 500);
        clock.advance_monotonic(70);
        assert_eq!(clock.monotonic(), 570);
        assert_eq!(clock.wall_now_ms(), 10_000);
        clock.set_wall(20_000);
        assert_eq!(clock.wall_now_ms(), 20_000);
        assert_eq!(clock.monotonic(), 570);
    }
}
