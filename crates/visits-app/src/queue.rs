//! The queue of reports waiting to leave the machine, `<dir>/queue`, and the
//! one place the crate ever calls the program's `Transport`.
//!
//! A report is `queue/<kind>-<id>.json`, the exact bytes to send, beside its
//! companion `queue/<kind>-<id>.retry`, which is
//! `{"first": <secs>, "attempts": <n>, "next": <secs>}`: when the report was
//! queued, how many posts have failed, and when it is next due. `seal` and
//! `Queue::enqueue_crash` write both, the companion first, each through a
//! temporary file and a rename, so the queue never reads a report's file time and
//! never sees half a report. A report found without a companion (or with one
//! that does not parse) is due at once and is given a fresh one by the next
//! `drain`, so it ages like any other.
//!
//! A failed post is tried again 15 minutes later, then an hour, four hours and
//! a day later, then daily. Whatever its attempts, a report first queued more
//! than seven days ago is deleted, so the queue never grows without bound,
//! even under `Local`, where sessions are never sent.
//!
//! The seven days hold under `Local` too: a session sealed while usage was
//! `Local` waits in the queue, and if the person turns usage `On` within seven
//! days of its sealing, the next `drain` sends it. A program that wants usage
//! turned on to start clean calls `Queue::clear_sessions` at that moment.

use crate::mode::{Crashes, Mode};
use crate::report::Crash;
use crate::retry::{write_whole, Retry};
use crate::state::{State, DAY};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use visits_core::app::is_hex;

/// How long a report may wait in the queue, counted from when it was queued.
const KEEP: u64 = 7 * DAY;

/// How old a temporary file must be before a drain removes it as left by a
/// crash; a younger one may be another process's write in progress.
const STRAY: u64 = 3_600;

/// The wait after the first, second, third and fourth failed post; every later
/// failure waits as long as the last.
const BACKOFF: [u64; 4] = [15 * 60, 3_600, 4 * 3_600, DAY];

/// How the program sends one report: a `POST` of `body` to `url` with this
/// `User-Agent`, answering the status, or an error when no status came back
/// (no network, a timeout, a refused connection). The program's transport
/// should use a short timeout; the crate calls it only from `Queue::drain`.
pub trait Transport {
    fn post(&self, url: &str, user_agent: &str, body: &[u8]) -> Result<u16, String>;
}

/// The queue under a program's folder: `Queue::new(dir)` works on `<dir>/queue`.
pub struct Queue {
    dir: PathBuf,
}

/// What one `drain` did, file by file: `sent` the collector accepted (`2xx`);
/// `failed` are rescheduled (`429`, `5xx`, an error, or any other status that
/// is neither success nor refusal); `rejected` the collector refused (another
/// `4xx`), deleted; `dropped` were deleted unsent, a report older than seven
/// days, a companion without its report, or a report from a source build;
/// `skipped` were due but kept, since the mode does not let them leave or only
/// logs them.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub sent: usize,
    pub failed: usize,
    pub rejected: usize,
    pub dropped: usize,
    pub skipped: usize,
}

/// The wait after the `attempts`-th failed post, `attempts` counting from 1.
fn backoff(attempts: u32) -> u64 {
    let i = (attempts.max(1) as usize - 1).min(BACKOFF.len() - 1);
    BACKOFF[i]
}

/// Deletes a report and then its companion. The report goes first, so a crash
/// between the two leaves a companion alone, which the next drain removes,
/// rather than a report without one, which would be sent again.
fn remove_both(report: &Path) {
    let _ = fs::remove_file(report);
    let _ = fs::remove_file(report.with_extension("retry"));
}

impl Queue {
    /// The queue in `<dir>/queue`. Nothing is read or created until it is used.
    pub fn new(dir: &Path) -> Queue {
        Queue { dir: dir.to_path_buf() }
    }

    /// The queue's folder.
    fn folder(&self) -> PathBuf {
        self.dir.join("queue")
    }

    /// The files in the queue's folder with this extension, sorted by name.
    fn files(&self, extension: &str) -> Vec<PathBuf> {
        files_in(&self.folder(), extension)
    }

