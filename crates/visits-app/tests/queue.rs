use serde_json::{json, Value};
use std::cell::RefCell;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};
use visits_app::{mode, Arch, Channel, Crash, CrashKind, Crashes, Envelope, Memory, Mode, Os, Outcome, Queue, Settings, State, Transport, Uptime, Usage};

const DAY: u64 = 86_400;
const NOW: u64 = 20_633 * DAY + DAY / 2;
const URL: &str = "https://visits.example.com/a";
const UA: &str = "arkos/0.4.2";
const VAR: &str = "ARKOS_TELEMETRY";

const SESSION: &str = "session-3f9a1c27b84d5e60";
const CRASH: &str = "crash-a41c0e9d7b3258f6";

/// A fresh, empty folder under the system's temporary directory.
fn scratch(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("visits-app-queue-{}-{name}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A transport that records every call and answers each from a script, the
/// last answer repeating once the script runs out.
struct Fake {
    answers: RefCell<Vec<Result<u16, String>>>,
    calls: RefCell<Vec<(String, String, Vec<u8>)>>,
}

impl Fake {
    fn answering(answers: &[Result<u16, String>]) -> Fake {
        Fake { answers: RefCell::new(answers.to_vec()), calls: RefCell::new(Vec::new()) }
    }

    fn calls(&self) -> usize {
        self.calls.borrow().len()
    }
}

impl Transport for Fake {
    fn post(&self, url: &str, user_agent: &str, body: &[u8]) -> Result<u16, String> {
        self.calls.borrow_mut().push((url.to_string(), user_agent.to_string(), body.to_vec()));
        let mut answers = self.answers.borrow_mut();
        if answers.len() > 1 { answers.remove(0) } else { answers[0].clone() }
    }
}

/// The mode `mode()` gives for these settings and environment on a release build.
fn mode_for(settings: Settings, vars: &[(&str, &str)]) -> Mode {
    let env = |k: &str| vars.iter().find(|(name, _)| *name == k).map(|(_, v)| v.to_string());
    mode(&settings, &env, VAR, &Channel::Release, true)
}

fn on() -> Mode {
    mode_for(Settings { usage: Some(Usage::On), crashes: Some(Crashes::Always) }, &[])
}

/// Writes `<dir>/queue/<name>.json` and, when `retry` is given, its companion.
fn put(dir: &Path, name: &str, retry: Option<Value>) -> PathBuf {
    let queue = dir.join("queue");
    std::fs::create_dir_all(&queue).unwrap();
    let path = queue.join(format!("{name}.json"));
    std::fs::write(&path, format!("{{\"report\":\"{name}\"}}")).unwrap();
    if let Some(retry) = retry {
        std::fs::write(path.with_extension("retry"), retry.to_string()).unwrap();
    }
    path
}

/// A companion as `seal` and `enqueue_crash` write it.
fn fresh(at: u64) -> Option<Value> {
    Some(json!({ "first": at, "attempts": 0, "next": at }))
}

fn retry_of(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path.with_extension("retry")).unwrap()).unwrap()
}

