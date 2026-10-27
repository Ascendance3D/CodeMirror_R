//! The browser clock: `Date.now()` (or a host function) behind
//! `cm_core::clock::set_host_clock`.
//!
//! The shared clock contract (L-B) is unchanged: a host clock that throws, returns
//! a non-number or a non-finite number makes the time unknown for that read (NaN,
//! counted by `missing_reads`), never a trap. The clock belongs to the module
//! instance (one thread), so it outlives the editors and is replaced, not stacked.

use cm_core::clock::{self, ClockStatus};
use wasm_bindgen::JsValue;

/// Install `f` as the time source, or `Date.now()` when `f` is `None`.
pub fn install(f: Option<js_sys::Function>) {
    let read: Box<dyn Fn() -> Option<f64>> = match f {
        Some(f) => Box::new(move || f.call0(&JsValue::NULL).ok().and_then(|v| v.as_f64())),
        None => Box::new(|| Some(js_sys::Date::now())),
    };
    clock::set_host_clock(Some(read));
}

/// Remove the host clock: on wasm32-unknown-unknown the time becomes unknown.
pub fn remove() {
    clock::set_host_clock(None);
}

/// `Date.now()` unless a host clock is already installed (the normal browser
/// path: an editor attached to a page always has a time source).
pub fn ensure_installed() {
    if clock::status() != ClockStatus::Host {
        install(None);
    }
}

/// 0 host, 1 system, 2 unavailable (the numbering of the L-B probe).
pub fn status_code() -> u32 {
    match clock::status() {
        ClockStatus::Host => 0,
        ClockStatus::System => 1,
        ClockStatus::Unavailable => 2,
    }
}
