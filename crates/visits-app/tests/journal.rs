use serde_json::{json, Value};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use visits_app::{Allowlist, Arch, Channel, DValue, Envelope, Event, Journal, Os, Rejected};

const ALLOW: &str = r#"
[events.pane_open]
kind = ["terminal", "viewer", "browser"]
how = ["palette", "key", "click"]
[events.window_open]
[events.resize]
cols = "int"
full = "bool"
"#;

const SID: &str = "3f9a1c27b84d5e60";

/// A fresh, empty folder under the system's temporary directory.
fn scratch(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("visits-app-journal-{}-{name}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn envelope() -> Envelope {
    Envelope { app: "arkos".into(), ver: "0.5.0".into(), bin: "app".into(), os: Os::Macos, osv: 15, arch: Arch::Arm64, ch: Channel::Release }
}

fn event(n: &str, d: &[(&str, DValue)]) -> Event {
    Event { n: n.into(), t: 0, d: d.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn s(v: &str) -> DValue {
    DValue::Str(v.into())
}

fn lines(dir: &std::path::Path) -> Vec<Value> {
    let text = std::fs::read_to_string(dir.join("journal").join(format!("{SID}.jsonl"))).unwrap();
    assert!(text.ends_with('\n'), "every line ends with a newline");
    text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn a_toml_with_events_parses_and_admits_what_it_lists() {
    let a = Allowlist::from_toml(ALLOW).unwrap();
    assert_eq!(a.check(&event("pane_open", &[("kind", s("viewer")), ("how", s("key"))])), Ok(()));
    assert_eq!(a.check(&event("pane_open", &[("kind", s("terminal"))])), Ok(()));
    assert_eq!(a.check(&event("window_open", &[])), Ok(()));
    let two = "[events.pane_open]\nkind = [\"terminal\"]\n[events.window_open]\n";
    assert!(Allowlist::from_toml(two).is_ok());
    assert!(Allowlist::from_toml("").is_ok());
}

#[test]
fn from_toml_refuses_what_the_wire_would() {
    assert!(Allowlist::from_toml("[events.Pane]\n").is_err(), "an upper-case name");
    assert!(Allowlist::from_toml("[events.\"pane-open\"]\n").is_err(), "a dash in a name");
    assert!(Allowlist::from_toml(&format!("[events.{}]\n", "a".repeat(33))).is_err(), "a name over 32");
    assert!(Allowlist::from_toml("[events.x]\nBad = \"int\"\n").is_err(), "a bad key");
    let seven = (1..=7).map(|i| format!("k{i} = \"int\"\n")).collect::<String>();
    assert!(Allowlist::from_toml(&format!("[events.x]\n{seven}")).is_err(), "a seventh key");
    let six = (1..=6).map(|i| format!("k{i} = \"int\"\n")).collect::<String>();
    assert!(Allowlist::from_toml(&format!("[events.x]\n{six}")).is_ok(), "six keys");
    assert!(Allowlist::from_toml("[events.x]\nk = [\"a b\"]\n").is_err(), "a value with a space");
    assert!(Allowlist::from_toml(&format!("[events.x]\nk = [\"{}\"]\n", "a".repeat(65))).is_err(), "a value over 64");
    assert!(Allowlist::from_toml(&format!("[events.x]\nk = [\"{}\"]\n", "a".repeat(64))).is_ok(), "a value of 64");
    assert!(Allowlist::from_toml("[events.x]\nk = \"text\"\n").is_err(), "a type other than int or bool");
    assert!(Allowlist::from_toml("[events.x]\nk = [1]\n").is_err(), "a value that is not a string");
    assert!(Allowlist::from_toml("[events.x]\nk = 1\n").is_err(), "a key that is neither a list nor a type");
    assert!(Allowlist::from_toml("[event.x]\n").is_err(), "a table other than events");
    assert!(Allowlist::from_toml("[events.x\n").is_err(), "not TOML");
}

#[test]
fn check_names_what_is_not_allowed() {
    let a = Allowlist::from_toml(ALLOW).unwrap();
    assert_eq!(a.check(&event("pane_close", &[])), Err(Rejected::Name("pane_close".into())));
    assert_eq!(
        a.check(&event("pane_open", &[("kind", s("viewer")), ("size", DValue::Int(3))])),
        Err(Rejected::Key("pane_open".into(), "size".into()))
    );
    assert_eq!(a.check(&event("pane_open", &[("kind", s("editor"))])), Err(Rejected::Value("pane_open".into(), "kind".into())));
    assert_eq!(a.check(&event("pane_open", &[("kind", DValue::Bool(true))])), Err(Rejected::Value("pane_open".into(), "kind".into())));
}

#[test]
fn int_admits_integers_only_and_bool_booleans_only() {
    let a = Allowlist::from_toml(ALLOW).unwrap();
    assert_eq!(a.check(&event("resize", &[("cols", DValue::Int(120))])), Ok(()));
    assert_eq!(a.check(&event("resize", &[("cols", DValue::Int(-4))])), Ok(()));
    assert_eq!(a.check(&event("resize", &[("cols", s("120"))])), Err(Rejected::Value("resize".into(), "cols".into())));
    assert_eq!(a.check(&event("resize", &[("cols", DValue::Bool(false))])), Err(Rejected::Value("resize".into(), "cols".into())));
    assert_eq!(a.check(&event("resize", &[("full", DValue::Bool(false))])), Ok(()));
    assert_eq!(a.check(&event("resize", &[("full", DValue::Int(1))])), Err(Rejected::Value("resize".into(), "full".into())));
    assert_eq!(a.check(&event("resize", &[("full", s("true"))])), Err(Rejected::Value("resize".into(), "full".into())));
}

#[test]
fn open_refuses_a_sid_that_could_be_a_path() {
    let dir = scratch("sid");
    for sid in ["../x", "../../etc/passwd0", "3F9A1C27B84D5E60", "3f9a1c27b84d5e6", &"a".repeat(33), "3f9a1c27b84d5e6g", ""] {
        let err = Journal::open(&dir, sid, &envelope(), Allowlist::from_toml(ALLOW).unwrap(), 1000).err();
        assert_eq!(err.map(|e| e.kind()), Some(ErrorKind::InvalidInput), "{sid:?}");
    }
    assert!(!dir.join("x").exists() && !dir.join("journal").exists(), "nothing is created for a refused sid");
    assert!(Journal::open(&dir, &"a".repeat(32), &envelope(), Allowlist::from_toml(ALLOW).unwrap(), 1000).is_ok());
}

#[test]
fn the_journal_has_a_start_line_events_and_marks() {
    let dir = scratch("lines");
    let mut j = Journal::open(&dir, SID, &envelope(), Allowlist::from_toml(ALLOW).unwrap(), 1000).unwrap();
    j.event(event("window_open", &[]), 1000);
    j.event(event("pane_open", &[("kind", s("viewer")), ("how", s("click"))]), 1012);
    j.event(event("pane_close", &[]), 1013);
    j.event(event("pane_open", &[("kind", s("editor"))]), 1014);
    j.event(event("resize", &[("cols", DValue::Int(-3))]), 1020);
    j.focus("terminal", 30);
    j.focus("viewer", 5);
    j.focus("terminal", 12);
    j.mark(1060).unwrap();

    let start = json!({ "start": 1000, "env": { "app": "arkos", "ver": "0.5.0", "bin": "app", "os": "macos", "osv": 15, "arch": "arm64", "ch": "release" } });
    let first = vec![
        start,
        json!({ "ev": { "n": "window_open", "t": 0 } }),
        json!({ "ev": { "n": "pane_open", "t": 12, "d": { "kind": "viewer", "how": "click" } } }),
        json!({ "ev": { "n": "resize", "t": 20, "d": { "cols": -3 } } }),
        json!({ "mark": { "t": 60, "focus": { "terminal": 42, "viewer": 5 }, "rejected": 2 } }),
    ];
    assert_eq!(lines(&dir), first);

    j.focus("viewer", 10);
    j.mark(1120).unwrap();
    let mut second = first.clone();
    second.push(json!({ "mark": { "t": 120, "focus": { "terminal": 42, "viewer": 15 }, "rejected": 2 } }));
    assert_eq!(lines(&dir), second, "a second mark appends a line with the running totals");
}

#[test]
fn a_mark_with_nothing_recorded_has_empty_focus() {
    let dir = scratch("empty");
    let mut j = Journal::open(&dir, SID, &envelope(), Allowlist::from_toml(ALLOW).unwrap(), 500).unwrap();
    j.mark(530).unwrap();
    assert_eq!(lines(&dir)[1], json!({ "mark": { "t": 30, "focus": {}, "rejected": 0 } }));
}

#[test]
fn a_named_channel_is_spelled_as_its_name() {
    let dir = scratch("channel");
    let env = Envelope { ch: Channel::Named("tap".into()), ..envelope() };
    Journal::open(&dir, SID, &env, Allowlist::from_toml(ALLOW).unwrap(), 7).unwrap();
    assert_eq!(lines(&dir)[0]["env"]["ch"], json!("tap"));
}

#[test]
fn a_surface_the_wire_would_refuse_is_left_out_of_focus() {
    let dir = scratch("surfaces");
    let mut j = Journal::open(&dir, SID, &envelope(), Allowlist::from_toml(ALLOW).unwrap(), 0).unwrap();
    j.focus("Bad Surface", 9);
    for i in 0..17 {
        j.focus(&format!("s{i:02}"), 1);
    }
    j.focus("s00", 1);
    j.mark(60).unwrap();
    let focus = lines(&dir)[1]["mark"]["focus"].as_object().unwrap().clone();
    assert_eq!(focus.len(), 16);
    assert_eq!(focus["s00"], json!(2));
    assert!(!focus.contains_key("s16") && !focus.contains_key("Bad Surface"));
}

#[test]
fn a_journal_is_opened_once() {
    let dir = scratch("twice");
    Journal::open(&dir, SID, &envelope(), Allowlist::from_toml(ALLOW).unwrap(), 0).unwrap();
    let err = Journal::open(&dir, SID, &envelope(), Allowlist::from_toml(ALLOW).unwrap(), 5).err();
    assert_eq!(err.map(|e| e.kind()), Some(ErrorKind::AlreadyExists));
    assert_eq!(lines(&dir).len(), 1, "the first start line is kept alone");
}
