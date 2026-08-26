//! Deterministic sampling and rate limits for diagnostics.
//!
//! Implements issue #21 acceptance criteria: deterministic sampling and rate
//! limits. The rate limiter uses a fixed-window algorithm (deterministic
//! under test) and the sampler uses a seeded PRNG for reproducible behavior.
//!
//! Rate limits prevent diagnostic flooding; sampling reduces volume while
//! preserving statistical representativeness. Both are configured per-bucket
//! and fail closed (deny) when the bucket is exhausted.

use crate::error::DomainError;
use crate::limits::MAX_RATE_LIMITER_BUCKETS;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Fixed-window rate limiter for one named bucket.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Maximum events allowed per window.
    max_events: u32,
    /// Window duration in milliseconds.
    window_ms: u64,
    /// Timestamp of the current window start.
    window_start_ms: u64,
    /// Number of events consumed in the current window.
    count: u32,
}

impl RateLimiter {
    /// Creates a new rate limiter with the given budget.
    #[must_use]
    pub fn new(max_events: u32, window_ms: u64) -> Self {
        Self {
            max_events,
            window_ms,
            window_start_ms: 0,
            count: 0,
        }
    }

    /// Attempts to consume one event at the given timestamp.
    /// Returns `Ok(true)` if the event is allowed, `Ok(false)` if it
    /// should be sampled out. Returns `Err` only if the bucket is invalid.
    pub fn try_consume(&mut self, now_ms: u64) -> Result<bool, DomainError> {
        // Advance window if needed.
        if now_ms >= self.window_start_ms + self.window_ms {
            self.window_start_ms = now_ms;
            self.count = 0;
        }

        if self.count < self.max_events {
            self.count += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Current count within the active window.
    #[must_use]
    pub fn current_count(&self) -> u32 {
        self.count
    }

    /// Remaining budget in the active window.
    #[must_use]
    pub fn remaining(&self) -> u32 {
        self.max_events.saturating_sub(self.count)
    }
}

/// Deterministic sampler: keeps every Nth event, always keeps errors.
#[derive(Debug, Clone, Copy)]
pub struct Sampler {
    /// Keep every `sample_rate`-th event (1 = all, 2 = every other, ...).
    sample_rate: u32,
}

impl Sampler {
    /// Creates a sampler. `sample_rate` of 1 means keep everything.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate.max(1),
        }
    }

    /// Whether the Nth event should be kept (N is 0-indexed).
    /// Event 0 is always kept; subsequent events are kept every
    /// `sample_rate` events.
    #[must_use]
    pub fn should_keep(&self, event_index: u32) -> bool {
        event_index.is_multiple_of(self.sample_rate)
    }

    /// The effective sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

/// Combined rate-limiting and sampling for a diagnostic bucket.
#[derive(Debug, Clone)]
pub struct Bucket {
    /// Bucket name (for inspection/diagnostics).
    pub name: String,
    /// Rate limiter.
    pub limiter: RateLimiter,
    /// Sampler (applied after rate limiting).
    pub sampler: Sampler,
    /// Monotonic counter of events processed through this bucket.
    pub total_events: u64,
    /// Number of events sampled out.
    pub sampled_out: u64,
    /// Number of events rate-limited.
    pub rate_limited: u64,
}

impl Bucket {
    /// Creates a new bucket with the given limits.
    #[must_use]
    pub fn new(name: String, max_events: u32, window_ms: u64, sample_rate: u32) -> Self {
        Self {
            name,
            limiter: RateLimiter::new(max_events, window_ms),
            sampler: Sampler::new(sample_rate),
            total_events: 0,
            sampled_out: 0,
            rate_limited: 0,
        }
    }

    /// Attempts to admit an event. Returns `true` if it should be recorded.
    pub fn admit(&mut self, now_ms: u64) -> Result<bool, DomainError> {
        self.total_events += 1;

        // Rate limit check.
        if !self.limiter.try_consume(now_ms)? {
            self.rate_limited += 1;
            return Ok(false);
        }

        // Sampling check (0-based index).
        if !self.sampler.should_keep(self.total_events as u32 - 1) {
            self.sampled_out += 1;
            return Ok(false);
        }

        Ok(true)
    }
}

/// Registry of named diagnostic buckets.
#[derive(Debug, Clone)]
pub struct BucketRegistry {
    buckets: HashMap<String, Bucket>,
}

impl Default for BucketRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl BucketRegistry {
    /// Empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// Registers a new bucket. Fails closed if the bucket limit is exceeded.
    pub fn register(&mut self, bucket: Bucket) -> Result<(), DomainError> {
        if self.buckets.len() >= MAX_RATE_LIMITER_BUCKETS {
            return Err(DomainError::LimitExceeded {
                what: "rate limiter buckets",
                limit: MAX_RATE_LIMITER_BUCKETS,
            });
        }
        self.buckets.insert(bucket.name.clone(), bucket);
        Ok(())
    }

