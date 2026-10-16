//! A queued report's companion, `<kind>-<id>.retry` beside `<kind>-<id>.json`:
//! `{"first": <secs>, "attempts": <n>, "next": <secs>}`, when the report was
//! queued, how many posts have failed and when it is next due. `seal` and the
//! queue both write it, through `Retry`, so its shape is spelled once.

use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::Path;

/// A report's companion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Retry {
    pub(crate) first: u64,
    pub(crate) attempts: u32,
    pub(crate) next: u64,
}

impl Retry {
    /// The companion of a report queued at `now`: due at once, never tried.
    pub(crate) fn fresh(now: u64) -> Retry {
        Retry { first: now, attempts: 0, next: now }
    }

    /// Reads the companion of the report at `report`; `None` when it is
    /// missing, cannot be read or does not hold the three integers.
    pub(crate) fn read(report: &Path) -> Option<Retry> {
        let bytes = fs::read(report.with_extension("retry")).ok()?;
        let v: Value = serde_json::from_slice(&bytes).ok()?;
        Some(Retry {
            first: v.get("first")?.as_u64()?,
            attempts: u32::try_from(v.get("attempts")?.as_u64()?).ok()?,
            next: v.get("next")?.as_u64()?,
        })
    }

    /// Writes the companion of the report at `report` whole.
    pub(crate) fn write(&self, report: &Path) -> io::Result<()> {
        let v = json!({ "first": self.first, "attempts": self.attempts, "next": self.next });
        write_whole(&report.with_extension("retry"), v.to_string().as_bytes())
    }
}

/// Writes a file whole through a temporary file beside it, `<name>.tmp`, and a
/// rename, so a reader sees the old file or the new one and never half of one.
pub(crate) fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}
