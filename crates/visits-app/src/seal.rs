//! Sealing: turning the journal of a session that has ended into a session
//! report in the queue, `<dir>/queue/session-<sid>.json`, beside its retry
//! companion `<dir>/queue/session-<sid>.retry`, which is
//! `{"first": <now>, "attempts": 0, "next": <now>}` so the queue never reads a
//! file's time. Both are written through a temporary file and a rename, the
//! companion first, so the queue never sees half a report.
//!
//! The journal is read as the `journal` module writes it: the complete lines
//! only (a last line without its `\n` was cut by a crash), each line that does
//! not parse as JSON skipped wherever it is, the first start line giving the
//! session's start and envelope, every event line an event in order, and the
//! last mark line the focus totals and the rejected count. A journal with no
//! start line, or whose envelope cannot be read, cannot be reported and is
//! deleted.

use crate::report::{End, Envelope, Event, Session};
use crate::state::State;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use visits_core::app::is_hex;

/// Seconds in a UTC day.
const DAY: u64 = 86_400;

/// What a journal says about its session.
struct Journal {
    start: u64,
    env: Envelope,
    ev: Vec<Event>,
    focus: BTreeMap<String, u64>,
    rejected: u64,
    /// The greatest `t` of any event or mark: the last time the journal saw.
    last_t: u64,
}

/// Reads a journal's complete lines; `None` when it has no readable start line.
fn read(bytes: &[u8]) -> Option<Journal> {
    let complete = &bytes[..bytes.iter().rposition(|&b| b == b'\n')?];
    let mut head: Option<(u64, Envelope)> = None;
    let mut ev = Vec::new();
    let mut focus = BTreeMap::new();
    let mut rejected = 0;
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
        } else if let Some(m) = v.get("mark").and_then(mark) {
            last_t = last_t.max(m.0);
            focus = m.1;
            rejected = m.2;
        }
    }
    let (start, env) = head?;
    Some(Journal { start, env, ev, focus, rejected, last_t })
}

/// Reads a mark line's `t`, focus totals and rejected count.
fn mark(v: &Value) -> Option<(u64, BTreeMap<String, u64>, u64)> {
    let t = v.get("t")?.as_u64()?;
    let focus = v.get("focus")?.as_object()?.iter().map(|(k, secs)| Some((k.clone(), secs.as_u64()?))).collect::<Option<_>>()?;
    let rejected = v.get("rejected")?.as_u64()?;
    Some((t, focus, rejected))
}

/// Writes a file whole through a temporary file and a rename.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

/// Seals `<dir>/journal/<sid>.jsonl` into `<dir>/queue/session-<sid>.json` and
/// deletes the journal, returning the report's path.
///
/// `len` is `now` less the start for `Quit`, and for `Lost` and `Unknown` the
/// last time the journal saw, since the process may have gone long before
/// anyone noticed. `iw` is the week of `state.first_launch_day`, which the
/// first seal sets to the day this session started; `rd` is the days since
/// `state.last_report_day`, `-1` without one, and that day becomes today once
/// the report is queued. The state is changed in memory only: the caller saves
/// it.
///
/// `Ok(None)` when there is nothing to queue: no journal for `sid`, a journal
/// without a start line (deleted), or a session the wire would refuse, such as
/// one from a source build (deleted and counted in `state.rejected`). A `sid`
/// that is not 16 to 32 lowercase hex characters is `InvalidInput`.
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
        fs::remove_file(&journal)?;
        return Ok(None);
    };

    let first = *state.first_launch_day.get_or_insert((j.start / DAY) as i64);
    let today = (now / DAY) as i64;
    // A clock set back must not give a negative count of days.
    let rd = state.last_report_day.map_or(-1, |last| (today - last).max(0));
    let len = match end {
        End::Quit => now.saturating_sub(j.start),
        End::Lost | End::Unknown => j.last_t,
    };
    let mut session = Session {
        env: j.env,
        sid: sid.to_string(),
        len,
        end,
        iw: first - first.rem_euclid(7),
        rd,
        focus: j.focus,
        ev: Vec::new(),
        ev_dropped: 0,
        ev_rejected: j.rejected,
    };
    for e in j.ev {
        session.push(e);
    }
    let Ok(report) = session.to_bytes() else {
        state.rejected += 1;
        fs::remove_file(&journal)?;
        return Ok(None);
    };

    let queue = dir.join("queue");
    fs::create_dir_all(&queue)?;
    let path = queue.join(format!("session-{sid}.json"));
    let retry = json!({ "first": now, "attempts": 0, "next": now });
    write_whole(&path.with_extension("retry"), retry.to_string().as_bytes())?;
    write_whole(&path, &report)?;
    state.last_report_day = Some(today);
    fs::remove_file(&journal)?;
    Ok(Some(path))
}

/// Seals as `Unknown` every journal last seen (its start plus its greatest `t`)
/// before `now - older_than_secs`, oldest first so the first launch is the
/// earliest one, and returns the reports queued. A journal with no start line is
/// deleted as `seal` deletes it: `Journal::open` writes that line as soon as it
/// creates the file. A file in the folder that is not `<sid>.jsonl` is left alone.
pub fn seal_stale(dir: &Path, now: u64, older_than_secs: u64, state: &mut State) -> io::Result<Vec<PathBuf>> {
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
        if !is_hex(sid, 16, 32) {
            continue;
        }
        let bytes = fs::read(entry.path())?;
        match read(&bytes) {
            Some(j) if j.start.saturating_add(j.last_t) < cutoff => stale.push((j.start, sid.to_string())),
            Some(_) => {}
            None => stale.push((0, sid.to_string())),
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