/// The names in `<dir>/queue`, sorted.
fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(dir.join("queue")) {
        Ok(entries) => entries.map(|e| e.unwrap().file_name().into_string().unwrap()).collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

fn envelope() -> Envelope {
    Envelope { app: "arkos".into(), ver: "0.4.2".into(), bin: "app".into(), os: Os::Macos, osv: 15, arch: Arch::Arm64, ch: Channel::Release }
}

fn crash(inc: &str) -> Crash {
    Crash {
        env: envelope(),
        inc: inc.into(),
        ck: CrashKind::Panic,
        commit: "9f2c1ab4e7d03b65a1c8f2e94d7b06a35c81e2fd".into(),
        pk: "other".into(),
        mh: None,
        file: "/src/editor/buffer.rs".into(),
        line: 412,
        frames: vec![(0, 4096)],
        images: vec![("arkos".into(), "0123456789abcdef0123456789abcdef".into(), 52428800)],
        sig: None,
        up: Uptime::Lt1h,
        mem: Some(Memory::Unknown),
    }
}

/// One drain at `now` with these answers, returning the outcome and the
/// transport, its counts added to `state` afterwards as the program adds them.
fn drain(dir: &Path, answers: &[Result<u16, String>], m: &Mode, now: u64, state: &mut State) -> (Outcome, Fake) {
    let fake = Fake::answering(answers);
    let outcome = Queue::new(dir).drain(&fake, URL, UA, m, now);
    state.sent += outcome.sent as u64;
    state.rejected += outcome.rejected as u64;
    (outcome, fake)
}

#[test]
fn enqueue_crash_writes_the_report_and_a_fresh_companion() {
    let dir = scratch("enqueue");
    let c = crash("a41c0e9d7b3258f6");
    let path = Queue::new(&dir).enqueue_crash(&c, NOW).unwrap();
    assert_eq!(path, dir.join("queue").join("crash-a41c0e9d7b3258f6.json"));
    assert_eq!(std::fs::read(&path).unwrap(), c.to_bytes().unwrap());
    assert_eq!(retry_of(&path), json!({ "first": NOW, "attempts": 0, "next": NOW }));
    assert_eq!(listing(&dir), ["crash-a41c0e9d7b3258f6.json", "crash-a41c0e9d7b3258f6.retry"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn enqueue_crash_takes_an_incident_of_32_hex() {
    let dir = scratch("enqueue-32");
    let inc = "0123456789abcdef0123456789abcdef";
    let path = Queue::new(&dir).enqueue_crash(&crash(inc), NOW).unwrap();
    assert_eq!(path.file_name().unwrap(), format!("crash-{inc}.json").as_str());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn enqueue_crash_refuses_an_incident_that_is_not_16_to_32_hex() {
    let dir = scratch("enqueue-bad");
    let q = Queue::new(&dir);
    for inc in ["../../etc/passwd", "a41c0e9d7b3258f", "0123456789abcdef0123456789abcdef0", "A41C0E9D7B3258F6", "a41c0e9d7b3258g6", ""] {
        let err = q.enqueue_crash(&crash(inc), NOW).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput, "{inc}");
    }
    assert!(listing(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn enqueue_crash_refuses_a_crash_the_wire_would_refuse() {
    let dir = scratch("enqueue-wire");
    let mut c = crash("a41c0e9d7b3258f6");
    c.pk = "Not A Name".into();
    let err = Queue::new(&dir).enqueue_crash(&c, NOW).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidData);
    assert!(listing(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn due_lists_the_files_whose_time_has_come() {
    let dir = scratch("due");
    let now_due = put(&dir, "session-0000000000000001", fresh(NOW));
    let past = put(&dir, "session-0000000000000002", Some(json!({ "first": NOW - DAY, "attempts": 2, "next": NOW - 1 })));
    put(&dir, "session-0000000000000003", Some(json!({ "first": NOW - DAY, "attempts": 2, "next": NOW + 1 })));
    let bare = put(&dir, "crash-0000000000000004", None);
    std::fs::write(dir.join("queue").join("session-0000000000000005.json.tmp"), "{}").unwrap();
    let q = Queue::new(&dir);
    assert_eq!(q.due(NOW), [bare, now_due, past]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn due_with_no_queue_folder_is_empty() {
    let dir = scratch("due-none");
    assert!(Queue::new(&dir).due(NOW).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_2xx_deletes_both_files_and_counts_sent() {
    for status in [200, 202, 204] {
        let dir = scratch("sent");
        let path = put(&dir, SESSION, fresh(NOW));
        let body = std::fs::read(&path).unwrap();
        let mut state = State { sent: 5, ..State::default() };
        let (outcome, fake) = drain(&dir, &[Ok(status)], &on(), NOW, &mut state);
        assert_eq!(outcome, Outcome { sent: 1, ..Outcome::default() });
        assert_eq!(*fake.calls.borrow(), [(URL.to_string(), UA.to_string(), body)]);
        assert!(listing(&dir).is_empty(), "{status}");
        assert_eq!(state, State { sent: 6, ..State::default() });
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn an_err_reschedules_and_never_deletes() {
    let dir = scratch("err");
    let path = put(&dir, SESSION, fresh(NOW - 60));
    let mut state = State::default();
    let (outcome, fake) = drain(&dir, &[Err("timed out".into())], &on(), NOW, &mut state);
    assert_eq!(outcome, Outcome { failed: 1, ..Outcome::default() });
    assert_eq!(fake.calls(), 1);
    assert!(path.exists());
    assert_eq!(retry_of(&path), json!({ "first": NOW - 60, "attempts": 1, "next": NOW + 15 * 60 }));
    assert_eq!(state, State::default());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_429_or_5xx_reschedules() {
    for status in [429, 500, 502, 503, 599] {
        let dir = scratch("retry");
        let path = put(&dir, CRASH, fresh(NOW));
        let mut state = State::default();
        let (outcome, _) = drain(&dir, &[Ok(status)], &on(), NOW, &mut state);
        assert_eq!(outcome, Outcome { failed: 1, ..Outcome::default() }, "{status}");
        assert!(path.exists());
        assert_eq!(retry_of(&path), json!({ "first": NOW, "attempts": 1, "next": NOW + 15 * 60 }));
        assert_eq!(state, State::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn another_4xx_deletes_both_files_and_counts_rejected() {
    for status in [400, 403, 404, 405, 413] {
        let dir = scratch("rejected");
        put(&dir, SESSION, fresh(NOW));
        let mut state = State { rejected: 2, ..State::default() };
        let (outcome, _) = drain(&dir, &[Ok(status)], &on(), NOW, &mut state);
        assert_eq!(outcome, Outcome { rejected: 1, ..Outcome::default() }, "{status}");
        assert!(listing(&dir).is_empty());
        assert_eq!(state, State { rejected: 3, ..State::default() });
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn the_backoff_is_15_minutes_an_hour_4_hours_a_day_then_daily() {
    let dir = scratch("backoff");
    let path = put(&dir, SESSION, fresh(NOW));
    let mut state = State::default();
    let mut now = NOW;
    for (attempts, wait) in [(1, 15 * 60), (2, 3_600), (3, 4 * 3_600), (4, DAY), (5, DAY), (6, DAY)] {
        let (outcome, fake) = drain(&dir, &[Ok(503)], &on(), now, &mut state);
        assert_eq!((outcome.failed, fake.calls()), (1, 1), "attempt {attempts}");
        assert_eq!(retry_of(&path), json!({ "first": NOW, "attempts": attempts, "next": now + wait }));
        // A second before the next time, the file is not due and is not posted.
        let (outcome, fake) = drain(&dir, &[Ok(204)], &on(), now + wait - 1, &mut state);
        assert_eq!((outcome, fake.calls()), (Outcome::default(), 0), "attempt {attempts}");
        now += wait;
    }
    let (outcome, _) = drain(&dir, &[Ok(204)], &on(), now, &mut state);
    assert_eq!(outcome.sent, 1);
    assert!(listing(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_first_queued_over_seven_days_ago_is_dropped_attempts_or_not() {
    let dir = scratch("drop");
    let old = NOW - 7 * DAY - 1;
    put(&dir, "session-0000000000000001", fresh(old));
    put(&dir, "session-0000000000000002", Some(json!({ "first": old, "attempts": 4, "next": NOW + DAY })));
    put(&dir, "crash-0000000000000003", Some(json!({ "first": old, "attempts": 6, "next": NOW - 1 })));
    // Exactly seven days old is not yet older than seven days.
    let kept = put(&dir, "session-0000000000000004", fresh(NOW - 7 * DAY));
    let kept_body = std::fs::read(&kept).unwrap();
    let mut state = State::default();
    let (outcome, fake) = drain(&dir, &[Ok(204)], &on(), NOW, &mut state);
    assert_eq!(outcome, Outcome { sent: 1, dropped: 3, ..Outcome::default() });
    assert_eq!(*fake.calls.borrow(), [(URL.to_string(), UA.to_string(), kept_body)]);
    assert!(listing(&dir).is_empty());
    assert_eq!(state, State { sent: 1, ..State::default() });
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_old_file_is_dropped_even_when_nothing_may_be_sent() {
    let dir = scratch("drop-local");
    put(&dir, SESSION, fresh(NOW - 8 * DAY));
    let local = mode_for(Settings::default(), &[]);
    let (outcome, fake) = drain(&dir, &[Ok(204)], &local, NOW, &mut State::default());
    assert_eq!(outcome, Outcome { dropped: 1, ..Outcome::default() });
    assert_eq!(fake.calls(), 0);
    assert!(listing(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_companion_without_its_report_is_removed_and_dropped() {
    let dir = scratch("orphan");
    let path = put(&dir, SESSION, fresh(NOW));
    std::fs::remove_file(&path).unwrap();
    let (outcome, fake) = drain(&dir, &[Ok(204)], &on(), NOW, &mut State::default());
    assert_eq!(outcome, Outcome { dropped: 1, ..Outcome::default() });
    assert_eq!(fake.calls(), 0);
    assert!(listing(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_report_without_a_companion_is_due_now_and_gets_one_on_its_first_attempt() {
    let dir = scratch("bare");
    let path = put(&dir, SESSION, None);
    let (outcome, fake) = drain(&dir, &[Err("offline".into())], &on(), NOW, &mut State::default());
    assert_eq!((outcome.failed, fake.calls()), (1, 1));
    assert_eq!(retry_of(&path), json!({ "first": NOW, "attempts": 1, "next": NOW + 15 * 60 }));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_report_whose_companion_is_damaged_is_treated_as_having_none() {
    let dir = scratch("damaged");
    let path = put(&dir, SESSION, None);
    std::fs::write(path.with_extension("retry"), "{\"first\": \"yesterday\"").unwrap();
    assert_eq!(Queue::new(&dir).due(NOW), std::slice::from_ref(&path));
    let (outcome, _) = drain(&dir, &[Ok(503)], &on(), NOW, &mut State::default());
    assert_eq!(outcome.failed, 1);
    assert_eq!(retry_of(&path), json!({ "first": NOW, "attempts": 1, "next": NOW + 15 * 60 }));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_reason_calls_the_transport_zero_times_and_skips() {
    for vars in [&[("CI", "1")][..], &[("DO_NOT_TRACK", "1")][..], &[(VAR, "off")][..]] {
        let dir = scratch("reason");
        let session = put(&dir, SESSION, fresh(NOW));
        let crash = put(&dir, CRASH, fresh(NOW));
        let m = mode_for(Settings { usage: Some(Usage::On), crashes: Some(Crashes::Always) }, vars);
        assert!(m.reason.is_some());
        let mut state = State::default();
        let (outcome, fake) = drain(&dir, &[Ok(204)], &m, NOW, &mut state);
        assert_eq!(outcome, Outcome { skipped: 2, ..Outcome::default() }, "{vars:?}");
        assert_eq!(fake.calls(), 0);
        assert_eq!((retry_of(&session), retry_of(&crash)), (fresh(NOW).unwrap(), fresh(NOW).unwrap()));
        assert_eq!(state, State::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn a_source_build_never_sends() {
    let dir = scratch("source");
    put(&dir, SESSION, fresh(NOW));
    put(&dir, CRASH, fresh(NOW));
    let env = |_: &str| None;
    let m = mode(&Settings { usage: Some(Usage::On), crashes: Some(Crashes::Always) }, &env, VAR, &Channel::Source, true);
    let (outcome, fake) = drain(&dir, &[Ok(204)], &m, NOW, &mut State::default());
    assert_eq!(outcome, Outcome { skipped: 2, ..Outcome::default() });
    assert_eq!(fake.calls(), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_default_mode_sends_the_crash_and_skips_the_session() {
    let dir = scratch("local");
    let session = put(&dir, SESSION, fresh(NOW));
    let crash = put(&dir, CRASH, fresh(NOW));
    let crash_body = std::fs::read(&crash).unwrap();
    let local = mode_for(Settings::default(), &[]);
    assert_eq!((local.usage, local.uploads, local.reason), (Usage::Local, false, None));
    let mut state = State::default();
    let (outcome, fake) = drain(&dir, &[Ok(204)], &local, NOW, &mut state);
    assert_eq!(outcome, Outcome { sent: 1, skipped: 1, ..Outcome::default() });
    assert_eq!(*fake.calls.borrow(), [(URL.to_string(), UA.to_string(), crash_body)]);
    assert!(!crash.exists() && !crash.with_extension("retry").exists());
    assert!(session.exists());
    assert_eq!(retry_of(&session), fresh(NOW).unwrap());
    assert_eq!(state.sent, 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn crashes_never_skips_a_queued_crash() {
    let dir = scratch("never");
    let crash = put(&dir, CRASH, fresh(NOW));
    for usage in [Usage::Local, Usage::On] {
        let never = mode_for(Settings { usage: Some(usage), crashes: Some(Crashes::Never) }, &[]);
        assert_eq!((never.reason, never.crashes), (None, Crashes::Never));
        let mut state = State::default();
        let (outcome, fake) = drain(&dir, &[Ok(204)], &never, NOW, &mut state);
        assert_eq!(outcome, Outcome { skipped: 1, ..Outcome::default() }, "{usage:?}");
        assert_eq!(fake.calls(), 0);
        assert!(crash.exists());
        assert_eq!(retry_of(&crash), fresh(NOW).unwrap());
        assert_eq!(state, State::default());
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn usage_on_sends_both_kinds() {
    let dir = scratch("on");
    put(&dir, SESSION, fresh(NOW));
    put(&dir, CRASH, fresh(NOW));
    let mut state = State::default();
    let (outcome, fake) = drain(&dir, &[Ok(204)], &on(), NOW, &mut state);
    assert_eq!(outcome, Outcome { sent: 2, ..Outcome::default() });
    assert_eq!(fake.calls(), 2);
    assert!(listing(&dir).is_empty());
    assert_eq!(state.sent, 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn telemetry_log_prints_and_keeps_every_due_file_and_never_posts() {
    let dir = scratch("log");
    let session = put(&dir, SESSION, fresh(NOW));
    let crash = put(&dir, CRASH, Some(json!({ "first": NOW - DAY, "attempts": 2, "next": NOW - 1 })));
    let later = put(&dir, "session-0000000000000009", Some(json!({ "first": NOW - DAY, "attempts": 2, "next": NOW + 1 })));
    let m = mode_for(Settings { usage: Some(Usage::On), crashes: Some(Crashes::Always) }, &[(VAR, "log")]);
    assert!(m.log);
    let mut state = State::default();
    let (outcome, fake) = drain(&dir, &[Ok(204)], &m, NOW, &mut state);
    assert_eq!(outcome, Outcome { skipped: 2, ..Outcome::default() });
    assert_eq!(fake.calls(), 0);
    assert_eq!(retry_of(&session), fresh(NOW).unwrap());
    assert_eq!(retry_of(&crash), json!({ "first": NOW - DAY, "attempts": 2, "next": NOW - 1 }));
    assert!(later.exists());
    assert_eq!(listing(&dir).len(), 6);
    assert_eq!(state, State::default());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_of_another_kind_is_skipped_and_kept() {
    let dir = scratch("other");
    let path = put(&dir, "check-0000000000000001", fresh(NOW));
    let (outcome, fake) = drain(&dir, &[Ok(204)], &on(), NOW, &mut State::default());
    assert_eq!(outcome, Outcome { skipped: 1, ..Outcome::default() });
    assert_eq!(fake.calls(), 0);
    assert!(path.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn drain_never_writes_the_state_file() {
    let dir = scratch("no-state");
    put(&dir, SESSION, fresh(NOW));
    drain(&dir, &[Ok(204)], &on(), NOW, &mut State::default());
    drain(&dir, &[Ok(400)], &on(), NOW, &mut State::default());
    assert!(!dir.join("state.toml").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Creates `path` with a modification time of `at` (unix seconds).
fn touch(path: &Path, at: u64) {
    let file = std::fs::File::create(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(at)).unwrap();
}

#[test]
fn a_companion_from_more_than_a_day_ahead_is_damaged_and_made_afresh() {
    let dir = scratch("future");
    let ahead = put(&dir, SESSION, Some(json!({ "first": NOW + DAY + 1, "attempts": 0, "next": NOW + DAY + 1 })));
    let within = put(&dir, CRASH, Some(json!({ "first": NOW + DAY, "attempts": 1, "next": NOW + DAY })));
    let local = mode_for(Settings::default(), &[]);
    let (outcome, fake) = drain(&dir, &[Ok(204)], &local, NOW, &mut State::default());
    assert_eq!(outcome, Outcome { skipped: 1, ..Outcome::default() }, "the session is due at once, the crash not yet");
    assert_eq!(fake.calls(), 0);
    assert_eq!(retry_of(&ahead), fresh(NOW).unwrap());
    assert_eq!(retry_of(&within), json!({ "first": NOW + DAY, "attempts": 1, "next": NOW + DAY }));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn stray_temp_files_are_removed_from_the_queue_and_the_folder() {
    let dir = scratch("tmp");
    put(&dir, SESSION, fresh(NOW));
    let queue = dir.join("queue");
    let stray = [
        queue.join(format!("{SESSION}.json.tmp")),
        queue.join(format!("{CRASH}.retry.tmp")),
        queue.join("anything.tmp"),
        dir.join("state.toml.4242.tmp"),
        dir.join("last-session.json.tmp"),
    ];
    for path in &stray {
        touch(path, NOW - 2 * 3_600);
    }
    // A write in progress, from the last hour, is left to finish, and in the
    // folder itself only the crate's own names are its to remove.
    let fresh_tmp = [
        queue.join("crash-0000000000000001.json.tmp"),
        dir.join("state.toml.4343.tmp"),
        dir.join("notes.tmp"),
        dir.join("state.toml.tmp.tmp"),
        dir.join("other.json.tmp"),
    ];
    touch(&fresh_tmp[0], NOW - 60);
    touch(&fresh_tmp[1], NOW - 60);
    for path in &fresh_tmp[2..] {
        touch(path, NOW - 2 * 3_600);
    }
    let local = mode_for(Settings::default(), &[]);
    let (outcome, _) = drain(&dir, &[Ok(204)], &local, NOW, &mut State::default());
    assert_eq!(outcome, Outcome { skipped: 1, ..Outcome::default() }, "a temp file is not a report");
    for path in &stray {
        assert!(!path.exists(), "{}", path.display());
    }
    for path in &fresh_tmp {
        assert!(path.exists(), "{}", path.display());
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Writes `<dir>/queue/<name>.json` with these bytes and a companion due at `next`.
fn put_bytes(dir: &Path, name: &str, bytes: &[u8], next: u64) -> PathBuf {
    let path = put(dir, name, Some(json!({ "first": NOW, "attempts": 0, "next": next })));
    std::fs::write(&path, bytes).unwrap();
    path
}

const SOURCE_BYTES: &[u8] = br#"{"v":2,"app":"arkos","ch":"source","k":"session"}"#;

#[test]
fn a_due_source_report_is_dropped_before_it_could_be_posted() {
    let dir = scratch("source-report");
    let source = put_bytes(&dir, SESSION, SOURCE_BYTES, NOW);
    // Not yet due: the post path never reaches it, and the sweep leaves it.
    let later = put_bytes(&dir, "session-0000000000000009", SOURCE_BYTES, NOW + 1);
    // A `d` key named `ch` is the program's own and says nothing of the build.
    let release = put_bytes(&dir, CRASH, br#"{"v":2,"app":"arkos","ch":"release","k":"crash","ev":[{"d":{"ch":"source"}}]}"#, NOW);
    let (outcome, fake) = drain(&dir, &[Ok(204)], &on(), NOW, &mut State::default());
    assert_eq!(outcome, Outcome { sent: 1, dropped: 1, ..Outcome::default() });
    assert_eq!(fake.calls(), 1);
    assert!(fake.calls.borrow().iter().all(|(_, _, body)| body.as_slice() != SOURCE_BYTES));
    assert!(!source.exists() && !source.with_extension("retry").exists());
    assert!(!release.exists(), "sent");
    assert!(later.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_source_build_under_log_keeps_its_reports_to_print() {
    let dir = scratch("source-log");
    let source = put_bytes(&dir, SESSION, SOURCE_BYTES, NOW);
    let env = |k: &str| (k == VAR).then(|| "log".to_string());
    let m = mode(&Settings { usage: Some(Usage::On), crashes: Some(Crashes::Always) }, &env, VAR, &Channel::Source, true);
    assert!(m.log);
    let (outcome, fake) = drain(&dir, &[Ok(204)], &m, NOW, &mut State::default());
    assert_eq!(outcome, Outcome { skipped: 1, ..Outcome::default() });
    assert_eq!(fake.calls(), 0);
    assert_eq!(std::fs::read(&source).unwrap(), SOURCE_BYTES);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn clear_sessions_deletes_the_queued_sessions_and_keeps_the_crashes() {
    let dir = scratch("clear");
    assert_eq!(Queue::new(&dir).clear_sessions().unwrap(), 0, "no queue folder is nothing to clear");
    put(&dir, SESSION, fresh(NOW));
    put(&dir, "session-0000000000000002", None);
    let lone = put(&dir, "session-0000000000000003", fresh(NOW));
    std::fs::remove_file(&lone).unwrap();
    put(&dir, CRASH, fresh(NOW));
    assert_eq!(Queue::new(&dir).clear_sessions().unwrap(), 2);
    assert_eq!(listing(&dir), [format!("{CRASH}.json"), format!("{CRASH}.retry")]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_outcome_is_copied() {
    let a = Outcome { sent: 1, ..Outcome::default() };
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn a_drain_posts_each_due_file_once_with_its_own_answer() {
    let dir = scratch("mixed");
    let a = put(&dir, "crash-0000000000000001", fresh(NOW));
    let b = put(&dir, "session-0000000000000002", fresh(NOW));
    let c = put(&dir, "session-0000000000000003", fresh(NOW));
    let mut state = State::default();
    let (outcome, fake) = drain(&dir, &[Ok(204), Err("reset".into()), Ok(413)], &on(), NOW, &mut state);
    assert_eq!(outcome, Outcome { sent: 1, failed: 1, rejected: 1, ..Outcome::default() });
    assert_eq!(fake.calls(), 3);
    assert!(!a.exists() && b.exists() && !c.exists());
    assert_eq!(listing(&dir), ["session-0000000000000002.json", "session-0000000000000002.retry"]);
    assert_eq!((state.sent, state.rejected), (1, 1));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_crash_read_back_from_an_incident_file_is_requeued() {
    let dir = scratch("requeue");
    let original = crash("a41c0e9d7b3258f6");
    let incident = dir.join("incident.json");
    std::fs::write(&incident, original.to_bytes().unwrap()).unwrap();
    let read = Crash::from_bytes(&std::fs::read(&incident).unwrap()).unwrap();
    assert_eq!(read, original);
    let path = Queue::new(&dir).enqueue_crash(&read, NOW).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original.to_bytes().unwrap());
    assert_eq!(retry_of(&path), json!({ "first": NOW, "attempts": 0, "next": NOW }));
    std::fs::remove_dir_all(&dir).unwrap();
}
