//! The pure half of lib-visits: no I/O, no HTTP type. Everything here is a
//! function of its arguments, tested against `spec/fixtures/`.

pub mod event;
pub mod validate;

pub use event::{Decoration, Event};
pub use validate::{validate_json, validate_query, Invalid, MAX_EVENTS};
