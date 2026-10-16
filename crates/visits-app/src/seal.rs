//! Sealing: turning the journal of a session that has ended into a session
//! report in the queue, `<dir>/queue/session-<sid>.json`, beside its retry
//! companion `<dir>/queue/session-<sid>.retry`, which is
//! `{"first": <now>, "attempts": 0, "next": <now>}` so the queue never reads a
//! report's file time. Both are written through a temporary file and a rename, the
//! companion first, so the queue never sees half a report.
//!
//! The journal is read as the `journal` module writes it: the complete lines
//! only (a last line without its `\n` was cut by a crash), each line that does
//! not parse as JSON skipped wherever it is, the first start line giving the
//! session's start and envelope, every event line an event in order, and the
//! last mark line the focus totals and the rejected and dropped counts (a mark
//! without `dropped`, from an older build, dropped none). A journal with no
//! start line, or whose envelope cannot be read, cannot be reported and is
//! deleted.
//!
//! The report is also copied to `<dir>/last-session.json`, overwritten by each
//! seal, so `view` can show the session sealed last after the queued report
//! has been sent; `seal_stale` seals oldest first, so the newest one stays.
//!
//! Two crash windows remain, both harmless. A crash between the report's rename
//! and the journal's removal leaves the journal to be sealed again, so the
//! collector may see the same `sid` twice. A crash between the two renames
//! leaves a `.retry` with no `.json`, which the queue's drain removes.

use crate::queue::modified_before;
use crate::report::{End, Envelope, Event, Session};
use crate::retry::{write_whole, Retry};
use crate::state::{day, State};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use visits_core::app::{is_hex, MAX_COUNT, MAX_DAYS, MAX_SECS};

/// The copy of the session report sealed last, beside the queue; `view` shows it.
pub(crate) const LAST: &str = "last-session.json";

/// What a journal says about its session.
struct Recorded {
    start: u64,
    env: Envelope,
    ev: Vec<Event>,
    focus: BTreeMap<String, u64>,
    rejected: u64,
    /// The events the journal admitted past the 200th and did not write.
    dropped: u64,
    /// The greatest `t` of any event or mark: the last time the journal saw.
    last_t: u64,
}

/// Reads a journal's complete lines; `None` when it has no readable start line.
fn read(bytes: &[u8]) -> Option<Recorded> {
    let complete = &bytes[..bytes.iter().rposition(|&b| b == b'\n')?];
    let mut head: Option<(u64, Envelope)> = None;
    let mut ev = Vec::new();
    let mut focus = BTreeMap::new();
    let mut rejected = 0;
    let mut dropped = 0;
    let mut last_t = 0;
    for line in complete.split(|&b| b == b'\n') {
        let Ok(v) = serde_json::from_slice::<Value>(line) else { continue };
        if let Some(start) = v.get("start") {
            // Only the first start line counts; `Journal::open` writes exactly one.
            if head.is_none() {
                let env = v.get("env").and_then(Value::as_object).and_then(Envelope::from_map);
                head = start.as_u64().zip(env);
            }
        } else if let Some(e) = v.get("ev").and_then(Event::from_value) {
            last_t = last_t.max(e.t);
            ev.push(e);
        } else if let Some((t, f, r, d)) = v.get("mark").and_then(mark) {
            last_t = last_t.max(t);
            focus = f;
            rejected = r;
            dropped = d;
        }
    }
    let (start, env) = head?;
    Some(Recorded { start, env, ev, focus, rejected, dropped, last_t })
}

/// Reads a mark line's `t`, focus totals, rejected count and dropped count
/// (`0` when the line has none).
fn mark(v: &Value) -> Option<(u64, BTreeMap<String, u64>, u64, u64)> {
    let t = v.get("t")?.as_u64()?;
    let focus = v.get("focus")?.as_object()?.iter().map(|(k, secs)| Some((k.clone(), secs.as_u64()?))).collect::<Option<_>>()?;
    let rejected = v.get("rejected")?.as_u64()?;
    let dropped = match v.get("dropped") {
        None => 0,
        Some(d) => d.as_u64()?,
    };
    Some((t, focus, rejected, dropped))
}

