//! Real time sources for the desktop composition root (issue #16).
//!
//! The engine crate deliberately ships no system-clock implementation
//! (`openstream_engine::clock`): deterministic tests inject `FakeClock`,
//! and the composition root owns the real monotonic/wall sources. This
//! module is that realization. It contains no scheduling or caching logic
//! beyond a process-lifetime monotonic anchor, so every reading is an
//! honest sample of the platform clock.

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use openstream_engine::clock::Clock;

/// Process-wide monotonic anchor. A single `Instant` start point keeps the
/// monotonic reading stable and non-decreasing for the whole runtime even
/// though individual `Instant::now()` samples are taken per call.
static MONOTONIC_ANCHOR: OnceLock<Instant> = OnceLock::new();

fn anchor() -> &'static Instant {
    MONOTONIC_ANCHOR.get_or_init(Instant::now)
}

/// Real clock backed by `std::time`.
///
/// - `monotonic_ms` derives from a process-lifetime `Instant` anchor, which
///   the OS guarantees never goes backwards within one process.
/// - `wall_now_ms` is the UTC epoch millisecond reading, consulted by the
///   engine only for admission-time envelope expiry.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl SystemClock {
    /// Creates the system clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Clock for SystemClock {
    fn monotonic_ms(&self) -> u64 {
        let millis = anchor().elapsed().as_millis();
        // A process would have to run ~584 million years to overflow; the
        // saturating ceiling keeps the conversion total regardless.
        u64::try_from(millis).unwrap_or(u64::MAX)
    }

    fn wall_now_ms(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(ahead) => i64::try_from(ahead.as_millis()).unwrap_or(i64::MAX),
            Err(before) => {
                // Clock set before 1970: report honestly negative instead of
                // clamping into a fabricated present.
                let millis = i64::try_from(before.duration().as_millis()).unwrap_or(i64::MAX);
                -millis
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SystemClock;
    use openstream_engine::clock::Clock;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn monotonic_readings_never_go_backwards() {
        let clock = SystemClock::new();
        let first = clock.monotonic_ms();
        sleep(Duration::from_millis(2));
        assert!(clock.monotonic_ms() >= first);
    }

    #[test]
    fn monotonic_readings_advance_with_real_time() {
        let clock = SystemClock::new();
        let before = clock.monotonic_ms();
        sleep(Duration::from_millis(5));
        let after = clock.monotonic_ms();
        assert!(after > before, "monotonic must advance across real sleep");
    }

    #[test]
    fn wall_reading_is_a_plausible_present_utc_millisecond() {
        let clock = SystemClock::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("host clock is after the Unix epoch in tests")
            .as_millis() as i64;
        let reading = clock.wall_now_ms();
        assert!(
            reading > 1_600_000_000_000,
            "reading {reading} is not a modern timestamp"
        );
        assert!(
            reading <= now + 1_000,
            "wall reading must not run ahead of the host"
        );
    }
}
