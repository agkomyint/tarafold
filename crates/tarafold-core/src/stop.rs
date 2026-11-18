//! Cooperative early stop for long searches: Ctrl-C (`interrupt`) and `--time-budget`
//! (`set_deadline`). Equation search checks [`requested`] between epochs and keeps what it has
//! found. A stopped search depends on timing, so it is not reproducible; the report says so.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static DEADLINE_HIT: AtomicBool = AtomicBool::new(false);
static DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);

/// Ask running searches to stop (Ctrl-C).
pub fn interrupt() {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// Stop searches once `at` has passed.
pub fn set_deadline(at: Option<Instant>) {
    if let Ok(mut d) = DEADLINE.lock() {
        *d = at;
    }
}

/// Whether the user interrupted.
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Whether a search stopped because the time budget ran out.
pub fn deadline_hit() -> bool {
    DEADLINE_HIT.load(Ordering::SeqCst)
}

/// Whether searches should stop now.
pub fn requested() -> bool {
    if interrupted() {
        return true;
    }
    let past = DEADLINE
        .lock()
        .ok()
        .and_then(|d| *d)
        .is_some_and(|d| Instant::now() >= d);
    if past {
        DEADLINE_HIT.store(true, Ordering::SeqCst);
    }
    past
}

/// Clear all stop state (tests and long-lived servers).
pub fn reset() {
    INTERRUPTED.store(false, Ordering::SeqCst);
    DEADLINE_HIT.store(false, Ordering::SeqCst);
    set_deadline(None);
}