    /// Queues a crash report as `<dir>/queue/crash-<inc>.json` with a fresh
    /// companion, both written through a temporary file and a rename, the
    /// companion first, and returns the report's path. The program calls it
    /// once the person chose to send the crash, or at once when crashes are
    /// `Always`; queuing it is the consent `drain` relies on.
    ///
    /// An `inc` that is not 16 to 32 lowercase hex characters is `InvalidInput`
    /// and a crash the wire would refuse is `InvalidData`, before anything is
    /// written. Queuing the same incident again replaces its report and starts
    /// its companion afresh.
    pub fn enqueue_crash(&self, crash: &Crash, now: u64) -> io::Result<PathBuf> {
        if !is_hex(&crash.inc, 16, 32) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "an incident id is 16 to 32 lowercase hex characters"));
        }
        let bytes = crash.to_bytes().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let folder = self.folder();
        fs::create_dir_all(&folder)?;
        let path = folder.join(format!("crash-{}.json", crash.inc));
        Retry::fresh(now).write(&path)?;
        write_whole(&path, &bytes)?;
        Ok(path)
    }

    /// Deletes every queued session report and its companion, and returns how
    /// many reports it deleted; crash reports stay. A program calls it when the
    /// person turns usage `On` and wants only what is recorded from then on to
    /// leave, rather than the sessions sealed under `Local` in the last seven
    /// days. A companion without its report is removed too and not counted.
    pub fn clear_sessions(&self) -> io::Result<usize> {
        let mut cleared = 0;
        for report in self.files("json") {
            if is_session(&report) {
                match fs::remove_file(&report) {
                    Ok(()) => cleared += 1,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
        }
        for retry in self.files("retry") {
            if is_session(&retry) {
                match fs::remove_file(&retry) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
            }
        }
        Ok(cleared)
    }

    /// The reports due at `now`, sorted by name: those whose companion's `next`
    /// has come, and those with no readable companion. It deletes nothing; a
    /// report past its seven days is listed until a `drain` drops it.
    pub fn due(&self, now: u64) -> Vec<PathBuf> {
        self.files("json").into_iter().filter(|p| Retry::read(p).is_none_or(|r| r.next <= now)).collect()
    }

    /// Posts each report due at `now` that the mode lets leave, and returns
    /// what happened to each. `state.sent` and `state.rejected` count what the
    /// collector accepted and refused; the caller saves `state` afterwards,
    /// since `drain` never writes `state.toml`.
    ///
    /// First, whatever the mode: a companion without its report is removed, a
    /// report first queued more than seven days ago is deleted with its
    /// companion, as is a report whose `ch` is `source` (sealed from a source
    /// build's journal, which only the collector's `403` would otherwise stop),
    /// all three `dropped`; a report without a readable companion, or with one
    /// whose `first` is more than a day ahead of `now`, is given a fresh one;
    /// and a temporary file a crash left over an hour ago, in the queue or the
    /// folder, is removed.
    ///
    /// Then, with `mode.log`, every due report is printed to stderr as
    /// `visits-app: would send <file name>: <bytes>` and kept (`skipped`), and
    /// the transport is never called; the name is printed without its folder,
    /// whose path would carry the person's user name. Otherwise a `session-*` report is posted only
    /// when `mode.uploads`, and a `crash-*` report only when `mode.reason` is
    /// `None` and `mode.crashes` is not `Never`, since queuing a crash was the
    /// person's consent, withdrawing it stops what is queued, and `uploads` is
    /// false under the default `Local`; any other report is `skipped` and kept.
    ///
    /// A `2xx` deletes the report and its companion; a `429`, a `5xx`, an
    /// error or any status outside `2xx` and `4xx` reschedules it on the
    /// backoff; any other `4xx` deletes both, since the collector will refuse
    /// it again.
    pub fn drain(&self, t: &dyn Transport, url: &str, user_agent: &str, mode: &Mode, now: u64, state: &mut State) -> Outcome {
        self.drain_to(t, url, user_agent, mode, now, state, &mut |line| eprintln!("{line}"))
    }

    /// `drain`, handing each line `mode.log` prints to `log`, one call per
    /// line and without its newline, so a test can collect what would reach
    /// stderr.
    #[allow(clippy::too_many_arguments)]
    fn drain_to(&self, t: &dyn Transport, url: &str, user_agent: &str, mode: &Mode, now: u64, state: &mut State, log: &mut dyn FnMut(&str)) -> Outcome {
        let mut out = Outcome::default();
        self.sweep(now, &mut out);
        for path in self.due(now) {
            if mode.log {
                if let Ok(bytes) = fs::read(&path) {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    log(&format!("visits-app: would send {name}: {}", String::from_utf8_lossy(&bytes)));
                }
                out.skipped += 1;
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let eligible = if name.starts_with("session-") {
                mode.uploads
            } else if name.starts_with("crash-") {
                mode.reason.is_none() && mode.crashes != Crashes::Never
            } else {
                false
            };
            if !eligible {
                out.skipped += 1;
                continue;
            }
            let body = match fs::read(&path) {
                Ok(body) => body,
                // Gone since it was listed: another drain has dealt with it.
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => {
                    reschedule(&path, now);
                    out.failed += 1;
                    continue;
                }
            };
            match t.post(url, user_agent, &body) {
                Ok(200..=299) => {
                    remove_both(&path);
                    state.sent = state.sent.saturating_add(1);
                    out.sent += 1;
                }
                Ok(status) if (400..=499).contains(&status) && status != 429 => {
                    remove_both(&path);
                    state.rejected = state.rejected.saturating_add(1);
                    out.rejected += 1;
                }
                Ok(_) | Err(_) => {
                    reschedule(&path, now);
                    out.failed += 1;
                }
            }
        }
        out
    }

    /// Removes companions without their report, reports past their seven days
    /// and reports from a source build, counting each as dropped; gives a
    /// report without a readable companion, or with one from more than a day
    /// ahead (a damaged file or a clock set back), a fresh one, so it too is
    /// dropped in seven days; and removes stray temporary files.
    fn sweep(&self, now: u64, out: &mut Outcome) {
        for folder in [self.folder(), self.dir.clone()] {
            for tmp in files_in(&folder, "tmp") {
                if modified_before(&tmp, now.saturating_sub(STRAY)) {
                    let _ = fs::remove_file(&tmp);
                }
            }
        }
        for retry in self.files("retry") {
            if !retry.with_extension("json").exists() && fs::remove_file(&retry).is_ok() {
                out.dropped += 1;
            }
        }
        for report in self.files("json") {
            if is_source(&report) {
                remove_both(&report);
                out.dropped += 1;
                continue;
            }
            match Retry::read(&report) {
                Some(r) if r.first > now.saturating_add(DAY) => {
                    let _ = Retry::fresh(now).write(&report);
                }
                Some(r) if now.saturating_sub(r.first) > KEEP => {
                    remove_both(&report);
                    out.dropped += 1;
                }
                Some(_) => {}
                None => {
                    let _ = Retry::fresh(now).write(&report);
                }
            }
        }
    }
}

/// Whether a queue file is a session's report or companion, by its name.
fn is_session(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("session-"))
}

/// Whether the report at `path` says it comes from a source build: its
/// top-level `ch` is `source`. A file that cannot be read or parsed is not.
fn is_source(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else { return false };
    serde_json::from_slice::<Value>(&bytes).is_ok_and(|v| v.get("ch").and_then(Value::as_str) == Some("source"))
}

/// Whether the file at `path` was last modified before `cutoff` (unix
/// seconds); `false` when its time cannot be read, so a file of unknown age is kept.
pub(crate) fn modified_before(path: &Path, cutoff: u64) -> bool {
    let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
    let secs = modified.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs());
    secs.is_some_and(|secs| secs < cutoff)
}

