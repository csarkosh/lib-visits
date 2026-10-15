//! The pure half of lib-visits: no I/O, no HTTP type. Everything here is a
//! function of its arguments, tested against `spec/fixtures/`.

pub mod event;
pub mod validate;

pub use event::{Decoration, Event};
pub use validate::{validate_json, validate_query, Invalid, MAX_EVENTS};
pub mod clock;
pub mod visit;

pub use clock::{rfc3339, ymd};
pub use visit::visit_hash;
