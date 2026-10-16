//! What the program's `telemetry view` command and its "Show report" button
//! print (the design's 4.7): everything is read from disk at the moment of
//! asking, so what the person sees is what is there.

use crate::mode::Mode;
use crate::queue::files_in;
use crate::retry::Retry;
use crate::seal::LAST;
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
    /// `<dir>/last-session.json`, the copy `seal` keeps of the session report
    /// it sealed last, whether or not that report has since been sent or
    /// dropped; `None` before the first seal.
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
/// copy of the session report sealed last, kept beside the queue, so it is
/// there to show after the report itself has left.
pub fn view(dir: &Path, mode: Mode, now: u64) -> View {
    let queued = files_in(&dir.join("queue"), "json")
        .into_iter()
        .map(|path| {
            let (attempts, next) = Retry::read(&path).map_or((0, now), |r| (r.attempts, r.next));
            (path, attempts, next)
        })
        .collect();
    View {
        mode,
        state: State::load(dir),
        queued,
        last_session: Some(dir.join(LAST)).filter(|p| p.is_file()),
        journals: files_in(&dir.join("journal"), "jsonl"),
    }
}

/// The bytes of the report at `path`, exactly as they would be sent, for the
/// program to show the person; a journal's path gives its lines.
pub fn payload_of(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path)
}