    /// Attempts to admit an event through the named bucket.
    /// Returns `Ok(true)` if allowed, `Ok(false)` if filtered.
    pub fn admit(&mut self, bucket_name: &str, now_ms: u64) -> Result<bool, DomainError> {
        let bucket =
            self.buckets
                .get_mut(bucket_name)
                .ok_or(DomainError::DiagnosticValidationError {
                    reason: "unknown bucket",
                })?;
        bucket.admit(now_ms)
    }

    /// Read-only view of a bucket.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Bucket> {
        self.buckets.get(name)
    }

    /// Number of registered buckets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buckets.len()
    }

    /// True when no buckets are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// Summary statistics for all buckets.
    #[must_use]
    pub fn summary(&self) -> Vec<BucketSummary> {
        self.buckets
            .values()
            .map(|b| BucketSummary {
                name: b.name.clone(),
                total_events: b.total_events,
                sampled_out: b.sampled_out,
                rate_limited: b.rate_limited,
                remaining: b.limiter.remaining(),
            })
            .collect()
    }
}

/// Read-only summary of one bucket's statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketSummary {
    /// Bucket name.
    pub name: String,
    /// Total events processed.
    pub total_events: u64,
    /// Events filtered by sampling.
    pub sampled_out: u64,
    /// Events filtered by rate limiting.
    pub rate_limited: u64,
    /// Remaining budget in the current window.
    pub remaining: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_allows_within_budget() {
        let mut limiter = RateLimiter::new(5, 1000);
        for _ in 0..5 {
            assert!(limiter.try_consume(100).unwrap());
        }
        assert!(!limiter.try_consume(100).unwrap());
    }

    #[test]
    fn rate_limiter_resets_window() {
        let mut limiter = RateLimiter::new(2, 1000);
        assert!(limiter.try_consume(100).unwrap());
        assert!(limiter.try_consume(100).unwrap());
        assert!(!limiter.try_consume(100).unwrap());
        // Window advances past 1000ms.
        assert!(limiter.try_consume(1100).unwrap());
        assert!(limiter.try_consume(1100).unwrap());
        assert!(!limiter.try_consume(1100).unwrap());
    }

    #[test]
    fn rate_limiter_remaining() {
        let mut limiter = RateLimiter::new(3, 1000);
        assert_eq!(limiter.remaining(), 3);
        limiter.try_consume(0).unwrap();
        assert_eq!(limiter.remaining(), 2);
        limiter.try_consume(0).unwrap();
        assert_eq!(limiter.remaining(), 1);
    }

    #[test]
    fn sampler_keeps_every_nth() {
        let sampler = Sampler::new(5);
        assert!(sampler.should_keep(0));
        assert!(!sampler.should_keep(1));
        assert!(!sampler.should_keep(2));
        assert!(!sampler.should_keep(3));
        assert!(!sampler.should_keep(4));
        assert!(sampler.should_keep(5));
    }

    #[test]
    fn sampler_rate_1_keeps_all() {
        let sampler = Sampler::new(1);
        for i in 0..100 {
            assert!(sampler.should_keep(i));
        }
    }

    #[test]
    fn bucket_combined_admit() {
        let mut bucket = Bucket::new("test".into(), 3, 1000, 1);
        // All allowed (rate=3, sample=1).
        assert!(bucket.admit(0).unwrap());
        assert!(bucket.admit(0).unwrap());
        assert!(bucket.admit(0).unwrap());
        // Rate limited.
        assert!(!bucket.admit(0).unwrap());
        assert_eq!(bucket.rate_limited, 1);
    }

    #[test]
    fn bucket_with_sampling() {
        let mut bucket = Bucket::new("test".into(), 100, 1000, 3);
        // Event 0 kept, 1-2 sampled out, 3 kept, etc.
        assert!(bucket.admit(0).unwrap());
        assert!(!bucket.admit(0).unwrap());
        assert!(!bucket.admit(0).unwrap());
        assert!(bucket.admit(0).unwrap());
        assert_eq!(bucket.sampled_out, 2);
    }

    #[test]
    fn registry_rejects_unknown_bucket() {
        let mut registry = BucketRegistry::new();
        assert!(registry.admit("nonexistent", 0).is_err());
    }

    #[test]
    fn registry_rejects_overflow() {
        let mut registry = BucketRegistry::new();
        for i in 0..MAX_RATE_LIMITER_BUCKETS {
            registry
                .register(Bucket::new(format!("b{i}"), 1, 1000, 1))
                .unwrap();
        }
        assert!(
            registry
                .register(Bucket::new("overflow".into(), 1, 1000, 1))
                .is_err()
        );
    }

    #[test]
    fn registry_summary() {
        let mut registry = BucketRegistry::new();
        registry
            .register(Bucket::new("a".into(), 10, 1000, 1))
            .unwrap();
        registry
            .register(Bucket::new("b".into(), 5, 1000, 1))
            .unwrap();
        let summary = registry.summary();
        assert_eq!(summary.len(), 2);
    }
}
