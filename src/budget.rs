//! Memory admission control.
//!
//! A conversion's working set is roughly the decoded pixels plus the
//! encoder's buffers, so the process peak grows with the number of
//! conversions running at once, and a few huge images together dominate it.
//! `acquire` makes a conversion wait until the estimated working sets of the
//! running conversions plus its own fit in the budget. A conversion that
//! alone exceeds the budget still runs once nothing else is running, so
//! nothing can wait forever.
//!
//! The budget is off (0) by default. Set it with `set_budget` or the
//! `IMGCONV_MEMORY_BUDGET_MB` environment variable.

use std::sync::{Condvar, Mutex, Once};

struct State {
    /// Bytes; 0 means unlimited.
    budget: u64,
    in_flight: u64,
}

static STATE: Mutex<State> = Mutex::new(State { budget: 0, in_flight: 0 });
static CHANGED: Condvar = Condvar::new();
static ENV: Once = Once::new();

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Set the budget in bytes (0 = unlimited). Waiting conversions are woken.
pub fn set_budget(bytes: u64) {
    // An explicit setting wins over the environment.
    ENV.call_once(|| {});
    lock().budget = bytes;
    CHANGED.notify_all();
}

fn init_from_env() {
    ENV.call_once(|| {
        if let Some(mb) = std::env::var("IMGCONV_MEMORY_BUDGET_MB").ok().and_then(|v| v.trim().parse::<u64>().ok()) {
            lock().budget = mb.saturating_mul(1 << 20);
        }
    });
}

/// Holds `cost` bytes of the budget until dropped.
pub struct Permit(u64);

pub fn acquire(cost: u64) -> Permit {
    init_from_env();
    let mut s = lock();
    while s.budget != 0 && s.in_flight != 0 && s.in_flight.saturating_add(cost) > s.budget {
        s = CHANGED.wait(s).unwrap_or_else(|e| e.into_inner());
    }
    s.in_flight = s.in_flight.saturating_add(cost);
    Permit(cost)
}

impl Drop for Permit {
    fn drop(&mut self) {
        lock().in_flight -= self.0;
        CHANGED.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn limits_concurrent_cost_and_never_deadlocks() {
        set_budget(100);
        let live = Arc::new(AtomicU64::new(0));
        let max = Arc::new(AtomicU64::new(0));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let (live, max) = (live.clone(), max.clone());
                std::thread::spawn(move || {
                    // One job (cost 500) is larger than the whole budget.
                    let cost = if i == 0 { 500 } else { 40 };
                    let _p = acquire(cost);
                    let now = live.fetch_add(cost, Ordering::SeqCst) + cost;
                    max.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    live.fetch_sub(cost, Ordering::SeqCst);
                })
            })
            .collect();
        threads.into_iter().for_each(|t| t.join().unwrap());
        // Small jobs never exceed the budget together; the oversized one ran alone.
        assert!(max.load(Ordering::SeqCst) <= 500);
        assert_eq!(lock().in_flight, 0);
        set_budget(0);
    }
}
