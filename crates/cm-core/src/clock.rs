//! The history code reads the wall clock (`+new Date`) to decide whether `+`-origin
//! events merge. Tests and the differential oracle need a controllable clock, so the
//! time source can be overridden per thread.

use std::cell::RefCell;

thread_local! {
    static OVERRIDE: RefCell<Option<Box<dyn Fn() -> f64>>> = const { RefCell::new(None) };
}

/// Milliseconds since the epoch, like `+new Date`.
pub fn now() -> f64 {
    let o = OVERRIDE.with(|c| c.borrow().as_ref().map(|f| f()));
    match o {
        Some(t) => t,
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0),
    }
}

/// Replace the clock for the current thread (`None` restores the system clock).
pub fn set_override(f: Option<Box<dyn Fn() -> f64>>) {
    OVERRIDE.with(|c| *c.borrow_mut() = f);
}

/// Convenience: a manually advanced clock shared through an `Rc<Cell<f64>>`.
pub fn manual(start: f64) -> std::rc::Rc<std::cell::Cell<f64>> {
    let cell = std::rc::Rc::new(std::cell::Cell::new(start));
    let c = cell.clone();
    set_override(Some(Box::new(move || c.get())));
    cell
}
