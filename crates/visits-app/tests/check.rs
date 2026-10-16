use serde_json::json;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use visits_app::{mode, payload_of, seal, view, Allowlist, Arch, Channel, Check, End, Envelope, Journal, Mode, Os, Settings, State, Usage};

const DAY: u64 = 86_400;
const D0: u64 = 20_633;
const NOON: u64 = D0 * DAY + DAY / 2;

/// A fresh, empty folder under the system's temporary directory.
fn scratch(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("visits-app-check-{}-{name}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn envelope(ch: Channel) -> Envelope {
    Envelope { app: "arkos".into(), ver: "0.4.2".into(), bin: "app".into(), os: Os::Macos, osv: 15, arch: Arch::Arm64, ch }
}

fn release() -> Envelope {
    envelope(Channel::Release)
}

/// The state after a check made at `at`.
fn checked_at(at: u64) -> State {
    let mut state = State::default();
    state.checked(at);
    state
}

fn rd(env: &Envelope, state: &State, now: u64) -> Option<i64> {
    Check::next(env, state, now).map(|c| c.rd)
}

#[test]
fn the_first_check_ever_has_rd_minus_one() {
    let check = Check::next(&release(), &State::default(), NOON).unwrap();
    assert_eq!(check, Check { env: release(), rd: -1 });
    assert!(check.to_query().unwrap().ends_with("&k=check&rd=-1"));
}

#[test]
fn checked_records_today_and_nothing_else() {
    let mut state = State { first_launch_day: Some(3), last_report_day: Some(4), last_check_day: Some(5), rejected: 6, sent: 7 };
    state.checked(NOON);
    assert_eq!(state, State { first_launch_day: Some(3), last_report_day: Some(4), last_check_day: Some(D0 as i64), rejected: 6, sent: 7 });
}

#[test]
fn there_is_no_second_check_the_same_utc_day() {
    let state = checked_at(NOON);
    assert_eq!(rd(&release(), &state, NOON), None);
    assert_eq!(rd(&release(), &state, NOON + 3_600), None);
    assert_eq!(rd(&release(), &state, D0 * DAY), None);
    assert_eq!(rd(&release(), &state, D0 * DAY + DAY - 1), None);
}

#[test]
fn rd_counts_the_days_since_the_last_check() {
    let state = checked_at(NOON);
    assert_eq!(rd(&release(), &state, NOON + DAY), Some(1));
    assert_eq!(rd(&release(), &state, NOON + 3 * DAY), Some(3));
    assert_eq!(rd(&release(), &state, NOON + 7 * DAY), Some(7));
}

#[test]
fn one_second_across_a_utc_midnight_is_the_next_day() {
    let state = checked_at(D0 * DAY + DAY - 1);
    assert_eq!(rd(&release(), &state, D0 * DAY + DAY - 1), None);
    assert_eq!(rd(&release(), &state, (D0 + 1) * DAY), Some(1));
}

#[test]
fn a_clock_set_back_still_checks_with_rd_zero() {
    let state = checked_at(NOON + 2 * DAY);
    assert_eq!(rd(&release(), &state, NOON), Some(0));
}

#[test]
fn a_source_build_never_checks_through_the_collector() {
    for ch in [Channel::Source, Channel::Named("source".into())] {
        let env = envelope(ch);
        for state in [State::default(), checked_at(NOON), checked_at(NOON - 3 * DAY)] {
            assert_eq!(Check::next(&env, &state, NOON), None);
        }
    }
    assert_eq!(rd(&envelope(Channel::Named("tap".into())), &State::default(), NOON), Some(-1));
}

/// Writes `<dir>/queue/<name>.json` and, when `retry` is given, its companion.
fn put(dir: &Path, name: &str, retry: Option<(u64, u32, u64)>) -> PathBuf {
    let queue = dir.join("queue");
    std::fs::create_dir_all(&queue).unwrap();
    let path = queue.join(format!("{name}.json"));
    std::fs::write(&path, format!("{{\"report\":\"{name}\"}}")).unwrap();
    if let Some((first, attempts, next)) = retry {
        std::fs::write(path.with_extension("retry"), json!({ "first": first, "attempts": attempts, "next": next }).to_string()).unwrap();
    }
    path
}

fn local() -> Mode {
    mode(&Settings::default(), &|_| None, "ARKOS_TELEMETRY", &Channel::Release, true)
}

#[test]
fn view_lists_the_queue_and_the_journals() {
    let dir = scratch("view");
    let newer = put(&dir, "session-0a0a0a0a0a0a0a0a", Some((NOON - 3_600, 0, NOON - 3_600)));
    let older = put(&dir, "session-ffffffffffffffff", Some((NOON - 2 * DAY, 3, NOON + 4 * 3_600)));
    let crash = put(&dir, "crash-a41c0e9d7b3258f6", Some((NOON - 600, 1, NOON + 300)));
    // A write in progress and a companion without its report are not reports.
    std::fs::write(dir.join("queue/session-1111111111111111.json.tmp"), b"{").unwrap();
    std::fs::write(dir.join("queue/session-2222222222222222.retry"), b"{}").unwrap();
    let journal = dir.join("journal");
    std::fs::create_dir_all(&journal).unwrap();
    std::fs::write(journal.join("bbbbbbbbbbbbbbbb.jsonl"), b"").unwrap();
    std::fs::write(journal.join("aaaaaaaaaaaaaaaa.jsonl"), b"").unwrap();
    std::fs::write(journal.join("notes.txt"), b"").unwrap();
    let state = State { first_launch_day: Some(D0 as i64 - 9), last_report_day: Some(D0 as i64), last_check_day: None, rejected: 2, sent: 5 };
    state.save(&dir).unwrap();

    let v = view(&dir, local(), NOON);
    assert_eq!(v.mode, local());
    assert_eq!(v.mode.usage, Usage::Local);
    assert_eq!(v.state, state);
    assert_eq!(v.queued, vec![(crash, 1, NOON + 300), (newer, 0, NOON - 3_600), (older, 3, NOON + 4 * 3_600)]);
    // Reports in the queue are not a sealed session: only `seal` keeps one.
    assert_eq!(v.last_session, None);
    assert_eq!(v.journals, vec![journal.join("aaaaaaaaaaaaaaaa.jsonl"), journal.join("bbbbbbbbbbbbbbbb.jsonl")]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_report_without_its_companion_is_listed_as_due_now() {
    let dir = scratch("bare");
    let crash = put(&dir, "crash-a41c0e9d7b3258f6", None);
    let v = view(&dir, local(), NOON);
    assert_eq!(v.queued, vec![(crash, 0, NOON)]);
    assert_eq!(v.last_session, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn view_of_an_empty_folder_is_empty() {
    let dir = scratch("empty");
    let missing = dir.join("never-made");
    let v = view(&missing, local(), NOON);
    assert_eq!((v.state, v.queued, v.last_session, v.journals), (State::default(), vec![], None, vec![]));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn payload_of_is_the_bytes_on_disk() {
    let dir = scratch("payload");
    let path = put(&dir, "session-3f9a1c27b84d5e60", Some((NOON, 0, NOON)));
    assert_eq!(payload_of(&path).unwrap(), b"{\"report\":\"session-3f9a1c27b84d5e60\"}");
    assert_eq!(payload_of(&dir.join("queue/session-0000000000000000.json")).unwrap_err().kind(), ErrorKind::NotFound);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Opens and seals a journal for `sid` that started at `start`, returning the
/// queued report's path.
fn sealed(dir: &Path, sid: &str, start: u64, state: &mut State) -> PathBuf {
    let allow = Allowlist::from_toml("[events.window_open]").unwrap();
    Journal::open(dir, sid, &release(), allow, start).unwrap();
    seal(dir, sid, End::Quit, start + 600, state).unwrap().unwrap()
}

#[test]
fn the_last_sealed_session_is_kept_beside_the_queue() {
    let dir = scratch("last");
    let mut state = State::default();
    let first = sealed(&dir, "3f9a1c27b84d5e60", NOON, &mut state);
    let last = dir.join("last-session.json");
    let v = view(&dir, local(), NOON + 600);
    assert_eq!(v.last_session.as_deref(), Some(last.as_path()));
    assert_eq!(payload_of(&last).unwrap(), std::fs::read(&first).unwrap());

    // Sent: the queued report is gone, and the copy stays to be shown.
    let bytes = std::fs::read(&first).unwrap();
    std::fs::remove_file(&first).unwrap();
    std::fs::remove_file(first.with_extension("retry")).unwrap();
    let v = view(&dir, local(), NOON + 700);
    assert_eq!(v.queued, vec![]);
    assert_eq!(v.last_session.as_deref(), Some(last.as_path()));
    assert_eq!(payload_of(&last).unwrap(), bytes);

    // A second seal replaces it.
    let second = sealed(&dir, "a41c0e9d7b3258f6", NOON + DAY, &mut state);
    let v = view(&dir, local(), NOON + DAY + 600);
    assert_eq!(v.last_session.as_deref(), Some(last.as_path()));
    assert_eq!(payload_of(&last).unwrap(), std::fs::read(&second).unwrap());
    assert_ne!(payload_of(&last).unwrap(), bytes);
    std::fs::remove_dir_all(&dir).unwrap();
}
