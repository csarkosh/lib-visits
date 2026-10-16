//! The client half of lib-visits for an installed program: the typed reports of
//! the wire spec's section 6, the consent mode, the journal, the queue, the
//! daily update check and the view of it all. The crate is pure: it opens
//! no connection, reads no clock and starts no thread; the program supplies the
//! time and the transport.
//!
//! A report is a plain struct with public fields. `Session::to_bytes`,
//! `Crash::to_bytes` and `Check::to_query` build the wire form by hand and run
//! it through `visits_core::app`, the same code the collector validates with,
//! so the program can never write what the collector would refuse for its
//! shape, and the bytes returned are the bytes the program sends and shows the
//! person. A source build's reports pass this check (the collector's `403` for
//! `ch: source`, `mode()` and `Queue::drain`, which deletes a queued source
//! report, are its guards), so it records and seals like any other and never
//! uploads.
//!
//! This departs from the design's 4.1, which has constructors that take the
//! enums and refuse the rest: the fields are public and the check happens once,
//! in `to_bytes` or `to_query`, against the wire schema itself. The wire check
//! is the same; only the point where a bad value is caught moves.
//!
//! `Journal` is what the program appends to while it runs, one line per event
//! the program's `Allowlist` admits, at most 200 a session; its line shapes are
//! in the `journal` module. The allowlist is a TOML file the program ships, one
//! table per event name, one key per permitted `d` key, and per key either the
//! list of values it may take, `"int"` or `"bool"`; and a `[focus]` table
//! listing the surfaces focus time may be counted against (a file without one
//! allows no surface):
//!
//! ```toml
//! [events.pane_open]
//! kind = ["terminal", "viewer", "browser"]
//! how = ["palette", "key", "click"]
//! [events.window_open]
//! [focus]
//! surfaces = ["terminal", "viewer", "browser"]
//! ```
//!
//! `Allowlist::from_toml` refuses a file that lists what the wire would refuse:
//! a name, key or surface outside `^[a-z][a-z0-9_]{0,31}$`, more than 6 keys
//! for one event, more than 16 surfaces, or a value outside
//! `^[A-Za-z0-9_.:/-]*$` or longer than 64.
//!
//! `new_id` makes the `sid` of a session and the `inc` of a crash: 32 hex
//! characters of randomness, and the only source the program should use.
//!
//! `seal` turns the journal of a session that has ended into a session report
//! in the queue, and `seal_stale` does so at launch for the journals a lost
//! process left behind. Both change a `State`, the small `state.toml` that
//! keeps the days the reports count from, and the crate never writes it on its
//! own. The program and its resident part may share the file, so the program
//! loads it right before each `seal`, `drain` or `checked` call and saves it
//! right after; `State::update` does both around one call.
//!
//! `Queue` holds the reports waiting to leave, each with a `.retry` companion
//! that says when it was queued, how often it failed and when it is next due.
//! `Queue::enqueue_crash` queues a crash the person agreed to send, and
//! `Queue::drain` is the only place the crate calls the program's `Transport`:
//! it posts what the mode lets leave, reschedules a failure on a backoff of 15
//! minutes, an hour, four hours, then daily, deletes what the collector
//! accepted or refused, and drops what has waited more than seven days. It
//! counts sent and refused reports in a `State` the program saves after it.
//!
//! `mode` turns the person's settings, the environment and the build's channel
//! into what may be recorded and sent. Its default is `Local`: recorded on the
//! person's disk, nothing sent until they say so. `Off` records nothing, so the
//! program opens no journal; a crash is sent only while crashes are not
//! `Never`, so withdrawing consent also stops the crashes already queued.
//!
//! `Check::next` says whether the update fetch the program is about to make is
//! the day's check: `Some` once a UTC day, with `rd` the days since the last
//! one (`-1` the first time), and never for a source build. The program sends
//! that one fetch through the collector with `Check::to_query` as its query,
//! the rest of the day's straight to its update source, and records the check
//! with `State::checked` whatever it answered. The check needs no consent: the
//! person installed a self-updating program.
//!
//! `view` reads the folder for the program's `telemetry view` command: the
//! mode, the state, each queued report with its failed attempts and next time,
//! the session sealed last (a copy `seal` keeps beside the queue, there even
//! after the report was sent) and the journals present; `payload_of` gives the
//! bytes of any one of them, for a "Show report" button.
//!
//! The program's loop, in its own threads and timers, with `dir` its folder
//! for the crate and `now()` the unix seconds. The state is loaded just before
//! each call that changes it and saved just after, through `State::update`.
//!
//! ```no_run
//! # use std::path::Path;
//! # use visits_app::*;
//! # struct Http;
//! # impl Transport for Http { fn post(&self, _: &str, _: &str, _: &[u8]) -> Result<u16, String> { Ok(204) } }
//! # fn now() -> u64 { 0 }
//! # fn fetch(_: &str) {}
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let (dir, http, ua) = (Path::new("/tmp/arkos"), Http, "arkos/0.4.2");
//! # let env = Envelope { app: "arkos".into(), ver: "0.4.2".into(), bin: "app".into(), os: Os::Macos, osv: 15, arch: Arch::Arm64, ch: Channel::Release };
//! # let allow = Allowlist::from_toml("[events.window_open]\n[focus]\nsurfaces = [\"editor\"]")?;
//! # let settings = Settings::default();
//! # let (collector, updates) = ("https://visits.example.com/a", "https://example.com/arkos/latest.json");
//! // `interactive` is true when the program was installed for a person, not run
//! // by a script; a desktop app's resident helper passes true too. `CI` and
//! // `DO_NOT_TRACK` are the environment's say, and `mode` reads them itself.
//! let mode = mode(&settings, &|k| std::env::var(k).ok(), "ARKOS_TELEMETRY", &env.ch, true);
//! // Usage is recorded only when it is not `Off`.
//! if mode.usage != Usage::Off {
//!     // At launch, seal what a lost process left over a day ago and open this session's journal.
//!     State::update(dir, |s| seal_stale(dir, now(), 86_400, s))??;
//!     let sid = new_id();
//!     let mut journal = Journal::open(dir, &sid, &env, allow, now())?;
//!     // Each minute, the focus totals are flushed.
//!     journal.focus("editor", 60);
//!     journal.mark(now())?;
//!     // When the process ends, its journal becomes a report in the queue.
//!     State::update(dir, |s| seal(dir, &sid, End::Quit, now(), s))??;
//! }
//! // Every 15 minutes, the queue posts what the mode lets leave.
//! State::update(dir, |s| Queue::new(dir).drain(&http, collector, ua, &mode, now(), s))?;
//! // Before an update fetch, the day's first goes through the collector.
//! match Check::next(&env, &State::load(dir), now()) {
//!     Some(check) => {
//!         fetch(&format!("{collector}/check?{}", check.to_query()?));
//!         State::update(dir, |s| s.checked(now()))?;
//!     }
//!     None => fetch(updates),
//! }
//! # Ok(()) }
//! ```

pub mod allowlist;
mod check;
mod id;
pub mod journal;
pub mod mode;
pub mod queue;
pub mod report;
mod retry;
pub mod seal;
pub mod state;
pub mod view;

pub use allowlist::{Allowlist, Rejected};
pub use id::new_id;
pub use journal::Journal;
pub use mode::{mode, Crashes, Mode, Settings, Usage};
pub use queue::{Outcome, Queue, Transport};
pub use report::{Arch, Channel, Check, Crash, CrashKind, DValue, End, Envelope, Event, Memory, Os, Session, Uptime};
pub use seal::{seal, seal_stale};
pub use state::State;
pub use view::{payload_of, view, View};
pub use visits_core::Invalid;
