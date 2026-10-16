use serde_json::{json, Value};
use visits_core::{app_of, channel_of, decorate_report, kind_of, validate_check, validate_report, Invalid};

fn fixture(name: &str) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/fixtures/app/");
    serde_json::from_str(&std::fs::read_to_string(format!("{path}{name}.json")).unwrap()).unwrap()
}

/// The check fixture's query as the JSON value the walk builds from it.
fn check_value() -> Value {
    json!({ "v": 2, "app": "arkos", "ver": "0.5.0", "bin": "daemon", "os": "macos", "osv": 15, "arch": "arm64", "ch": "release", "k": "check", "rd": -1 })
}

#[test]
fn every_valid_report_round_trips_to_the_same_value() {
    for name in ["session", "crash"] {
        let f = fixture(name);
        let r = validate_report(f.to_string().as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(serde_json::to_value(&r).unwrap(), f, "{name}");
        assert_eq!(app_of(&r), "arkos");
        assert_eq!(channel_of(&r), "release");
        assert_eq!(kind_of(&r), name);
    }
}

#[test]
fn the_check_query_is_coerced_by_the_field_table() {
    let q = fixture("check")["query"].as_str().unwrap().to_string();
    let r = validate_check(&q).unwrap();
    assert_eq!(serde_json::to_value(&r).unwrap(), check_value());
    assert_eq!(kind_of(&r), "check");
    assert_eq!(app_of(&r), "arkos");
}

#[test]
fn every_invalid_entry_is_refused_with_its_reason() {
    let all = fixture("invalid");
    let mut wrong = Vec::new();
    for case in all.as_array().unwrap() {
        let reason = case["reason"].as_str().unwrap();
        let err = match (case.get("report"), case.get("check")) {
            (Some(body), None) => validate_report(body.to_string().as_bytes()).err(),
            (None, Some(Value::String(q))) => validate_check(q).err(),
            _ => panic!("an entry carries a report or a check: {case}"),
        };
        match err {
            None => wrong.push(format!("accepted, expected {reason}")),
            Some(err) if err.to_string() != reason => wrong.push(format!("{err}, expected {reason}")),
            Some(err) => {
                let status = if err == Invalid::Channel { 403 } else { 400 };
                assert_eq!(case["status"], status, "{reason}");
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn an_array_body_is_malformed() {
    let body = Value::Array(vec![fixture("session")]).to_string();
    assert_eq!(validate_report(body.as_bytes()), Err(Invalid::Json));
    assert_eq!(validate_report(b"{"), Err(Invalid::Json));
    assert_eq!(validate_report(&[0xff, 0xfe]), Err(Invalid::Json));
}

#[test]
fn the_new_refusals_read_as_the_plan_spells_them() {
    assert_eq!(Invalid::Unknown("ev.x".into()).to_string(), "unknown key: ev.x");
    assert_eq!(Invalid::Pattern("images.uuid".into()).to_string(), "bad value: images.uuid");
    assert_eq!(Invalid::Channel.to_string(), "source channel");
    assert_eq!(Invalid::Kind.to_string(), "wrong kind for this route");
}

/// A valid session with one key replaced, for the cases the fixtures leave out.
fn session_with(key: &str, value: Value) -> Result<visits_core::AppReport, Invalid> {
    let mut f = fixture("session");
    f.as_object_mut().unwrap().insert(key.into(), value);
    validate_report(f.to_string().as_bytes())
}

fn crash_with(key: &str, value: Value) -> Result<visits_core::AppReport, Invalid> {
    let mut f = fixture("crash");
    f.as_object_mut().unwrap().insert(key.into(), value);
    validate_report(f.to_string().as_bytes())
}

#[test]
fn nested_paths_and_counts_are_named_without_an_index() {
    assert_eq!(session_with("focus", json!({ "terminal": -1 })), Err(Invalid::Pattern("focus.terminal".into())));
    assert_eq!(session_with("focus", json!({ "terminal": "1" })), Err(Invalid::Type("focus.terminal".into())));
    let seventeen: serde_json::Map<String, Value> = (0..17).map(|i| (format!("s{i}"), json!(1))).collect();
    assert_eq!(session_with("focus", Value::Object(seventeen)), Err(Invalid::Keys("focus".into())));
    assert_eq!(session_with("ev", json!([{ "n": "launch", "t": 0, "x": 1 }])), Err(Invalid::Unknown("ev.x".into())));
    assert_eq!(session_with("ev", json!([{ "t": 0 }])), Err(Invalid::Missing("ev.n")));
    assert_eq!(session_with("ev", json!([{ "n": "a", "t": 0, "d": { "f": 1.5 } }])), Err(Invalid::Type("ev.d.f".into())));
    assert!(session_with("ev", Value::Array(vec![json!({ "n": "tick", "t": 1 }); 200])).is_ok());
    assert_eq!(crash_with("frames", json!([[0, -1]])), Err(Invalid::Pattern("frames".into())));
    assert_eq!(crash_with("frames", Value::Array(vec![json!([0, 1]); 65])), Err(Invalid::Keys("frames".into())));
    assert_eq!(crash_with("images", json!([["a b", "0123456789abcdef0123456789abcdef", 1]])), Err(Invalid::Pattern("images.name".into())));
    assert_eq!(crash_with("images", json!([["a", "0123456789ABCDEF0123456789abcdef", 1]])), Err(Invalid::Pattern("images.uuid".into())));
    assert_eq!(crash_with("images", json!([["a", "0123456789abcdef0123456789abcdef", "1"]])), Err(Invalid::Type("images.size".into())));
    assert_eq!(crash_with("mh", json!("ab")), Err(Invalid::Pattern("mh".into())));
    assert_eq!(crash_with("file", json!("/src/a b.rs")), Err(Invalid::Pattern("file".into())));
    assert_eq!(crash_with("sid", json!("3f9a1c27b84d5e60")), Err(Invalid::Unknown("sid".into())));
}

#[test]
fn the_version_pattern_is_matched_by_hand() {
    for good in ["0.5.0", "10.20.30", "1.2.3-beta.1", "1.2.3+build-7"] {
        assert!(session_with("ver", json!(good)).is_ok(), "{good}");
    }
    for bad in ["0.5", "1.2.3.", "1.2.3-", "a.b.c", "1..3", "1.2.3_x", "1.2.3-a+b", "1.2.3-0123456789012345678901234567"] {
        assert_eq!(session_with("ver", json!(bad)), Err(Invalid::Pattern("ver".into())), "{bad}");
    }
}

#[test]
fn source_is_refused_only_once_the_rest_is_valid() {
    assert_eq!(session_with("ch", json!("source")), Err(Invalid::Channel));
    let mut f = fixture("session");
    f["ch"] = json!("source");
    f["sid"] = json!("xyz");
    assert_eq!(validate_report(f.to_string().as_bytes()), Err(Invalid::Pattern("sid".into())));
    let q = fixture("check")["query"].as_str().unwrap().replace("ch=release", "ch=source");
    assert_eq!(validate_check(&q), Err(Invalid::Channel));
}

#[test]
fn a_check_refuses_what_a_report_would() {
    let q = fixture("check")["query"].as_str().unwrap().to_string();
    assert_eq!(validate_check(&format!("{q}&s=1")), Err(Invalid::Unknown("s".into())));
    assert_eq!(validate_check(&format!("{q}&time=1")), Err(Invalid::Unknown("time".into())));
    assert_eq!(validate_check(&q.replace("v=2", "v=3")), Err(Invalid::Version));
    assert_eq!(validate_check(&q.replace("osv=15", "osv=fifteen")), Err(Invalid::Type("osv".into())));
    assert_eq!(validate_check(&format!("{q}&app=arkos")), Err(Invalid::Pattern("app".into())));
}

#[test]
fn decoration_adds_s_and_time_and_nothing_else() {
    for name in ["session", "crash"] {
        let f = fixture(name);
        let r = decorate_report(validate_report(f.to_string().as_bytes()).unwrap(), 1_792_032_030_250);
        let s = r.s.as_ref().unwrap();
        assert_eq!(s.sv, 2);
        assert_eq!(s.at, "2026-10-15T02:40:30Z");
        assert_eq!(r.time.as_deref(), Some("2026-10-15T02:40:30.250Z"));
        let line = serde_json::to_value(&r).unwrap();
        let mut expected = f.clone();
        expected["s"] = json!({ "sv": 2, "at": "2026-10-15T02:40:30Z" });
        expected["time"] = json!("2026-10-15T02:40:30.250Z");
        assert_eq!(line, expected, "{name}: the report's keys plus s and time");
    }
    let q = fixture("check")["query"].as_str().unwrap().to_string();
    let line = serde_json::to_value(decorate_report(validate_check(&q).unwrap(), 0)).unwrap();
    let mut expected = check_value();
    expected["s"] = json!({ "sv": 2, "at": "1970-01-01T00:00:00Z" });
    expected["time"] = json!("1970-01-01T00:00:00.000Z");
    assert_eq!(line, expected);
}

#[test]
fn iw_is_a_whole_number_of_weeks() {
    assert!(session_with("iw", json!(0)).is_ok());
    assert!(session_with("iw", json!(20622)).is_ok());
    assert_eq!(session_with("iw", json!(20630)), Err(Invalid::Pattern("iw".into())));
    assert_eq!(session_with("iw", json!(-7)), Err(Invalid::Pattern("iw".into())));
    assert_eq!(session_with("iw", json!("14")), Err(Invalid::Type("iw".into())));
}

#[test]
fn a_d_integer_is_any_i64() {
    for n in [json!(-5), json!(i64::MIN), json!(i64::MAX)] {
        assert!(session_with("ev", json!([{ "n": "a", "t": 0, "d": { "delta": n } }])).is_ok(), "{n}");
    }
    let above = json!([{ "n": "a", "t": 0, "d": { "delta": u64::MAX } }]);
    assert_eq!(session_with("ev", above), Err(Invalid::Pattern("ev.d.delta".into())));
}

#[test]
fn keys_and_strings_outside_their_pattern_are_bad_values() {
    assert_eq!(session_with("ev", json!([{ "n": "a", "t": 0, "d": { "X": 1 } }])), Err(Invalid::Pattern("ev.d.X".into())));
    assert_eq!(session_with("focus", json!({ "Bad": 1 })), Err(Invalid::Pattern("focus.Bad".into())));
    assert_eq!(session_with("ev", json!([{ "n": "café", "t": 0 }])), Err(Invalid::Pattern("ev.n".into())));
    assert_eq!(session_with("ev", json!([{ "n": "a", "t": 0, "d": { "v": "café" } }])), Err(Invalid::Pattern("ev.d.v".into())));
    // Thirty-two characters but thirty-three bytes: refused by the pattern, whatever the count.
    let wide = format!("{}é", "a".repeat(31));
    assert_eq!(wide.chars().count(), 32);
    assert_eq!(session_with("ev", json!([{ "n": wide, "t": 0 }])), Err(Invalid::Pattern("ev.n".into())));
    assert!(session_with("ev", json!([{ "n": "a".repeat(32), "t": 0 }])).is_ok());
    assert_eq!(session_with("ev", json!([{ "n": "a".repeat(33), "t": 0 }])), Err(Invalid::Pattern("ev.n".into())));
}

#[test]
fn counts_types_and_integer_range() {
    let image = json!(["a", "0123456789abcdef0123456789abcdef", 1]);
    assert!(crash_with("images", Value::Array(vec![image.clone(); 32])).is_ok());
    assert_eq!(crash_with("images", Value::Array(vec![image; 33])), Err(Invalid::Keys("images".into())));
    assert_eq!(session_with("ev", json!(["launch"])), Err(Invalid::Type("ev".into())));
    assert_eq!(session_with("osv", json!(1.5)), Err(Invalid::Type("osv".into())));
    assert!(session_with("osv", json!(u64::MAX)).is_ok());
}

#[test]
fn query_integers_read_as_a_body_would() {
    let base = "v=2&app=arkos&ver=0.5.0&bin=daemon&os=macos&arch=arm64&ch=release&k=check";
    assert_eq!(validate_check(&format!("{base}&osv=15&rd")), Err(Invalid::Type("rd".into())));
    assert_eq!(validate_check(&format!("{base}&osv=15&rd=")), Err(Invalid::Type("rd".into())));
    assert_eq!(validate_check(&format!("{base}&osv=15&rd=%2B5")), Err(Invalid::Type("rd".into())));
    assert_eq!(validate_check(&format!("{base}&osv=15&rd=-")), Err(Invalid::Type("rd".into())));
    assert_eq!(validate_check(&format!("{base}&osv=1.5&rd=0")), Err(Invalid::Type("osv".into())));
    let r = validate_check(&format!("{base}&osv={}&rd=0", u64::MAX)).unwrap();
    assert_eq!(r.fields["osv"], json!(u64::MAX));
    assert_eq!(validate_check(&format!("{base}&osv=99999999999999999999&rd=0")), Err(Invalid::Type("osv".into())));
}

#[test]
fn a_repeated_unknown_query_key_is_named_as_unknown() {
    let q = fixture("check")["query"].as_str().unwrap().to_string();
    assert_eq!(validate_check(&format!("{q}&zz=1&zz=2")), Err(Invalid::Unknown("zz".into())));
}

#[test]
fn the_public_matchers_are_the_walks_own() {
    use visits_core::app::{is_hex, is_name, is_value};
    assert!(is_name("pane_open") && is_name("a") && is_name(&"a".repeat(32)));
    assert!(!is_name("") && !is_name("Pane") && !is_name("1a") && !is_name("pane-open") && !is_name(&"a".repeat(33)));
    assert!(is_value("") && is_value("a/b:c.d-e_F9") && is_value(&"a".repeat(64)));
    assert!(!is_value("a b") && !is_value("a+b") && !is_value(&"a".repeat(65)));
    assert!(is_hex("3f9a1c27b84d5e60", 16, 32) && is_hex(&"a".repeat(32), 16, 32));
    assert!(!is_hex("3F9A1C27B84D5E60", 16, 32) && !is_hex("../x", 16, 32) && !is_hex(&"a".repeat(33), 16, 32));
}
