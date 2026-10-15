//! The pure half of lib-visits: no I/O, no HTTP type. Everything here is a
//! function of its arguments, tested against `spec/fixtures/`.

pub mod classify;
pub mod clock;
pub mod decorate;
pub mod event;
pub mod validate;
pub mod visit;

pub use classify::{browser_of, host_key, is_crawler, os_of, split_referrer};
pub use clock::{rfc3339, rfc3339_ms, ymd};
pub use decorate::{decorate, RequestFacts};
pub use event::{Decoration, Event};
pub use validate::{validate_json, validate_query, Invalid, MAX_EVENTS};
pub use visit::visit_hash;
