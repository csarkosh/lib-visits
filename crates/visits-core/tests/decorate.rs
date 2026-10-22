use serde_json::Value;
use visits_core::{browser_of, decorate, is_crawler, os_of, split_referrer, validate_json, RequestFacts};

fn fixture(name: &str) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/fixtures/");
    serde_json::from_str(&std::fs::read_to_string(format!("{path}{name}.json")).unwrap()).unwrap()
}

#[test]
fn every_fixture_decorates_to_its_out() {
    for name in ["view", "view-internal", "leave", "click-named", "click-outbound", "vitals"] {
        let f = fixture(name);
        let facts = &f["facts"];
        let s = |k: &str| facts.get(k).and_then(Value::as_str);
        let event = validate_json(f["in"].to_string().as_bytes()).unwrap().remove(0);
        let secs = facts["unix_secs"].as_u64().unwrap();
        let out = decorate(event, &RequestFacts {
            unix_secs: secs,
            unix_millis: facts.get("unix_millis").and_then(Value::as_u64).unwrap_or(secs * 1000),
            ip: s("ip"),
            user_agent: s("user_agent"),
            origin_host: s("origin_host").unwrap(),
            accept_language: s("accept_language"),
            sec_ch_ua: s("sec_ch_ua"),
            sec_ch_ua_platform: s("sec_ch_ua_platform"),
            country: s("country").map(String::from),
            secret: s("secret").map(str::as_bytes),
        });
        assert_eq!(serde_json::to_value(&out).unwrap(), f["out"], "{name}");
    }
}

#[test]
fn browsers_and_systems_from_user_agents() {
    let edge = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.2849.56";
    assert_eq!(browser_of(edge, None), ("Edge".into(), Some(130)));
    assert_eq!(os_of(edge, None), "Windows");
    let safari = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15";
    assert_eq!(browser_of(safari, None), ("Safari".into(), Some(17)));
    assert_eq!(os_of(safari, None), "macOS");
    let android = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36";
    assert_eq!(browser_of(android, None), ("Chrome".into(), Some(130)));
    assert_eq!(os_of(android, None), "Android");
    assert_eq!(os_of("Mozilla/5.0 (X11; CrOS x86_64 14541.0.0)", None), "ChromeOS");
    assert_eq!(browser_of("curl/8.4.0", None), ("other".into(), None));
    assert_eq!(os_of("curl/8.4.0", None), "other");
    assert_eq!(browser_of("", Some("\"Not/A)Brand\";v=\"8\", \"Chromium\";v=\"126\", \"Brave\";v=\"126\"")), ("Brave".into(), Some(126)));
    assert_eq!(browser_of("", Some("\"Chromium\";v=\"126\", \"Not/A)Brand\";v=\"8\"")), ("Chromium".into(), Some(126)));
    assert_eq!(os_of("", Some("\"Chrome OS\"")), "ChromeOS");
}

#[test]
fn crawlers_and_referrers() {
    assert!(is_crawler("Mozilla/5.0 (compatible; bingbot/2.0)"));
    assert!(is_crawler("Chrome-Lighthouse"));
    assert!(!is_crawler("Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0"));
    assert_eq!(split_referrer("https://www.Example.com:8443/a/b?q#f"), Some(("example.com".into(), "/a/b".into())));
    assert_eq!(split_referrer("android-app://com.google.android.gm/"), Some(("com.google.android.gm".into(), "/".into())));
    assert_eq!(split_referrer("https://host"), Some(("host".into(), "/".into())));
    assert_eq!(split_referrer(""), None);
    assert_eq!(split_referrer("not a url"), None);
}

#[test]
fn an_unparseable_referrer_is_external_without_rh() {
    let f = fixture("view");
    let mut input = f["in"].clone();
    input["r"] = Value::from("garbage");
    let event = validate_json(input.to_string().as_bytes()).unwrap().remove(0);
    let facts = &f["facts"];
    let out = decorate(event, &RequestFacts {
        unix_secs: facts["unix_secs"].as_u64().unwrap(),
        unix_millis: facts["unix_secs"].as_u64().unwrap() * 1000,
        ip: None,
        user_agent: None,
        origin_host: "example.com",
        accept_language: None,
        sec_ch_ua: None,
        sec_ch_ua_platform: None,
        country: None,
        secret: None,
    });
    let s = serde_json::to_value(&out).unwrap()["s"].clone();
    assert_eq!(s["rk"], "external");
    assert!(s.get("rh").is_none());
}

#[test]
fn a_headless_brand_in_the_client_hints_is_a_bot_under_a_phone_user_agent() {
    let event = validate_json(br#"{"v":1,"n":"view","id":"ipv00vn5r7","p":"/apps"}"#).unwrap().remove(0);
    let out = decorate(event, &RequestFacts {
        unix_secs: 1_792_134_519,
        unix_millis: 1_792_134_519_586,
        ip: None,
        user_agent: Some("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1"),
        origin_host: "csarko.sh",
        accept_language: None,
        sec_ch_ua: Some("\"HeadlessChrome\";v=\"138\", \"Chromium\";v=\"138\", \"Not)A;Brand\";v=\"24\""),
        sec_ch_ua_platform: None,
        country: None,
        secret: None,
    });
    let s = &serde_json::to_value(&out).unwrap()["s"];
    assert_eq!(s["bn"], "HeadlessChrome");
    assert_eq!(s["bot"], true);
}
