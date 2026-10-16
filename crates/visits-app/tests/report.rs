use serde_json::Value;
use std::collections::BTreeMap;
use visits_app::{Arch, Channel, Check, Crash, CrashKind, DValue, End, Envelope, Event, Invalid, Memory, Os, Session, Uptime};

fn fixture(name: &str) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/fixtures/app/");
    serde_json::from_str(&std::fs::read_to_string(format!("{path}{name}.json")).unwrap()).unwrap()
}

fn envelope(bin: &str) -> Envelope {
    Envelope {
        app: "arkos".into(),
        ver: "0.5.0".into(),
        bin: bin.into(),
        os: Os::Macos,
        osv: 15,
        arch: Arch::Arm64,
        ch: Channel::Release,
    }
}

fn event(n: &str, t: u64, d: &[(&str, DValue)]) -> Event {
    Event { n: n.into(), t, d: d.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// The values of `spec/fixtures/app/session.json`.
fn session() -> Session {
    Session {
        env: envelope("app"),
        sid: "3f9a1c27b84d5e60".into(),
        len: 1840,
        end: End::Quit,
        iw: 20629,
        rd: 1,
        focus: [("editor", 1210), ("terminal", 410), ("settings", 20)].iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        ev: vec![
            event("launch", 0, &[]),
            event(
                "command",
                12,
                &[
                    ("id", DValue::Str("file.open".into())),
                    ("src", DValue::Str("palette".into())),
                    ("n", DValue::Int(3)),
                    ("restored", DValue::Bool(true)),
                ],
            ),
            event("surface_open", 95, &[("name", DValue::Str("terminal".into()))]),
            event("quit", 1840, &[]),
        ],
        ev_dropped: 0,
        ev_rejected: 1,
    }
}

/// The values of `spec/fixtures/app/crash.json`.
fn crash() -> Crash {
    Crash {
        env: envelope("app"),
        inc: "a41c0e9d7b3258f6".into(),
        ck: CrashKind::Panic,
        commit: "9f2c1ab4e7d03b65a1c8f2e94d7b06a35c81e2fd".into(),
        pk: "other".into(),
        mh: Some("b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944c".into()),
        file: "/src/editor/buffer.rs".into(),
        line: 412,
        frames: vec![(0, 4096), (1, 81920)],
        images: vec![
            ("arkos".into(), "0123456789abcdef0123456789abcdef".into(), 52428800),
            ("libsystem_c.dylib".into(), "fedcba9876543210fedcba9876543210".into(), 3145728),
        ],
        sig: None,
        up: Uptime::Lt1h,
        mem: Some(Memory::Unknown),
    }
}

fn as_value(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn the_session_fixture_is_what_to_bytes_writes() {
    assert_eq!(as_value(&session().to_bytes().unwrap()), fixture("session"));
}

#[test]
fn the_crash_fixture_is_what_to_bytes_writes() {
    assert_eq!(as_value(&crash().to_bytes().unwrap()), fixture("crash"));
}

#[test]
fn the_check_fixture_is_what_to_query_writes() {
    let check = Check { env: envelope("daemon"), rd: -1 };
    assert_eq!(check.to_query().unwrap(), fixture("check")["query"].as_str().unwrap());
}

#[test]
fn a_session_without_events_or_counts_still_writes_them() {
    let mut s = session();
    s.ev.clear();
    s.ev_rejected = 0;
    let v = as_value(&s.to_bytes().unwrap());
    assert_eq!(v["ev"], serde_json::json!([]));
    assert_eq!(v["ev_dropped"], 0);
    assert_eq!(v["ev_rejected"], 0);
}

#[test]
fn an_event_with_an_empty_d_omits_it() {
    let v = as_value(&session().to_bytes().unwrap());
    assert!(v["ev"][0].as_object().unwrap().get("d").is_none());
}

#[test]
fn a_crash_with_a_signal_and_no_options_writes_sig_and_omits_the_rest() {
    let mut c = crash();
    c.ck = CrashKind::Signal;
    c.sig = Some(11);
    c.mh = None;
    c.mem = None;
    c.up = Uptime::Ge1d;
    let v = as_value(&c.to_bytes().unwrap());
    assert_eq!(v["ck"], "signal");
    assert_eq!(v["sig"], 11);
    assert_eq!(v["up"], ">=1d");
    let obj = v.as_object().unwrap();
    assert!(obj.get("mh").is_none());
    assert!(obj.get("mem").is_none());
}

#[test]
fn every_enum_writes_its_wire_spelling() {
    let mut s = session();
    for (os, text) in [(Os::Macos, "macos"), (Os::Linux, "linux"), (Os::Windows, "windows")] {
        s.env.os = os;
        assert_eq!(as_value(&s.to_bytes().unwrap())["os"], text);
    }
    for (arch, text) in [(Arch::Arm64, "arm64"), (Arch::X86_64, "x86_64")] {
        s.env.arch = arch;
        assert_eq!(as_value(&s.to_bytes().unwrap())["arch"], text);
    }
    for (end, text) in [(End::Quit, "quit"), (End::Lost, "lost"), (End::Unknown, "unknown")] {
        s.end = end;
        assert_eq!(as_value(&s.to_bytes().unwrap())["end"], text);
    }
    let mut c = crash();
    for (up, text) in [(Uptime::Lt1m, "<1m"), (Uptime::Lt10m, "<10m"), (Uptime::Lt1h, "<1h"), (Uptime::Lt1d, "<1d"), (Uptime::Ge1d, ">=1d")] {
        c.up = up;
        assert_eq!(as_value(&c.to_bytes().unwrap())["up"], text);
    }
    for (mem, text) in [(Memory::Normal, "normal"), (Memory::Warn, "warn"), (Memory::Critical, "critical"), (Memory::Unknown, "unknown")] {
        c.mem = Some(mem);
        assert_eq!(as_value(&c.to_bytes().unwrap())["mem"], text);
    }
}

#[test]
fn a_named_channel_is_written_as_its_name() {
    let mut s = session();
    s.env.ch = Channel::Named("tap".into());
    assert_eq!(as_value(&s.to_bytes().unwrap())["ch"], "tap");
}

#[test]
fn a_named_channel_with_a_capital_fails_its_pattern() {
    let mut s = session();
    s.env.ch = Channel::Named("Tap".into());
    assert_eq!(s.to_bytes(), Err(Invalid::Pattern("ch".into())));
    let check = Check { env: s.env.clone(), rd: 0 };
    assert_eq!(check.to_query(), Err(Invalid::Pattern("ch".into())));
}

#[test]
fn a_source_build_never_produces_bytes() {
    let mut s = session();
    s.env.ch = Channel::Source;
    assert_eq!(s.to_bytes(), Err(Invalid::Channel));
    let mut c = crash();
    c.env.ch = Channel::Source;
    assert_eq!(c.to_bytes(), Err(Invalid::Channel));
    let check = Check { env: c.env.clone(), rd: 0 };
    assert_eq!(check.to_query(), Err(Invalid::Channel));
}

#[test]
fn a_d_value_with_a_space_fails_its_pattern() {
    let mut s = session();
    s.ev.push(event("command", 20, &[("x", DValue::Str("two words".into()))]));
    assert_eq!(s.to_bytes(), Err(Invalid::Pattern("ev.d.x".into())));
}

#[test]
fn an_event_name_with_a_capital_fails_its_pattern() {
    let mut s = session();
    s.ev.push(event("Launch", 20, &[]));
    assert_eq!(s.to_bytes(), Err(Invalid::Pattern("ev.n".into())));
}

#[test]
fn an_iw_that_is_not_a_week_fails() {
    let mut s = session();
    s.iw = 20630;
    assert_eq!(s.to_bytes(), Err(Invalid::Pattern("iw".into())));
}

#[test]
fn a_check_rd_below_minus_one_fails() {
    let check = Check { env: envelope("daemon"), rd: -2 };
    assert_eq!(check.to_query(), Err(Invalid::Pattern("rd".into())));
}

#[test]
fn a_version_with_build_metadata_survives_the_query() {
    let mut env = envelope("daemon");
    env.ver = "1.2.3+build.7".into();
    let q = Check { env, rd: 0 }.to_query().unwrap();
    assert!(q.contains("ver=1.2.3%2Bbuild.7"), "{q}");
}

#[test]
fn push_keeps_the_first_200_and_counts_the_rest() {
    let mut s = session();
    s.ev.clear();
    for t in 0..205 {
        s.push(event("tick", t, &[]));
    }
    assert_eq!(s.ev.len(), 200);
    assert_eq!(s.ev_dropped, 5);
    assert_eq!(s.ev[199].t, 199);
    let v = as_value(&s.to_bytes().unwrap());
    assert_eq!(v["ev"].as_array().unwrap().len(), 200);
    assert_eq!(v["ev_dropped"], 5);
}

#[test]
fn an_event_d_map_is_built_from_a_btreemap() {
    let mut d = BTreeMap::new();
    d.insert("ok".to_string(), DValue::Bool(false));
    d.insert("delta".to_string(), DValue::Int(-4));
    let mut s = session();
    s.ev = vec![Event { n: "sync".into(), t: 3, d }];
    let v = as_value(&s.to_bytes().unwrap());
    assert_eq!(v["ev"][0]["d"], serde_json::json!({ "ok": false, "delta": -4 }));
}

#[test]
fn the_wire_spellings_read_back_into_the_enums() {
    for os in [Os::Macos, Os::Linux, Os::Windows] {
        assert_eq!(Os::from_wire(os.as_str()), Some(os));
    }
    for arch in [Arch::Arm64, Arch::X86_64] {
        assert_eq!(Arch::from_wire(arch.as_str()), Some(arch));
    }
    assert_eq!(Os::from_wire("MacOS"), None);
    assert_eq!(Os::from_wire("freebsd"), None);
    assert_eq!(Arch::from_wire("aarch64"), None);
    assert_eq!(Channel::from_wire("release"), Channel::Release);
    assert_eq!(Channel::from_wire("source"), Channel::Source);
    assert_eq!(Channel::from_wire("tap"), Channel::Named("tap".into()));
}
