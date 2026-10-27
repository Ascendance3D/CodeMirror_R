//! The history code reads the wall clock (`+new Date`) to decide whether `+`-origin
//! events merge. Tests and the differential oracle need a controllable clock, and a
//! browser host has to supply one, so the time source can be replaced per thread.
//!
//! Where the time comes from:
//!
//! 1. A host clock installed with [`set_override`] or [`set_host_clock`] (per thread;
//!    a wasm module instance has one thread, so one call covers it).
//! 2. Otherwise the system clock (`SystemTime`), on every target that has one.
//! 3. On `wasm32-unknown-unknown` there is no system clock (`SystemTime::now` traps
//!    there), so without a host clock the time is *unknown*.
//!
//! Unknown time is never a trap and never a made-up value. [`try_now`] returns `None`,
//! [`now`] returns NaN, and [`missing_reads`] counts it. Every comparison with NaN
//! is false, so the history treats an event at an unknown time as outside the merge
//! window (`historyEventDelay`): such `+`-origin changes and selections become
//! separate undo events. Undo steps get finer, never coarser. A browser host should
//! install `Date.now` before it creates an editor; [`is_available`] checks.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

type HostClock = Rc<dyn Fn() -> Option<f64>>;

thread_local! {
    static OVERRIDE: RefCell<Option<HostClock>> = const { RefCell::new(None) };
    static MISSING_READS: Cell<u64> = const { Cell::new(0) };
}

/// Where [`now`] gets its time on this thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockStatus {
    /// A host clock is installed.
    Host,
    /// No host clock; the system clock is used.
    System,
    /// No time source on this target and no host clock.
    Unavailable,
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn system_now() -> Option<f64> {
    Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0),
    )
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn system_now() -> Option<f64> {
    None
}

const HAS_SYSTEM_CLOCK: bool = !cfg!(all(target_arch = "wasm32", target_os = "unknown"));

/// Milliseconds since the epoch, like `+new Date`, or `None` when the time is
/// unknown (no system clock and no host clock, or the host clock returned `None` or
/// a non-finite value). Each `None` counts as a missing read ([`missing_reads`]).
pub fn try_now() -> Option<f64> {
    // Clone the host clock out of the cell, so it may call back into this module
    // (even replace itself) without a re-entrant borrow.
    let host = OVERRIDE.with(|c| c.borrow().clone());
    let t = match host {
        Some(f) => f().filter(|t| t.is_finite()),
        None => system_now(),
    };
    if t.is_none() {
        MISSING_READS.with(|m| m.set(m.get() + 1));
    }
    t
}

/// Milliseconds since the epoch, like `+new Date`; NaN when the time is unknown
/// (see [`try_now`] and the module documentation).
pub fn now() -> f64 {
    try_now().unwrap_or(f64::NAN)
}

/// Replace the clock for the current thread (`None` restores the default).
pub fn set_override(f: Option<Box<dyn Fn() -> f64>>) {
    set_host_clock(f.map(|f| -> Box<dyn Fn() -> Option<f64>> { Box::new(move || Some(f())) }));
}

/// Install a host clock that may fail (`None`: the time is unknown, for example
/// after the host's time source went away), or remove it with `None`. The clock is
/// owned by this thread until it is replaced or the thread ends.
pub fn set_host_clock(f: Option<Box<dyn Fn() -> Option<f64>>>) {
    let f: Option<HostClock> = f.map(Rc::from);
    // Drop the old clock after the borrow ends: its destructor may use this module.
    let old = OVERRIDE.with(|c| c.replace(f));
    drop(old);
}

/// Where the time comes from on this thread.
pub fn status() -> ClockStatus {
    if OVERRIDE.with(|c| c.borrow().is_some()) {
        ClockStatus::Host
    } else if HAS_SYSTEM_CLOCK {
        ClockStatus::System
    } else {
        ClockStatus::Unavailable
    }
}

/// Whether a time source exists on this thread (a host clock, or the system clock).
/// A host clock can still return `None` at a given read.
pub fn is_available() -> bool {
    status() != ClockStatus::Unavailable
}

/// How many reads on this thread found no time.
pub fn missing_reads() -> u64 {
    MISSING_READS.with(|m| m.get())
}

/// Convenience: a manually advanced clock shared through an `Rc<Cell<f64>>`.
pub fn manual(start: f64) -> std::rc::Rc<std::cell::Cell<f64>> {
    let cell = std::rc::Rc::new(std::cell::Cell::new(start));
    let c = cell.clone();
    set_override(Some(Box::new(move || c.get())));
    cell
}
