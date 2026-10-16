//! The crate's small state file, `<dir>/state.toml`: the days the reports are
//! counted from and the counts `view` shows. A day is a UTC day number, the days
//! since the epoch (`unix seconds / 86_400`).
//!
//! The crate never saves it on its own: `seal`, `drain` and `checked` change a
//! `State` in memory and the program saves it with `State::save` once after
//! each call, so one place owns the file.

use std::fs;
use std::io;
use std::path::Path;
use toml::{Table, Value};

/// The file's name inside the crate's folder.
const FILE: &str = "state.toml";

/// What the crate remembers between runs.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct State {
    /// The day of the first session sealed, which gives a report's `iw`.
    pub first_launch_day: Option<i64>,
    /// The day of the last session report queued, which gives its `rd`.
    pub last_report_day: Option<i64>,
    /// The day of the last update check, which gives a check's `rd`.
    pub last_check_day: Option<i64>,
    /// Reports refused: by the wire check when sealing, or by the collector.
    pub rejected: u64,
    /// Reports the collector accepted.
    pub sent: u64,
}

impl State {
    /// Reads `<dir>/state.toml`. A missing or unreadable file is the default
    /// state, and a key that is missing or not a fitting integer is its default,
    /// so a damaged file costs the counts and never the program.
    pub fn load(dir: &Path) -> State {
        let Ok(text) = fs::read_to_string(dir.join(FILE)) else { return State::default() };
        let Ok(table) = text.parse::<Table>() else { return State::default() };
        let int = |k: &str| table.get(k).and_then(Value::as_integer);
        let count = |k: &str| int(k).and_then(|n| u64::try_from(n).ok()).unwrap_or(0);
        State {
            first_launch_day: int("first_launch_day"),
            last_report_day: int("last_report_day"),
            last_check_day: int("last_check_day"),
            rejected: count("rejected"),
            sent: count("sent"),
        }
    }

    /// Writes `<dir>/state.toml` whole, through a temporary file renamed over
    /// it, so a crash leaves the old file or the new one and never half of one.
    /// A day that is `None` is left out of the file.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let mut text = String::new();
        for (key, day) in [("first_launch_day", self.first_launch_day), ("last_report_day", self.last_report_day), ("last_check_day", self.last_check_day)] {
            if let Some(day) = day {
                text.push_str(&format!("{key} = {day}\n"));
            }
        }
        // TOML integers are signed 64-bit, so a count is capped at `i64::MAX`.
        for (key, n) in [("rejected", self.rejected), ("sent", self.sent)] {
            text.push_str(&format!("{key} = {}\n", n.min(i64::MAX as u64)));
        }
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE}.tmp"));
        fs::write(&tmp, text)?;
        fs::rename(&tmp, dir.join(FILE))
    }
}
