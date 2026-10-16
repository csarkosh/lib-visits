//! The client half of lib-visits for an installed program: the typed reports of
//! the wire spec's section 6 and the consent mode. The crate is pure: it opens
//! no connection, reads no clock and starts no thread; the program supplies the
//! time and the transport.
//!
//! A report is a plain struct with public fields. `Session::to_bytes`,
//! `Crash::to_bytes` and `Check::to_query` build the wire form by hand and run
//! it through `visits_core::app`, the same code the collector validates with,
//! so the program can never write what the collector would refuse, and the
//! bytes returned are the bytes the program sends and shows the person.
//!
//! This departs from the design's 4.1, which has constructors that take the
//! enums and refuse the rest: the fields are public and the check happens once,
//! in `to_bytes` or `to_query`, against the wire schema itself. The wire check
//! is the same; only the point where a bad value is caught moves.
//!
//! `mode` turns the person's settings, the environment and the build's channel
//! into what may be recorded and sent. Its default is `Local`: recorded on the
//! person's disk, nothing sent until they say so.

pub mod mode;
pub mod report;

pub use mode::{mode, Crashes, Mode, Settings, Usage};
pub use report::{Arch, Channel, Check, Crash, CrashKind, DValue, End, Envelope, Event, Memory, Os, Session, Uptime};
pub use visits_core::Invalid;
