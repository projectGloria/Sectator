//! Per-indexer circuit breaker: after repeated failures an indexer is skipped for a
//! cooldown (doubling up to a cap), then given a single half-open trial request.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const FAILURE_THRESHOLD: u32 = 3;
pub const BASE_COOLDOWN: Duration = Duration::from_secs(60);
pub const MAX_COOLDOWN: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Closed: request normally.
    Allow,
    /// Cooldown elapsed: one trial request; its outcome closes or re-opens the circuit.
    Trial,
    /// Open: skip without a request.
    Skip { retry_in: Duration },
}

#[derive(Debug, Default, Clone)]
struct Circuit {
    failures: u32,
    trips: u32,
    open_until: Option<Instant>,
    trial_in_flight: bool,
}

#[derive(Default)]
pub struct Breaker {
    circuits: Mutex<HashMap<String, Circuit>>,
}

fn cooldown(trips: u32) -> Duration {
    let factor = 1u32 << trips.saturating_sub(1).min(16);
    (BASE_COOLDOWN * factor).min(MAX_COOLDOWN)
}

impl Breaker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn admit(&self, id: &str) -> Admission {
        self.admit_at(id, Instant::now())
    }

    pub fn admit_at(&self, id: &str, now: Instant) -> Admission {
        let mut map = self.circuits.lock().unwrap();
        let Some(c) = map.get_mut(id) else { return Admission::Allow };
        match c.open_until {
            None => Admission::Allow,
            Some(until) if now < until => Admission::Skip { retry_in: until - now },
            Some(_) if c.trial_in_flight => Admission::Skip { retry_in: Duration::ZERO },
            Some(_) => {
                c.trial_in_flight = true;
                Admission::Trial
            }
        }
    }

    pub fn record_success(&self, id: &str) {
        self.circuits.lock().unwrap().remove(id);
    }

    /// Cancellations are not the indexer's fault: release a trial slot without counting.
    pub fn record_neutral(&self, id: &str) {
        if let Some(c) = self.circuits.lock().unwrap().get_mut(id) {
            c.trial_in_flight = false;
        }
    }

    pub fn record_failure(&self, id: &str) {
        self.record_failure_at(id, Instant::now())
    }

    pub fn record_failure_at(&self, id: &str, now: Instant) {
        let mut map = self.circuits.lock().unwrap();
        let c = map.entry(id.to_string()).or_default();
        c.failures += 1;
        let was_trial = c.trial_in_flight;
        c.trial_in_flight = false;
        if was_trial || c.failures >= FAILURE_THRESHOLD {
            c.trips += 1;
            c.open_until = Some(now + cooldown(c.trips));
        }
    }

    pub fn reset(&self) {
        self.circuits.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_after_threshold_and_recovers() {
        let b = Breaker::new();
        let t0 = Instant::now();
        for _ in 0..FAILURE_THRESHOLD - 1 {
            b.record_failure_at("x", t0);
            assert_eq!(b.admit_at("x", t0), Admission::Allow);
        }
        b.record_failure_at("x", t0);
        assert!(matches!(b.admit_at("x", t0), Admission::Skip { .. }));
        assert_eq!(b.admit_at("other", t0), Admission::Allow);

        let later = t0 + BASE_COOLDOWN;
        assert_eq!(b.admit_at("x", later), Admission::Trial);
        // Only one trial at a time.
        assert!(matches!(b.admit_at("x", later), Admission::Skip { .. }));
        b.record_success("x");
        assert_eq!(b.admit_at("x", later), Admission::Allow);
    }

    #[test]
    fn failed_trial_doubles_cooldown_up_to_cap() {
        let b = Breaker::new();
        let mut now = Instant::now();
        for _ in 0..FAILURE_THRESHOLD {
            b.record_failure_at("x", now);
        }
        let mut expected = BASE_COOLDOWN;
        for _ in 0..6 {
            match b.admit_at("x", now) {
                Admission::Skip { retry_in } => assert_eq!(retry_in, expected),
                other => panic!("expected skip, got {other:?}"),
            }
            now += expected;
            assert_eq!(b.admit_at("x", now), Admission::Trial);
            b.record_failure_at("x", now);
            expected = (expected * 2).min(MAX_COOLDOWN);
        }
        assert_eq!(expected, MAX_COOLDOWN);
    }

    #[test]
    fn neutral_outcome_frees_trial() {
        let b = Breaker::new();
        let t0 = Instant::now();
        for _ in 0..FAILURE_THRESHOLD {
            b.record_failure_at("x", t0);
        }
        let later = t0 + BASE_COOLDOWN;
        assert_eq!(b.admit_at("x", later), Admission::Trial);
        b.record_neutral("x");
        assert_eq!(b.admit_at("x", later), Admission::Trial);
    }
}
