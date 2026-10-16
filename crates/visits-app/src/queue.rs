//! The queue of reports waiting to leave the machine, `<dir>/queue`, and the
//! one place the crate ever calls the program's `Transport`.
//!
//! A report is `queue/<kind>-<id>.json`, the exact bytes to send, beside its
//! companion `queue/<kind>-<id>.retry`, which is
//! `{"first": <secs>, "attempts": <n>, "next": <secs>}`: when the report was
//! queued, how many posts have failed, and when it is next due. `seal` and
//! `Queue::enqueue_crash` write both, the companion first, each through a
//! temporary file and a rename, so the queue never reads a file's time and
//! never sees half a report. A report found without a companion (or with one
//! that does not parse) is due at once and is given a fresh one by the next
//! `drain`, so it ages like any other.
//!
//! A failed post is tried again 15 minutes later, then an hour, four hours and
//! a day later, then daily. Whatever its attempts, a report first queued more
//! than seven days ago is deleted, so the queue never grows without bound,
//! even under `Local`, where sessions are never sent.

use crate::mode::Mode;
use crate::report::Crash;
use crate::seal::write_whole;
use crate::state::State;
use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use visits_core::app::is_hex;

/// Seconds in a day.
const DAY: u64 = 86_400;

/// How long a report may wait in the queue, counted from when it was queued.
const KEEP: u64 = 7 * DAY;

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
/// days or a companion without its report; `skipped` were due but kept, since
/// the mode does not let them leave or only logs them.
#[derive(Default, Debug, PartialEq)]
pub struct Outcome {
    pub sent: usize,
    pub failed: usize,
    pub rejected: usize,
    pub dropped: usize,
    pub skipped: usize,
}

/// A report's companion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Retry {
    pub(crate) first: u64,
    pub(crate) attempts: u32,
    pub(crate) next: u64,
}

impl Retry {
    /// The companion of a report queued at `now`: due at once, never tried.
    fn fresh(now: u64) -> Retry {
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
    fn write(&self, report: &Path) -> io::Result<()> {
        let v = json!({ "first": self.first, "attempts": self.attempts, "next": self.next });
        write_whole(&report.with_extension("retry"), v.to_string().as_bytes())
    }
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

    /// The files in the queue's folder with this extension, sorted by name, so
    /// `x.json.tmp`, a write in progress, is never one. Empty when the folder
    /// does not exist or cannot be read.
    pub(crate) fn files(&self, extension: &str) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.folder()) else { return Vec::new() };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == extension) && p.is_file())
            .collect();
        files.sort();
        files
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
    /// companion (both `dropped`), and a report without a readable companion is
    /// given a fresh one.
    ///
    /// Then, with `mode.log`, every due report is printed to stderr as
    /// `visits-app: would send <path>: <bytes>` and kept (`skipped`), and the
    /// transport is never called. Otherwise a `session-*` report is posted only
    /// when `mode.uploads`, and a `crash-*` report only when `mode.reason` is
    /// `None`, since queuing a crash was the person's consent and `uploads` is
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
                    log(&format!("visits-app: would send {}: {}", path.display(), String::from_utf8_lossy(&bytes)));
                }
                out.skipped += 1;
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let eligible = if name.starts_with("session-") {
                mode.uploads
            } else if name.starts_with("crash-") {
                mode.reason.is_none()
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

    /// Removes companions without their report and reports past their seven
    /// days, counting both as dropped, and gives a report without a readable
    /// companion a fresh one, so it too is dropped in seven days.
    fn sweep(&self, now: u64, out: &mut Outcome) {
        for retry in self.files("retry") {
            if !retry.with_extension("json").exists() && fs::remove_file(&retry).is_ok() {
                out.dropped += 1;
            }
        }
        for report in self.files("json") {
            match Retry::read(&report) {
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
        assert_eq!(
            lines,
            [
                format!("visits-app: would send {}: {{\"k\":\"crash\"}}", crash.display()),
                format!("visits-app: would send {}: {{\"k\":\"session\"}}", session.display()),
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
