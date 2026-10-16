//! What the program's `telemetry view` command and its "Show report" button
//! print (the design's 4.7): everything is read from disk at the moment of
//! asking, so what the person sees is what is there.

use crate::mode::Mode;
use crate::queue::{Queue, Retry};
use crate::state::State;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The crate's folder as it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// The mode the program runs in, as the program computed it.
    pub mode: Mode,
    /// `<dir>/state.toml`: the days the reports count from, and the counts of
    /// reports sent and refused.
    pub state: State,
    /// Every report in `<dir>/queue`, sorted by name, with its failed
    /// attempts and the time it is next due, from its `.retry` companion.
    pub queued: Vec<(PathBuf, u32, u64)>,
    /// The session report sealed last among those still queued.
    pub last_session: Option<PathBuf>,
    /// The journals in `<dir>/journal`, sorted by name: sessions running, or
    /// left by a process that is gone and not yet sealed.
    pub journals: Vec<PathBuf>,
}

/// Reads the crate's folder `dir` for the person, with the `mode` the program
/// runs in. Nothing is written or deleted, and a folder that does not exist,
/// or a part of it that cannot be read, shows as empty.
///
/// A report whose companion is missing or unreadable is shown with no attempts
/// and due at `now`, which is how the queue treats it. The last session is the
/// `session-*` report whose companion says it was queued last, the name
/// deciding a tie, and a report without a companion counts as queued at `now`,
/// as the next drain will record it. A session the collector accepted has left
/// the queue, so under `On` this is usually `None`.
pub fn view(dir: &Path, mode: Mode, now: u64) -> View {
    let mut queued = Vec::new();
    let mut last: Option<(u64, PathBuf)> = None;
    for path in Queue::new(dir).files("json") {
        let retry = Retry::read(&path);
        let (first, attempts, next) = retry.map_or((now, 0, now), |r| (r.first, r.attempts, r.next));
        let is_session = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("session-"));
        // The files come sorted by name, so `>=` lets the later name win a tie.
        if is_session && last.as_ref().is_none_or(|(at, _)| first >= *at) {
            last = Some((first, path.clone()));
        }
        queued.push((path, attempts, next));
    }
    View { mode, state: State::load(dir), queued, last_session: last.map(|(_, path)| path), journals: journals(dir) }
}

/// The `<sid>.jsonl` files in `<dir>/journal`, sorted by name.
fn journals(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir.join("journal")) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl") && p.is_file())
        .collect();
    files.sort();
    files
}

/// The bytes of the report at `path`, exactly as they would be sent, for the
/// program to show the person; a journal's path gives its lines.
pub fn payload_of(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path)
}
