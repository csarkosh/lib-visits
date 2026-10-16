use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use visits_app::{seal, seal_stale, Allowlist, Arch, Channel, DValue, End, Envelope, Event, Journal, Os, Session, State};

const SID: &str = "3f9a1c27b84d5e60";
const DAY: u64 = 86_400;

/// A day number that is not a multiple of 7, at noon UTC, so `iw` must round.
const D0: u64 = 20_633;
const NOON: u64 = D0 * DAY + DAY / 2;

/// A fresh, empty folder under the system's temporary directory.
fn scratch(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("visits-app-seal-{}-{name}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The envelope the start lines of these tests carry, which differs from the
/// one a program would build today, so a test can tell where it came from.
fn envelope() -> Envelope {
    Envelope { app: "arkos".into(), ver: "0.4.2".into(), bin: "app".into(), os: Os::Linux, osv: 6, arch: Arch::X86_64, ch: Channel::Named("tap".into()) }
}

fn start_line(start: u64) -> String {
    let env = json!({ "app": "arkos", "ver": "0.4.2", "bin": "app", "os": "linux", "osv": 6, "arch": "x86_64", "ch": "tap" });
    format!("{}\n", json!({ "start": start, "env": env }))
}

fn ev_line(n: &str, t: u64, d: Value) -> String {
    let mut ev = json!({ "n": n, "t": t });
    if d.as_object().is_some_and(|m| !m.is_empty()) {
        ev["d"] = d;
    }
    format!("{}\n", json!({ "ev": ev }))
}

fn mark_line(t: u64, focus: Value, rejected: u64) -> String {
    format!("{}\n", json!({ "mark": { "t": t, "focus": focus, "rejected": rejected } }))
}

fn write_journal(dir: &Path, sid: &str, text: &str) -> PathBuf {
    let folder = dir.join("journal");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("{sid}.jsonl"));
    std::fs::write(&path, text).unwrap();
    path
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn event(n: &str, t: u64, d: &[(&str, DValue)]) -> Event {
    Event { n: n.into(), t, d: d.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn focus(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

/// A journal of four events and two marks, started at `start`.
fn four_events_two_marks(start: u64) -> String {
    [
        start_line(start),
        ev_line("launch", 0, json!({})),
        ev_line("pane_open", 12, json!({ "kind": "terminal", "how": "key" })),
        mark_line(60, json!({ "editor": 40, "terminal": 15 }), 1),
        ev_line("resize", 70, json!({ "cols": 120, "full": true })),
        ev_line("window_open", 95, json!({})),
        mark_line(120, json!({ "editor": 80, "terminal": 30, "settings": 5 }), 2),
    ]
    .concat()
}

#[test]
fn a_journal_seals_into_the_report_it_describes() {
    let dir = scratch("four");
    let journal = write_journal(&dir, SID, &four_events_two_marks(NOON));
    let mut state = State::default();
    let now = NOON + 1840;

    let path = seal(&dir, SID, End::Quit, now, &mut state).unwrap().expect("a report is queued");

    assert_eq!(path, dir.join("queue").join(format!("session-{SID}.json")));
    let iw = (D0 - D0 % 7) as i64;
    let expected = Session {
        env: envelope(),
        sid: SID.into(),
        len: 1840,
        end: End::Quit,
        iw,
        rd: -1,
        focus: focus(&[("editor", 80), ("terminal", 30), ("settings", 5)]),
        ev: vec![
            event("launch", 0, &[]),
            event("pane_open", 12, &[("kind", DValue::Str("terminal".into())), ("how", DValue::Str("key".into()))]),
            event("resize", 70, &[("cols", DValue::Int(120)), ("full", DValue::Bool(true))]),
            event("window_open", 95, &[]),
        ],
        ev_dropped: 0,
        ev_rejected: 2,
    };
    assert_eq!(std::fs::read(&path).unwrap(), expected.to_bytes().unwrap(), "the queued bytes are the report's bytes");
    assert_eq!(read_json(&path)["ver"], "0.4.2", "the version that ran, from the start line");
    assert_eq!(read_json(&dir.join("queue").join(format!("session-{SID}.retry"))), json!({ "first": now, "attempts": 0, "next": now }));
    assert!(!journal.exists(), "the journal is deleted");
    assert!(!dir.join("state.toml").exists(), "seal never writes the state itself");
    assert_eq!(state.first_launch_day, Some(D0 as i64));
    assert_eq!(state.last_report_day, Some(D0 as i64));
    assert_eq!(state.rejected, 0);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_journal_the_journal_wrote_seals_with_what_it_recorded() {
    let dir = scratch("written");
    let allow = Allowlist::from_toml("[events.launch]\n[events.pane_open]\nkind = [\"terminal\"]\n[focus]\nsurfaces = [\"terminal\"]\n").unwrap();
    let mut journal = Journal::open(&dir, SID, &envelope(), allow, NOON).unwrap();
    journal.event(event("launch", 0, &[]), NOON);
    journal.event(event("pane_open", 0, &[("kind", DValue::Str("terminal".into()))]), NOON + 12);
    journal.event(event("not_listed", 0, &[]), NOON + 20);
    journal.focus("terminal", 40);
    journal.mark(NOON + 60).unwrap();
    drop(journal);
    let mut state = State::default();

    let path = seal(&dir, SID, End::Lost, NOON + 9000, &mut state).unwrap().unwrap();

    let report = read_json(&path);
    assert_eq!(report["ev"], json!([{ "n": "launch", "t": 0 }, { "n": "pane_open", "t": 12, "d": { "kind": "terminal" } }]));
    assert_eq!(report["focus"], json!({ "terminal": 40 }));
    assert_eq!(report["ev_rejected"], 1);
    assert_eq!(report["len"], 60);
    assert_eq!(report["ch"], "tap");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_events_the_journal_dropped_are_added_to_ev_dropped() {
    let dir = scratch("dropped");
    let mut journal = Journal::open(&dir, SID, &envelope(), Allowlist::from_toml("[events.tick]\n").unwrap(), NOON).unwrap();
    for t in 0..203 {
        journal.event(event("tick", 0, &[]), NOON + t);
    }
    journal.mark(NOON + 300).unwrap();
    drop(journal);
    let path = seal(&dir, SID, End::Quit, NOON + 400, &mut State::default()).unwrap().unwrap();
    let report = read_json(&path);
    assert_eq!(report["ev"].as_array().unwrap().len(), 200);
    assert_eq!(report["ev_dropped"], 3);

    // A journal an older build wrote, with more than 200 event lines and a mark
    // that counts dropped events too, adds both.
    let sid = "a41c0e9d7b3258f6";
    let mut text = start_line(NOON);
    for t in 0..202 {
        text.push_str(&ev_line("tick", t, json!({})));
    }
    text.push_str(&format!("{}\n", json!({ "mark": { "t": 250, "focus": {}, "rejected": 0, "dropped": 4 } })));
    write_journal(&dir, sid, &text);
    let path = seal(&dir, sid, End::Quit, NOON + 400, &mut State::default()).unwrap().unwrap();
    assert_eq!(read_json(&path)["ev_dropped"], 6);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_journal_without_a_mark_seals_with_an_empty_focus() {
    let dir = scratch("nomark");
    write_journal(&dir, SID, &[start_line(NOON), ev_line("launch", 0, json!({})), ev_line("window_open", 30, json!({}))].concat());
    let mut state = State::default();

    let path = seal(&dir, SID, End::Quit, NOON + 45, &mut state).unwrap().unwrap();

    let report = read_json(&path);
    assert_eq!(report["focus"], json!({}));
    assert_eq!(report["ev_rejected"], 0);
    assert_eq!(report["len"], 45);
    assert_eq!(report["ev"].as_array().unwrap().len(), 2);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_cut_last_line_and_a_garbled_line_are_ignored() {
    let dir = scratch("cut");
    let text = [
        start_line(NOON),
        ev_line("launch", 0, json!({})),
        "{\"ev\": {\"n\": \"pane_\n".to_string(),
        ev_line("window_open", 30, json!({})),
        // A whole object without its newline was cut before the newline landed.
        json!({ "ev": { "n": "late", "t": 99 } }).to_string(),
    ]
    .concat();
    write_journal(&dir, SID, &text);
    let mut state = State::default();

    let path = seal(&dir, SID, End::Lost, NOON + 5000, &mut state).unwrap().unwrap();

    let report = read_json(&path);
    let names: Vec<&str> = report["ev"].as_array().unwrap().iter().map(|e| e["n"].as_str().unwrap()).collect();
    assert_eq!(names, ["launch", "window_open"]);
    assert_eq!(report["len"], 30, "the cut line's t does not count");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_lost_session_lasts_until_the_last_time_the_journal_saw() {
    let dir = scratch("lost");
    write_journal(&dir, SID, &four_events_two_marks(NOON));
    let mut state = State::default();

    let path = seal(&dir, SID, End::Lost, NOON + 7 * 3600, &mut state).unwrap().unwrap();
    let report = read_json(&path);
    assert_eq!(report["len"], 120, "the last mark's t, not the sealing time");
    assert_eq!(report["end"], "lost");

    // An event after the last mark is the last time seen.
    let sid = "aaaaaaaaaaaaaaaa";
    write_journal(&dir, sid, &[four_events_two_marks(NOON), ev_line("window_open", 150, json!({}))].concat());
    let path = seal(&dir, sid, End::Unknown, NOON + 7 * 3600, &mut state).unwrap().unwrap();
    assert_eq!(read_json(&path)["len"], 150);
    assert_eq!(read_json(&path)["end"], "unknown");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rd_is_minus_one_the_first_time_and_the_day_difference_after() {
    let dir = scratch("rd");
    let mut state = State::default();

    write_journal(&dir, SID, &four_events_two_marks(NOON));
    let first = seal(&dir, SID, End::Quit, NOON + 600, &mut state).unwrap().unwrap();
    assert_eq!(read_json(&first)["rd"], -1);

    let later = NOON + 3 * DAY;
    let sid = "bbbbbbbbbbbbbbbb";
    write_journal(&dir, sid, &four_events_two_marks(later));
    let second = seal(&dir, sid, End::Quit, later + 600, &mut state).unwrap().unwrap();
    assert_eq!(read_json(&second)["rd"], 3);
    assert_eq!(state.last_report_day, Some(D0 as i64 + 3));

    let sid = "cccccccccccccccc";
    write_journal(&dir, sid, &four_events_two_marks(later + 900));
    let third = seal(&dir, sid, End::Quit, later + 1200, &mut state).unwrap().unwrap();
    assert_eq!(read_json(&third)["rd"], 0, "the same day");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn iw_is_a_week_and_is_set_once() {
    let dir = scratch("iw");
    let mut state = State::default();

    write_journal(&dir, SID, &four_events_two_marks(NOON));
    let first = seal(&dir, SID, End::Quit, NOON + 600, &mut state).unwrap().unwrap();
    let iw = read_json(&first)["iw"].as_i64().unwrap();
    assert_eq!(iw % 7, 0);
    assert_eq!(iw, D0 as i64 - (D0 as i64) % 7);
    assert_eq!(state.first_launch_day, Some(D0 as i64));

    let weeks_on = NOON + 30 * DAY;
    let sid = "dddddddddddddddd";
    write_journal(&dir, sid, &four_events_two_marks(weeks_on));
    let second = seal(&dir, sid, End::Quit, weeks_on + 600, &mut state).unwrap().unwrap();
    assert_eq!(read_json(&second)["iw"].as_i64().unwrap(), iw, "the install week never moves");
    assert_eq!(state.first_launch_day, Some(D0 as i64));

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_session_the_wire_refuses_is_dropped_and_counted() {
    let dir = scratch("refused");
    // A capital is outside the channel's name pattern, so the wire refuses it.
    let text = start_line(NOON).replace("\"tap\"", "\"Tap\"") + &ev_line("launch", 0, json!({}));
    let journal = write_journal(&dir, SID, &text);
    let mut state = State { rejected: 4, ..State::default() };

    assert_eq!(seal(&dir, SID, End::Quit, NOON + 60, &mut state).unwrap(), None);

    assert_eq!(state.rejected, 5);
    assert_eq!(state.last_report_day, None, "nothing was reported");
    assert!(!journal.exists());
    let queued = std::fs::read_dir(dir.join("queue")).map(|r| r.count()).unwrap_or(0);
    assert_eq!(queued, 0, "nothing is queued");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_source_build_seals_like_any_other() {
    let dir = scratch("source");
    let text = start_line(NOON).replace("\"tap\"", "\"source\"") + &ev_line("launch", 0, json!({}));
    write_journal(&dir, SID, &text);
    let mut state = State { rejected: 4, ..State::default() };

    let path = seal(&dir, SID, End::Quit, NOON + 60, &mut state).unwrap().expect("a source build's report is queued");

    assert_eq!(read_json(&path)["ch"], "source");
    assert_eq!(state.rejected, 4, "a source build is never counted as rejected");
    assert_eq!(state.last_report_day, Some(D0 as i64));

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_journal_without_a_start_line_is_discarded() {
    let dir = scratch("nostart");
    let journal = write_journal(&dir, SID, &[ev_line("launch", 0, json!({})), mark_line(60, json!({}), 0)].concat());
    let mut state = State::default();

    assert_eq!(seal(&dir, SID, End::Quit, NOON, &mut state).unwrap(), None);
    assert!(!journal.exists());
    assert_eq!(state, State::default(), "a discarded journal changes no count and no day");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_journal_seals_to_nothing_and_a_bad_sid_is_refused() {
    let dir = scratch("missing");
    let mut state = State::default();
    assert_eq!(seal(&dir, SID, End::Quit, NOON, &mut state).unwrap(), None);
    assert_eq!(seal(&dir, "../../etc/passwd", End::Quit, NOON, &mut state).unwrap_err().kind(), ErrorKind::InvalidInput);
    assert_eq!(state, State::default());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn seal_stale_seals_the_old_journal_and_leaves_the_fresh_one() {
    let dir = scratch("stale");
    let now = NOON + 10 * DAY;
    let old = "1111111111111111";
    let fresh = "2222222222222222";
    // Started two days ago and last seen 120 seconds later.
    let old_journal = write_journal(&dir, old, &four_events_two_marks(now - 2 * DAY));
    // Started 30 hours ago but last seen 10 hours ago: still within the day.
    let fresh_text = [four_events_two_marks(now - 30 * 3600), mark_line(20 * 3600, json!({ "editor": 900 }), 2)].concat();
    let fresh_journal = write_journal(&dir, fresh, &fresh_text);
    let mut state = State::default();

    let sealed = seal_stale(&dir, now, DAY, &mut state).unwrap();

    assert_eq!(sealed, vec![dir.join("queue").join(format!("session-{old}.json"))]);
    let report = read_json(&sealed[0]);
    assert_eq!(report["end"], "unknown");
    assert_eq!(report["len"], 120);
    assert!(!old_journal.exists());
    assert!(fresh_journal.exists(), "a journal seen within the day is left alone");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn seal_stale_seals_oldest_first_so_the_first_launch_is_the_earliest() {
    let dir = scratch("stale-order");
    let now = NOON + 30 * DAY;
    // The sid that sorts first by name started later, so name order would be wrong.
    write_journal(&dir, "aaaaaaaaaaaaaaaa", &four_events_two_marks(NOON + 5 * DAY));
    write_journal(&dir, "ffffffffffffffff", &four_events_two_marks(NOON));
    let mut state = State::default();

    let sealed = seal_stale(&dir, now, DAY, &mut state).unwrap();

    let names: Vec<_> = sealed.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["session-ffffffffffffffff.json", "session-aaaaaaaaaaaaaaaa.json"]);
    assert_eq!(state.first_launch_day, Some(D0 as i64));
    let iw = (D0 - D0 % 7) as i64;
    assert_eq!(read_json(&sealed[1])["iw"], iw, "the later session carries the earlier install week");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn seal_stale_discards_a_start_less_journal_only_once_its_file_is_old() {
    let dir = scratch("stale-startless");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let fresh = write_journal(&dir, "3333333333333333", "");
    let old = write_journal(&dir, "4444444444444444", "{\"start\": 1");
    let two_days_ago = SystemTime::now() - Duration::from_secs(2 * DAY);
    std::fs::File::options().write(true).open(&old).unwrap().set_modified(two_days_ago).unwrap();
    let mut state = State::default();

    let sealed = seal_stale(&dir, now, DAY, &mut state).unwrap();

    assert!(sealed.is_empty());
    assert!(fresh.exists(), "another instance may be about to write its start line");
    assert!(!old.exists(), "a start-less journal older than the cutoff is discarded");
    assert_eq!(state, State::default());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn seal_stale_skips_an_entry_it_cannot_read() {
    let dir = scratch("stale-unreadable");
    let now = NOON + 10 * DAY;
    // A folder named like a journal cannot be read as a file.
    std::fs::create_dir_all(dir.join("journal").join("5555555555555555.jsonl")).unwrap();
    write_journal(&dir, "6666666666666666", &four_events_two_marks(NOON));
    let mut state = State::default();

    let sealed = seal_stale(&dir, now, DAY, &mut state).unwrap();

    assert_eq!(sealed, vec![dir.join("queue").join("session-6666666666666666.json")]);
    assert!(dir.join("journal").join("5555555555555555.jsonl").is_dir());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn seal_stale_without_a_journal_folder_seals_nothing() {
    let dir = scratch("stale-empty");
    let mut state = State::default();
    assert_eq!(seal_stale(&dir, NOON, DAY, &mut state).unwrap(), Vec::<PathBuf>::new());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_state_round_trips_through_its_file() {
    let dir = scratch("state");
    assert_eq!(State::load(&dir), State::default(), "no file is the default");

    let state = State { first_launch_day: Some(20_633), last_report_day: Some(20_640), last_check_day: None, rejected: 3, sent: 12 };
    state.save(&dir).unwrap();
    assert_eq!(State::load(&dir), state);
    let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["state.toml"], "the temp file is renamed away");

    std::fs::write(dir.join("state.toml"), "not = [toml").unwrap();
    assert_eq!(State::load(&dir), State::default(), "an unreadable file is the default");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_state_is_written_through_a_temp_file_named_for_the_process() {
    let dir = scratch("state-tmp");
    // Another process's temp file in the way changes nothing.
    std::fs::create_dir_all(dir.join("state.toml.tmp")).unwrap();
    std::fs::create_dir_all(dir.join("state.toml.1.tmp")).unwrap();
    State { sent: 1, ..State::default() }.save(&dir).unwrap();
    assert_eq!(State::load(&dir).sent, 1);
    // This process's own name in the way is the only one that stops it.
    std::fs::create_dir_all(dir.join(format!("state.toml.{}.tmp", std::process::id()))).unwrap();
    assert!(State::default().save(&dir).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn update_loads_changes_and_saves_the_state() {
    let dir = scratch("update");
    State::update(&dir, |s| s.sent += 1).unwrap();
    assert_eq!(State::load(&dir), State { sent: 1, ..State::default() }, "from no file, the default");

    // What another process saved in between is kept.
    State { rejected: 2, ..State::load(&dir) }.save(&dir).unwrap();
    let got = State::update(&dir, |s| {
        s.sent += 1;
        seal(&dir, SID, End::Quit, NOON, s)
    })
    .unwrap();
    assert_eq!(got.unwrap(), None, "the closure's own result comes back");
    assert_eq!(State::load(&dir), State { sent: 2, rejected: 2, ..State::default() });
    std::fs::remove_dir_all(&dir).unwrap();
}
