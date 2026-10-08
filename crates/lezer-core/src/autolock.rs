//! 15-minute inactivity auto-lock state machine (`AutoLockFsm`) with `DeferredLockPending`
//! mid-operation protection.
//!
//! Guarantees that if the 15-minute inactivity timer expires while a multi-minute
//! RISC Zero / RapidSnark ZK proof or critical transaction is in flight
//! (`active_critical_ops > 0`), the wallet transitions to `DeferredLockPending`
//! rather than wiping keys mid-proof, and immediately transitions to `Locked`
//! the instant the final critical operation completes.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Default inactivity timeout: 15 minutes (900 seconds).
pub const DEFAULT_AUTOLOCK_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Lock state of the wallet vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    Uninitialized,
    Locked,
    Unlocked,
    DeferredLockPending,
}

/// 15-minute inactivity finite state machine with critical-operation guard.
#[derive(Debug, Clone)]
pub struct AutoLockFsm {
    state: LockState,
    timeout: Duration,
    last_activity: Instant,
    active_critical_ops: usize,
}

impl Default for AutoLockFsm {
    fn default() -> Self {
        Self::new(LockState::Uninitialized, DEFAULT_AUTOLOCK_TIMEOUT)
    }
}

impl AutoLockFsm {
    pub fn new(initial_state: LockState, timeout: Duration) -> Self {
        Self {
            state: initial_state,
            timeout,
            last_activity: Instant::now(),
            active_critical_ops: 0,
        }
    }

    pub fn state(&self) -> LockState {
        self.state
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub fn active_critical_ops(&self) -> usize {
        self.active_critical_ops
    }

    /// Transition to `Locked` when a vault file exists on disk but has not been unlocked.
    pub fn mark_vault_present_locked(&mut self) {
        if self.state == LockState::Uninitialized {
            self.state = LockState::Locked;
        }
    }

    /// Transition to `Unlocked` and reset the inactivity timer.
    pub fn on_unlock(&mut self, now: Instant) {
        self.state = LockState::Unlocked;
        self.last_activity = now;
    }

    /// Record user/authenticated activity, resetting the 15-minute inactivity countdown.
    pub fn record_activity(&mut self, now: Instant) {
        if self.state == LockState::Unlocked {
            self.last_activity = now;
        }
    }

    /// Request an explicit lock (or inactivity timeout expiration).
    /// If `active_critical_ops > 0`, enters `DeferredLockPending` until ops finish.
    pub fn request_lock(&mut self) -> LockState {
        match self.state {
            LockState::Uninitialized => LockState::Uninitialized,
            LockState::Locked => LockState::Locked,
            LockState::Unlocked | LockState::DeferredLockPending => {
                if self.active_critical_ops > 0 {
                    self.state = LockState::DeferredLockPending;
                } else {
                    self.state = LockState::Locked;
                }
                self.state
            }
        }
    }

    /// Begin a critical operation (e.g. ZK proving or signing).
    pub fn begin_critical_op(&mut self, now: Instant) -> bool {
        if self.state != LockState::Unlocked {
            return false;
        }
        self.last_activity = now;
        self.active_critical_ops += 1;
        true
    }

    /// Complete a critical operation. If `DeferredLockPending` was set while proving,
    /// immediately locks the vault once `active_critical_ops` reaches 0.
    pub fn end_critical_op(&mut self, now: Instant) -> LockState {
        self.active_critical_ops = self.active_critical_ops.saturating_sub(1);
        if self.active_critical_ops == 0 && self.state == LockState::DeferredLockPending {
            self.state = LockState::Locked;
        } else if self.state == LockState::Unlocked {
            self.last_activity = now;
        }
        self.state
    }

    /// Poll the inactivity timer at `now`. Returns `true` if state transitioned to `Locked`
    /// (requiring the caller to zeroize in-memory keys).
    pub fn check_inactivity(&mut self, now: Instant) -> bool {
        if self.state == LockState::Unlocked
            && now.saturating_duration_since(self.last_activity) >= self.timeout
        {
            let next = self.request_lock();
            return next == LockState::Locked;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_15_minutes_of_inactivity() {
        let t0 = Instant::now();
        let mut fsm = AutoLockFsm::new(LockState::Uninitialized, DEFAULT_AUTOLOCK_TIMEOUT);
        fsm.on_unlock(t0);
        assert_eq!(fsm.state(), LockState::Unlocked);

        // At 14m59s, still unlocked
        assert!(!fsm.check_inactivity(t0 + Duration::from_secs(14 * 60 + 59)));
        assert_eq!(fsm.state(), LockState::Unlocked);

        // At 15m00s, transitions to Locked
        assert!(fsm.check_inactivity(t0 + Duration::from_secs(15 * 60)));
        assert_eq!(fsm.state(), LockState::Locked);
    }

    #[test]
    fn deferred_lock_protects_in_flight_proving_and_locks_on_completion() {
        let t0 = Instant::now();
        let mut fsm = AutoLockFsm::new(LockState::Uninitialized, DEFAULT_AUTOLOCK_TIMEOUT);
        fsm.on_unlock(t0);
        assert!(fsm.begin_critical_op(t0));

        // Timer expires at +16 minutes while proof is still running
        let t1 = t0 + Duration::from_secs(16 * 60);
        assert!(!fsm.check_inactivity(t1));
        assert_eq!(fsm.state(), LockState::DeferredLockPending);

        // As soon as the proof completes, vault immediately locks
        let final_state = fsm.end_critical_op(t1 + Duration::from_secs(30));
        assert_eq!(final_state, LockState::Locked);
    }
}