/// Removes a file, counting one already gone as removed: another process
/// sealing the same journal may have got there first.
fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Seals `<dir>/journal/<sid>.jsonl` into `<dir>/queue/session-<sid>.json` and
/// deletes the journal, returning the report's path. The same bytes are
/// written to `<dir>/last-session.json`, replacing the previous session's.
///
/// `len` is `now` less the start for `Quit`, and for `Lost` and `Unknown` the
/// last time the journal saw, since the process may have gone long before
/// anyone noticed. `iw` is the week of `state.first_launch_day`, which the
/// first seal sets to the day this session started; `rd` is the days since
/// `state.last_report_day`, `-1` without one, and that day becomes today once
/// the report is queued. The state is changed in memory only: the caller saves
/// it, through `State::update`.
///
/// A session is kept within the wire's maxima rather than refused whole: `len`
/// and each focus total are capped at 31 days, an event later than that is
/// dropped and counted in `ev_dropped`, the counts are capped at 1,000,000 and
/// `rd` at 100,000 days.
///
/// `Ok(None)` when there is nothing to queue: no journal for `sid`, a journal
/// without a start line (deleted), or a session the wire would refuse, such as
/// one whose journal names a channel outside the name pattern (deleted and
/// counted in `state.rejected`). A source build's session is queued like any
/// other; `mode()` keeps it from uploading and the next `drain` that is not
/// under `log` deletes it. A `sid` that is not 16 to 32 lowercase hex
/// characters is `InvalidInput`.
pub fn seal(dir: &Path, sid: &str, end: End, now: u64, state: &mut State) -> io::Result<Option<PathBuf>> {
    if !is_hex(sid, 16, 32) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a session id is 16 to 32 lowercase hex characters"));
    }
    let journal = dir.join("journal").join(format!("{sid}.jsonl"));
    let bytes = match fs::read(&journal) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let Some(j) = read(&bytes) else {
        remove_if_present(&journal)?;
        return Ok(None);
    };

    let first = *state.first_launch_day.get_or_insert(day(j.start));
    let today = day(now);
    // A clock set back must not give a negative count of days.
    let rd = state.last_report_day.map_or(-1, |last| (today - last).clamp(0, MAX_DAYS as i64));
    let len = match end {
        End::Quit => now.saturating_sub(j.start),
        End::Lost | End::Unknown => j.last_t,
    };
    let mut session = Session {
        env: j.env,
        sid: sid.to_string(),
        len: len.min(MAX_SECS),
        end,
        iw: first - first.rem_euclid(7),
        rd,
        focus: j.focus.into_iter().map(|(k, secs)| (k, secs.min(MAX_SECS))).collect(),
        ev: Vec::new(),
        ev_dropped: j.dropped,
        ev_rejected: j.rejected.min(MAX_COUNT),
    };
    for e in j.ev {
        if e.t > MAX_SECS {
            session.ev_dropped += 1;
        } else {
            session.push(e);
        }
    }
    session.ev_dropped = session.ev_dropped.min(MAX_COUNT);
    let Ok(report) = session.to_bytes() else {
        state.rejected += 1;
        remove_if_present(&journal)?;
        return Ok(None);
    };

    let queue = dir.join("queue");
    fs::create_dir_all(&queue)?;
    let path = queue.join(format!("session-{sid}.json"));
    Retry::fresh(now).write(&path)?;
    write_whole(&path, &report)?;
    // A copy for `view`, which outlives the queued report once it is sent.
    write_whole(&dir.join(LAST), &report)?;
    state.last_report_day = Some(today);
    remove_if_present(&journal)?;
    Ok(Some(path))
}

/// Seals as `Unknown` every journal last seen (its start plus its greatest `t`)
/// before `now - older_than_secs`, oldest first so the first launch is the
/// earliest one, and returns the reports queued.
///
/// A journal with no start line has no time of its own, so its file's
/// modification time stands in: it is deleted, as `seal` deletes it, only when
/// that time is before the same cutoff, and left alone otherwise, since another
/// instance may be between creating it and writing its first line. A file that
/// cannot be read, or that is not `<sid>.jsonl`, is left alone.
pub fn seal_stale(dir: &Path, now: u64, older_than_secs: u64, state: &mut State) -> io::Result<Vec<PathBuf>> {
    seal_stale_except(dir, now, older_than_secs, &[], state)
}

/// `seal_stale`, leaving alone every journal whose `sid` is in `except`: a
/// program's resident part skips the windows still connected, since a sleeping
/// machine makes a live journal look stale.
pub fn seal_stale_except(dir: &Path, now: u64, older_than_secs: u64, except: &[&str], state: &mut State) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir.join("journal")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let cutoff = now.saturating_sub(older_than_secs);
    let mut stale = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(sid) = name.to_str().and_then(|n| n.strip_suffix(".jsonl")) else { continue };
        if !is_hex(sid, 16, 32) || except.contains(&sid) {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path()) else { continue };
        match read(&bytes) {
            Some(j) if j.start.saturating_add(j.last_t) < cutoff => stale.push((j.start, sid.to_string())),
            Some(_) => {}
            None if modified_before(&entry.path(), cutoff) => stale.push((0, sid.to_string())),
            None => {}
        }
    }
    stale.sort();
    let mut sealed = Vec::new();
    for (_, sid) in stale {
        if let Some(path) = seal(dir, &sid, End::Unknown, now, state)? {
            sealed.push(path);
        }
    }
    Ok(sealed)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_journal_already_removed_counts_as_removed() {
        let dir = std::env::temp_dir().join(format!("visits-app-seal-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gone.jsonl");
        fs::write(&path, b"x").unwrap();
        remove_if_present(&path).unwrap();
        assert!(!path.exists());
        remove_if_present(&path).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }
}
