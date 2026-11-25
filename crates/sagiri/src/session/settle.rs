//! Decides when a new PRIMARY selection is complete enough to be read.
//!
//! Applications take PRIMARY when a selection starts, not when it is finished. Reading at
//! once would cut the selection short, so the watcher waits until the selection has been
//! quiet for a moment. The caller passes in a monotonic time; `Settle` never reads the
//! clock itself.

use std::time::{Duration, Instant};

/// How long a pending selection must be quiet before it is processed.
pub const QUIET: Duration = Duration::from_millis(150);

/// How long a selection may stay pending before it is processed regardless.
pub const LIMIT: Duration = Duration::from_secs(30);

/// A pending selection: when it started and when it last showed activity.
#[derive(Clone, Copy, Debug)]
struct Pending {
    since: Instant,
    last_activity: Instant,
}

/// The settle state machine. Idle until the first owner-change notification.
#[derive(Debug, Default)]
pub struct Settle {
    pending: Option<Pending>,
}

impl Settle {
    pub fn new() -> Self {
        Settle::default()
    }

    /// A PRIMARY owner-change notification arrived at `now`.
    pub fn on_owner_change(&mut self, now: Instant) {
        match &mut self.pending {
            Some(p) => p.last_activity = now,
            None => {
                self.pending = Some(Pending {
                    since: now,
                    last_activity: now,
                })
            }
        }
    }

    /// Whether the pending selection should be processed now. `held` says whether mouse
    /// button 1 or Shift is down. Returns `true` exactly once per pending selection;
    /// afterwards the machine is idle until the next notification.
    pub fn poll(&mut self, now: Instant, held: bool) -> bool {
        let Some(p) = &mut self.pending else {
            return false;
        };
        if held {
            p.last_activity = now;
        }
        let quiet = now.saturating_duration_since(p.last_activity) >= QUIET;
        let expired = now.saturating_duration_since(p.since) >= LIMIT;
        if quiet || expired {
            self.pending = None;
            true
        } else {
            false
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn idle_never_fires() {
        let t0 = Instant::now();
        let mut s = Settle::new();
        assert!(!s.is_pending());
        for ms in [0, 150, 1_000, 30_000, 60_000] {
            assert!(!s.poll(at(t0, ms), false));
            assert!(!s.poll(at(t0, ms), true));
        }
        assert!(!s.is_pending());
    }

    #[test]
    fn fires_after_quiet_period() {
        let t0 = Instant::now();
        let mut s = Settle::new();
        s.on_owner_change(t0);
        assert!(s.is_pending());
        assert!(!s.poll(at(t0, 50), false));
        assert!(!s.poll(at(t0, 100), false));
        assert!(s.poll(at(t0, 150), false));
        assert!(!s.is_pending());
    }

    #[test]
    fn waits_while_button_is_held() {
        let t0 = Instant::now();
        let mut s = Settle::new();
        s.on_owner_change(t0);
        for ms in (50..=2_000).step_by(50) {
            assert!(!s.poll(at(t0, ms), true), "held at {ms} ms");
        }
        assert!(!s.poll(at(t0, 2_050), false));
        assert!(!s.poll(at(t0, 2_100), false));
        assert!(!s.poll(at(t0, 2_149), false));
        assert!(s.poll(at(t0, 2_150), false));
    }

    #[test]
    fn new_notification_restarts_the_wait() {
        let t0 = Instant::now();
        let mut s = Settle::new();
        s.on_owner_change(t0);
        assert!(!s.poll(at(t0, 100), false));
        s.on_owner_change(at(t0, 120));
        assert!(!s.poll(at(t0, 150), false));
        assert!(!s.poll(at(t0, 250), false));
        assert!(s.poll(at(t0, 270), false));
    }

    #[test]
    fn fires_after_limit_even_when_held() {
        let t0 = Instant::now();
        let mut s = Settle::new();
        s.on_owner_change(t0);
        for ms in (50..30_000).step_by(50) {
            assert!(!s.poll(at(t0, ms), true), "held at {ms} ms");
        }
        assert!(s.poll(at(t0, 30_000), true));
        assert!(!s.is_pending());
    }

    #[test]
    fn limit_counts_from_the_first_notification() {
        let t0 = Instant::now();
        let mut s = Settle::new();
        s.on_owner_change(t0);
        for ms in (100..30_000).step_by(100) {
            s.on_owner_change(at(t0, ms));
            assert!(!s.poll(at(t0, ms), false), "notified at {ms} ms");
        }
        s.on_owner_change(at(t0, 30_000));
        assert!(s.poll(at(t0, 30_000), false));
    }

    #[test]
    fn fires_once_per_selection() {
        let t0 = Instant::now();
        let mut s = Settle::new();
        s.on_owner_change(t0);
        assert!(s.poll(at(t0, 150), false));
        assert!(!s.poll(at(t0, 300), false));
        assert!(!s.poll(at(t0, 31_000), true));
        assert!(!s.is_pending());
        s.on_owner_change(at(t0, 40_000));
        assert!(s.is_pending());
        assert!(!s.poll(at(t0, 40_100), false));
        assert!(s.poll(at(t0, 40_150), false));
    }
}
