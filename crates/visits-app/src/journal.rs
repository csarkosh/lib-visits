//! The journal a running program appends to: `<dir>/journal/<sid>.jsonl`, one
//! JSON object per line, each line ending in `\n`. Sealing (a later step) reads
//! it back, so its shape is fixed here:
//!
//! - the first line, written by `Journal::open`, is
//!   `{"start": <unix seconds>, "env": {"app", "ver", "bin", "os", "osv", "arch", "ch"}}`,
//!   the envelope spelled as a report spells it, less `v` and `k`, so a session
//!   sealed after the program was upgraded keeps the version that ran;
//! - each event the allowlist admits is a line `{"ev": {"n", "t", "d"}}`, spelled
//!   as in a report (`d` left out when empty), its `t` the seconds since `start`;
//! - each `Journal::mark` is a line
//!   `{"mark": {"t": <seconds since start>, "focus": {<surface>: <seconds>}, "rejected": <count>}}`,
//!   the running totals since `start`, so the last mark replaces every earlier one.
//!
//! Every line is written with one `write_all` of the whole line on a file opened
//! for appending, so a crash leaves at most one cut line, the last, which a
//! reader skips because it has no `\n`.

use crate::allowlist::Allowlist;
use crate::report::{Envelope, Event};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use visits_core::app::{is_hex, is_name};

/// The most `focus` keys a session report carries (section 6.4).
const MAX_FOCUS: usize = 16;

/// An open journal: the file, the session's start, the allowlist, and the focus
/// seconds and rejected events counted since the start.
#[derive(Debug)]
pub struct Journal {
    file: File,
    start: u64,
    allow: Allowlist,
    focus: BTreeMap<String, u64>,
    rejected: u64,
}

impl Journal {
    /// Creates `<dir>/journal/<sid>.jsonl` and writes its start line. A `sid`
    /// that is not 16 to 32 lowercase hex characters is `InvalidInput`, so it
    /// can never name a path outside the folder; a journal that already exists
    /// for this `sid` is `AlreadyExists`, so a file has exactly one start line.
    pub fn open(dir: &Path, sid: &str, env: &Envelope, allow: Allowlist, now: u64) -> io::Result<Journal> {
        if !is_hex(sid, 16, 32) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a session id is 16 to 32 lowercase hex characters"));
        }
        let folder = dir.join("journal");
        fs::create_dir_all(&folder)?;
        let file = OpenOptions::new().append(true).create_new(true).open(folder.join(format!("{sid}.jsonl")))?;
        let mut journal = Journal { file, start: now, allow, focus: BTreeMap::new(), rejected: 0 };
        let mut env = env.to_map("session");
        env.remove("v");
        env.remove("k");
        journal.line(&json!({ "start": now, "env": env }))?;
        Ok(journal)
    }

    /// Records an event at `now`: its `t` is set to the seconds since the start,
    /// and it is appended if the allowlist admits it and counted as rejected if
    /// not. A failed write loses the event without disturbing the program.
    pub fn event(&mut self, mut ev: Event, now: u64) {
        ev.t = now.saturating_sub(self.start);
        if self.allow.check(&ev).is_err() {
            self.rejected += 1;
            return;
        }
        let _ = self.line(&json!({ "ev": ev.to_value() }));
    }

    /// Adds `secs` seconds in the foreground to `surface`, in memory until the
    /// next `mark`. A surface that is not a name, or a seventeenth surface, is
    /// ignored, so one bad call cannot make the whole session unsendable.
    pub fn focus(&mut self, surface: &str, secs: u64) {
        if !is_name(surface) || (self.focus.len() >= MAX_FOCUS && !self.focus.contains_key(surface)) {
            return;
        }
        *self.focus.entry(surface.to_string()).or_default() += secs;
    }

    /// Appends a mark line with the time since the start, the focus totals and
    /// the rejected count. The program calls it at most once a minute.
    pub fn mark(&mut self, now: u64) -> io::Result<()> {
        let focus: Map<String, Value> = self.focus.iter().map(|(k, v)| (k.clone(), Value::from(*v))).collect();
        let t = now.saturating_sub(self.start);
        self.line(&json!({ "mark": { "t": t, "focus": focus, "rejected": self.rejected } }))
    }

    /// Writes one line with a single `write_all`.
    fn line(&mut self, v: &Value) -> io::Result<()> {
        // Serializing a map of strings, numbers and booleans cannot fail.
        let mut bytes = serde_json::to_vec(v).map_err(io::Error::other)?;
        bytes.push(b'\n');
        self.file.write_all(&bytes)
    }
}