/// The files in `folder` with this extension, sorted by name, so `x.json.tmp`,
/// a write in progress, is never one. Empty when the folder does not exist or
/// cannot be read.
pub(crate) fn files_in(folder: &Path, extension: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == extension) && p.is_file())
        .collect();
    files.sort();
    files
}

/// Counts one more failed post of the report at `report` and sets its next
/// time on the backoff. A write that fails leaves the report due, to be tried
/// at the next drain.
fn reschedule(report: &Path, now: u64) {
    let r = Retry::read(report).unwrap_or(Retry::fresh(now));
    let attempts = r.attempts.saturating_add(1);
    let _ = Retry { first: r.first, attempts, next: now.saturating_add(backoff(attempts)) }.write(report);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode::{Crashes, Usage};

    struct Never;

    impl Transport for Never {
        fn post(&self, _: &str, _: &str, _: &[u8]) -> Result<u16, String> {
            panic!("the transport was called");
        }
    }

    #[test]
    fn log_prints_each_due_report_with_its_bytes() {
        let dir = std::env::temp_dir().join(format!("visits-app-queue-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let queue = dir.join("queue");
        fs::create_dir_all(&queue).unwrap();
        // Written session first, so the lines must come in name order, not in
        // the order the files were made.
        let session = queue.join("session-3f9a1c27b84d5e60.json");
        fs::write(&session, br#"{"k":"session"}"#).unwrap();
        Retry::fresh(100).write(&session).unwrap();
        let crash = queue.join("crash-a41c0e9d7b3258f6.json");
        fs::write(&crash, br#"{"k":"crash"}"#).unwrap();
        Retry::fresh(100).write(&crash).unwrap();
        let mode = Mode { usage: Usage::On, crashes: Crashes::Always, uploads: false, invite: false, log: true, reason: Some("telemetry log") };
        let mut lines = Vec::new();
        let out = Queue::new(&dir).drain_to(&Never, "u", "a", &mode, 100, &mut State::default(), &mut |line| lines.push(line.to_string()));
        assert_eq!(out, Outcome { skipped: 2, ..Outcome::default() });
        // The file's name only: the folder's path would carry the person's user name.
        assert_eq!(
            lines,
            [
                "visits-app: would send crash-a41c0e9d7b3258f6.json: {\"k\":\"crash\"}".to_string(),
                "visits-app: would send session-3f9a1c27b84d5e60.json: {\"k\":\"session\"}".to_string(),
            ]
        );
        assert!(crash.exists() && session.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_backoff_holds_at_a_day() {
        let waits: Vec<u64> = (1..=7).map(backoff).collect();
        assert_eq!(waits, [900, 3_600, 14_400, DAY, DAY, DAY, DAY]);
    }
}
