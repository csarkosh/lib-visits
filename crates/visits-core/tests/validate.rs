use serde_json::{json, Value};
use visits_core::{validate_json, validate_query, Invalid};

fn fixture(name: &str) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/fixtures/");
    serde_json::from_str(&std::fs::read_to_string(format!("{path}{name}.json")).unwrap()).unwrap()
}

#[test]
fn every_valid_fixture_parses_and_keeps_only_known_keys() {
    for name in ["view", "view-internal", "leave", "click-named", "click-outbound", "vitals"] {
        let f = fixture(name);
        let events = validate_json(f["in"].to_string().as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(events.len(), 1, "{name}");
        let back = serde_json::to_value(&events[0]).unwrap();
        let mut expected = f["out"].clone();
        expected.as_object_mut().unwrap().remove("s");
        expected.as_object_mut().unwrap().remove("time");
        assert_eq!(back, expected, "{name}: the event minus the decoration");
    }
}

#[test]
fn every_invalid_fixture_is_refused_with_its_reason() {
    for case in fixture("invalid").as_array().unwrap() {
        let body = match &case["in"] { Value::String(s) => s.clone(), v => v.to_string() };
        let err = validate_json(body.as_bytes()).err().unwrap_or_else(|| panic!("{} was accepted", case["name"]));
        assert_eq!(err.to_string(), case["error"], "{}", case["name"]);
    }
}

#[test]
fn an_array_is_capped_at_fifty() {
    let one = json!({ "v": 1, "n": "view", "id": "a", "p": "/" });
    let fifty = Value::Array(vec![one.clone(); 50]);
    assert_eq!(validate_json(fifty.to_string().as_bytes()).unwrap().len(), 50);
    let more = Value::Array(vec![one; 51]);
    assert!(matches!(validate_json(more.to_string().as_bytes()), Err(Invalid::TooMany)));
}

#[test]
fn a_query_string_is_coerced_by_the_key_table() {
    let events = validate_query("v=1&n=view&id=a&p=%2Fx&sw=1512&dpr=2.5&tp=true&d=%7B%22lcp%22%3A1%7D&junk=1").unwrap();
    let e = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(e["sw"], 1512);
    assert_eq!(e["dpr"], 2.5);
    assert_eq!(e["tp"], true);
    assert_eq!(e["d"], json!({ "lcp": 1 }));
    assert!(e.get("junk").is_none());
    assert_eq!(validate_query("v=1&n=view&id=a&p=/&sw=wide").unwrap_err().to_string(), "bad type: sw");
    assert_eq!(validate_query("v=1&n=x&id=a&p=/&d=notjson").unwrap_err().to_string(), "bad type: d");
}

#[test]
fn non_utf8_is_malformed() {
    assert!(matches!(validate_json(&[0xff, 0xfe]), Err(Invalid::Json)));
}
