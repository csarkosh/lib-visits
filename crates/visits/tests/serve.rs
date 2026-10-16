//! The server end to end: a real listener, a raw HTTP client, a capturing sink.
use std::collections::BTreeMap;
use std::future::IntoFuture;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use visits::app::{router, AppState};
use visits::ignore::IpNet;
use visits::limiter::Limiter;
use visits::sink::Sink;
use visits_core::{AppReport, Event};

#[derive(Default)]
struct Capture { events: Mutex<Vec<Event>>, reports: Mutex<Vec<AppReport>> }
impl Sink for Capture {
    fn write(&self, events: &[Event]) -> std::io::Result<()> { self.events.lock().unwrap().extend_from_slice(events); Ok(()) }
    fn write_report(&self, r: &AppReport) -> std::io::Result<()> { self.reports.lock().unwrap().push(r.clone()); Ok(()) }
}

struct Server { addr: String, captured: Arc<Capture> }

async fn start(secret: Option<&str>, trusted_proxies: usize) -> Server {
    start_with(secret, trusted_proxies, vec![]).await
}

async fn start_with(secret: Option<&str>, trusted_proxies: usize, ignore: Vec<IpNet>) -> Server {
    launch(&["https://example.com"], BTreeMap::new(), Limiter::new(3.0, 0.0), secret, trusted_proxies, ignore).await
}

/// A collector for installed programs only: no origin, so no beacon route, and
/// one trusted proxy, so `X-Forwarded-For` names the client.
async fn start_apps(apps: BTreeMap<String, Option<String>>, limiter: Limiter) -> Server {
    launch(&[], apps, limiter, None, 1, vec![]).await
}

async fn launch(
    origins: &[&str],
    apps: BTreeMap<String, Option<String>>,
    limiter: Limiter,
    secret: Option<&str>,
    trusted_proxies: usize,
    ignore: Vec<IpNet>,
) -> Server {
    let captured = Arc::new(Capture::default());
    let state = Arc::new(AppState {
        origins: origins.iter().map(|o| o.to_string()).collect(),
        path: "/v".into(),
        secret: secret.map(|s| s.as_bytes().to_vec()),
        sink: Box::new(SharedSink(captured.clone())),
        limiter: Mutex::new(limiter),
        trusted_proxies,
        log_ip: false,
        geoip: None,
        ignore,
        apps,
        app_path: "/a".into(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(axum::serve(listener, router(state).into_make_service_with_connect_info::<std::net::SocketAddr>()).into_future());
    Server { addr, captured }
}

struct SharedSink(Arc<Capture>);
impl Sink for SharedSink {
    fn write(&self, events: &[Event]) -> std::io::Result<()> { self.0.write(events) }
    fn write_report(&self, r: &AppReport) -> std::io::Result<()> { self.0.write_report(r) }
}

fn request(addr: &str, method: &str, target: &str, headers: &[(&str, &str)], body: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut req = format!("{method} {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers { req.push_str(&format!("{k}: {v}\r\n")); }
    req.push_str("\r\n");
    req.push_str(body);
    let _ = s.write_all(req.as_bytes());
    let mut out = String::new();
    if let Err(e) = s.read_to_string(&mut out) {
        assert!(!out.is_empty(), "no response ({e})");
    }
    assert!(!out.is_empty(), "no response");
    let status: u16 = out[9..12].parse().unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    (status, head.to_lowercase(), body.to_string())
}

const VIEW: &str = r#"{"v":1,"n":"view","id":"abc","p":"/","r":"https://www.google.com/"}"#;
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepts_a_post_decorates_it_and_writes_it() {
    let srv = start(Some("s"), 1).await;
    let (status, head, _) = request(&srv.addr, "POST", "/v", &[("Origin", "https://example.com"), ("User-Agent", UA), ("X-Forwarded-For", "203.0.113.9, 10.0.0.1")], VIEW);
    assert_eq!(status, 204);
    assert!(head.contains("cache-control: no-store"));
    assert!(head.contains("access-control-allow-origin: https://example.com"));
    let got = srv.captured.events.lock().unwrap();
    let s = got[0].s.as_ref().unwrap();
    assert_eq!(s.host, "example.com");
    assert_eq!(s.bn, "Firefox");
    assert_eq!(s.rk, "external");
    assert_eq!(s.visit.as_ref().unwrap().len(), 16);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_get_beacon_with_a_referer_and_no_origin_is_the_same_event() {
    let srv = start(None, 0).await;
    let (status, _, _) = request(&srv.addr, "GET", "/v?v=1&n=view&id=abc&p=%2F&sw=100", &[("Referer", "https://example.com/page"), ("User-Agent", UA)], "");
    assert_eq!(status, 204);
    let got = srv.captured.events.lock().unwrap();
    assert_eq!(got[0].fields["sw"], 100);
    assert!(got[0].s.as_ref().unwrap().visit.is_none(), "no secret, no visit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refusals_carry_the_spec_statuses() {
    let srv = start(Some("s"), 0).await;
    let origin = ("Origin", "https://example.com");
    assert_eq!(request(&srv.addr, "POST", "/v", &[("Origin", "https://evil.example")], VIEW).0, 403);
    assert_eq!(request(&srv.addr, "POST", "/v", &[], VIEW).0, 403);
    let (status, _, body) = request(&srv.addr, "POST", "/v", &[origin], "{");
    assert_eq!((status, body.as_str()), (400, r#"{"error":"malformed json"}"#));
    assert_eq!(request(&srv.addr, "PUT", "/v", &[origin], VIEW).0, 405);
    let big = format!(r#"{{"v":1,"n":"view","id":"abc","p":"/","t":"{}"}}"#, "x".repeat(17_000));
    let (status, head, _) = request(&srv.addr, "POST", "/v", &[origin], &big);
    assert_eq!(status, 413);
    assert!(head.contains("cache-control: no-store"), "oversize 413: {head}");
    let fifty_one = format!("[{}]", vec![VIEW; 51].join(","));
    assert_eq!(request(&srv.addr, "POST", "/v", &[origin], &fifty_one).0, 413);
    let (status, head, _) = request(&srv.addr, "GET", "/healthcheck", &[], "");
    assert_eq!(status, 200);
    assert!(head.contains("content-type: text/plain"));
    assert!(head.contains("cache-control: no-store"), "healthcheck: {head}");
    let (status, head, _) = request(&srv.addr, "GET", "/nope", &[], "");
    assert_eq!(status, 404);
    assert!(head.contains("cache-control: no-store"), "404: {head}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_limiter_answers_429_after_the_burst_and_keys_unknown_addresses_together() {
    let srv = start(Some("s"), 1).await;
    let h = [("Origin", "https://example.com"), ("X-Forwarded-For", "  , garbage")];
    for _ in 0..3 { assert_eq!(request(&srv.addr, "POST", "/v", &h, VIEW).0, 204); }
    let (status, head, _) = request(&srv.addr, "POST", "/v", &h, VIEW);
    assert_eq!(status, 429);
    assert!(head.contains("retry-after: 1"));
    let other = [("Origin", "https://example.com"), ("X-Forwarded-For", "198.51.100.1")];
    assert_eq!(request(&srv.addr, "POST", "/v", &other, VIEW).0, 204, "a real address has its own bucket");
    assert!(srv.captured.events.lock().unwrap()[0].s.as_ref().unwrap().visit.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ignored_address_is_answered_and_dropped() {
    let srv = start_with(Some("s"), 1, vec![IpNet::parse("203.0.113.0/24").unwrap(), IpNet::parse("2001:db8::/32").unwrap()]).await;
    let four = [("Origin", "https://example.com"), ("X-Forwarded-For", "203.0.113.9")];
    let six = [("Origin", "https://example.com"), ("X-Forwarded-For", "2001:db8:1::5")];
    let other = [("Origin", "https://example.com"), ("X-Forwarded-For", "198.51.100.1")];
    let (status, head, _) = request(&srv.addr, "POST", "/v", &four, VIEW);
    assert_eq!(status, 204);
    assert!(head.contains("access-control-allow-origin: https://example.com"));
    assert_eq!(request(&srv.addr, "POST", "/v", &six, VIEW).0, 204);
    assert!(srv.captured.events.lock().unwrap().is_empty(), "nothing written for an ignored address");
    assert_eq!(request(&srv.addr, "POST", "/v", &other, VIEW).0, 204);
    assert_eq!(srv.captured.events.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_line_time_is_the_client_clock_within_five_minutes_else_receipt() {
    use std::time::{SystemTime, UNIX_EPOCH};
    use visits_core::rfc3339_ms;
    let srv = start(None, 0).await;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    let origin = [("Origin", "https://example.com")];
    let near = format!(r#"{{"v":1,"n":"view","id":"a","p":"/","ts":{}}}"#, now - 4_000);
    let far = format!(r#"{{"v":1,"n":"view","id":"b","p":"/","ts":{}}}"#, now - 3_600_000);
    assert_eq!(request(&srv.addr, "POST", "/v", &origin, &near).0, 204);
    assert_eq!(request(&srv.addr, "POST", "/v", &origin, &far).0, 204);
    let got = srv.captured.events.lock().unwrap();
    assert_eq!(got[0].time.as_deref(), Some(rfc3339_ms(now - 4_000).as_str()));
    let receipt = got[1].time.as_deref().unwrap();
    assert!(receipt >= rfc3339_ms(now - 1_000).as_str() && receipt <= rfc3339_ms(now + 60_000).as_str(), "{receipt}");
}

#[test]
fn client_ip_counts_from_the_right() {
    use std::net::{IpAddr, Ipv4Addr};
    use visits::app::client_ip;
    let peer = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
    assert_eq!(client_ip(Some("203.0.113.9, 10.0.0.2"), peer, 0), Some(peer));
    assert_eq!(client_ip(Some("203.0.113.9, 10.0.0.2"), peer, 1), "10.0.0.2".parse().ok());
    assert_eq!(client_ip(Some("203.0.113.9, 10.0.0.2"), peer, 2), "203.0.113.9".parse().ok());
    assert_eq!(client_ip(Some("203.0.113.9"), peer, 2), None, "fewer entries than proxies: unknown, never the peer");
    assert_eq!(client_ip(Some("  , garbage"), peer, 1), None);
    assert_eq!(client_ip(None, peer, 1), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_fixture_is_accepted_and_decorated() {
    use serde_json::Value;
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/fixtures/");
    for name in ["view", "view-internal", "leave", "click-named", "click-outbound", "vitals"] {
        let f: Value = serde_json::from_str(&std::fs::read_to_string(format!("{dir}{name}.json")).unwrap()).unwrap();
        let facts = &f["facts"];
        let s = |k: &str| facts.get(k).and_then(Value::as_str);
        let srv = start(Some("test-secret"), 1).await;
        // One trusted proxy: the client is the last X-Forwarded-For entry, the one the proxy appended.
        let xff = s("ip").unwrap().to_string();
        let mut headers = vec![("Origin", "https://example.com"), ("User-Agent", s("user_agent").unwrap()), ("X-Forwarded-For", xff.as_str())];
        if let Some(v) = s("accept_language") { headers.push(("Accept-Language", v)); }
        if let Some(v) = s("sec_ch_ua") { headers.push(("Sec-CH-UA", v)); }
        if let Some(v) = s("sec_ch_ua_platform") { headers.push(("Sec-CH-UA-Platform", v)); }
        let (status, _, _) = request(&srv.addr, "POST", "/v", &headers, &f["in"].to_string());
        assert_eq!(status, 204, "{name}");
        let got = srv.captured.events.lock().unwrap();
        assert_eq!(got.len(), 1, "{name}");
        let mut line = serde_json::to_value(&got[0]).unwrap();
        let mut want = f["out"].clone();
        // The fixture's ts is years from now, so the line's time is the time of receipt.
        let time = line["time"].as_str().unwrap_or_else(|| panic!("{name}: time"));
        assert!(time.len() == 24 && time.ends_with('Z') && time != want["time"], "{name}: {time}");
        line["time"] = want["time"].clone();
        let (ls, ws) = (line["s"].as_object_mut().unwrap(), want["s"].as_object_mut().unwrap());
        ls.insert("at".into(), ws["at"].clone());
        match ws.remove("visit") {
            Some(_) => {
                let v = ls.get("visit").and_then(Value::as_str).unwrap_or_else(|| panic!("{name}: visit"));
                assert!(v.len() == 16 && v.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{name}: {v}");
            }
            None => {}
        }
        ls.remove("visit");
        ls.remove("cc");
        ws.remove("cc");
        assert_eq!(line, want, "{name}");
    }
}

const ARKOS_UA: &str = "arkos/0.5.0";
const ARKOS_LATEST: &str = "https://example.com/arkos/latest";

/// Two allowed programs: `arkos` with an update page, `zed` without one.
fn programs() -> BTreeMap<String, Option<String>> {
    BTreeMap::from([("arkos".to_string(), Some(ARKOS_LATEST.to_string())), ("zed".to_string(), None)])
}

fn app_fixture(name: &str) -> String {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/fixtures/app/");
    std::fs::read_to_string(format!("{dir}{name}")).unwrap()
}

fn session() -> String {
    app_fixture("session.json")
}

fn check_query() -> String {
    let f: serde_json::Value = serde_json::from_str(&app_fixture("check.json")).unwrap();
    f["query"].as_str().unwrap().to_string()
}

/// The session fixture with one top-level key replaced.
fn session_with(key: &str, value: serde_json::Value) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&session()).unwrap();
    v[key] = value;
    v.to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_report_is_written_with_its_decoration_and_nothing_about_the_request() {
    let srv = start_apps(programs(), Limiter::new(1000.0, 0.0)).await;
    let h = [("User-Agent", ARKOS_UA), ("X-Forwarded-For", "203.0.113.9")];
    let (status, head, _) = request(&srv.addr, "POST", "/a", &h, &session());
    assert_eq!(status, 204);
    assert!(head.contains("cache-control: no-store"), "{head}");
    assert!(!head.contains("access-control-allow-origin"), "{head}");
    let got = srv.captured.reports.lock().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].s.as_ref().unwrap().sv, 2);
    assert_eq!(got[0].time.as_ref().unwrap().len(), 24);
    // The line exactly as the sink writes it.
    let line = serde_json::to_string(&got[0]).unwrap();
    for absent in ["203.0.113.9", ARKOS_UA, "visit", "host", "cc", "bn"] {
        assert!(!line.contains(absent), "{absent} in {line}");
    }
    let mut want: serde_json::Value = serde_json::from_str(&session()).unwrap();
    let mut line: serde_json::Value = serde_json::from_str(&line).unwrap();
    let decoration = line.as_object_mut().unwrap();
    assert!(decoration.remove("s").is_some() && decoration.remove("time").is_some());
    assert_eq!(line, want.take(), "the report's keys, s and time, nothing else");
    assert!(srv.captured.events.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_check_is_written_then_redirected() {
    let srv = start_apps(programs(), Limiter::new(1000.0, 0.0)).await;
    let (status, head, _) = request(&srv.addr, "GET", &format!("/a/check?{}", check_query()), &[("User-Agent", ARKOS_UA)], "");
    assert_eq!(status, 302);
    assert!(head.contains(&format!("location: {ARKOS_LATEST}")), "{head}");
    assert!(head.contains("cache-control: no-store"), "{head}");
    {
        let got = srv.captured.reports.lock().unwrap();
        assert_eq!(got.len(), 1, "written before the answer");
        assert_eq!(got[0].fields["k"], "check");
        assert_eq!(got[0].fields["rd"], -1);
        assert_eq!(got[0].s.as_ref().unwrap().sv, 2);
    }
    let zed = check_query().replace("app=arkos", "app=zed");
    let (status, head, _) = request(&srv.addr, "GET", &format!("/a/check?{zed}"), &[("User-Agent", "zed/1.2.3")], "");
    assert_eq!(status, 204, "no redirect configured");
    assert!(!head.contains("location"), "{head}");
    assert_eq!(srv.captured.reports.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ignored_address_is_redirected_and_not_written() {
    let ignore = vec![IpNet::parse("203.0.113.0/24").unwrap()];
    let srv = launch(&[], programs(), Limiter::new(1000.0, 0.0), None, 1, ignore).await;
    let h = [("User-Agent", ARKOS_UA), ("X-Forwarded-For", "203.0.113.9")];
    let (status, head, _) = request(&srv.addr, "GET", &format!("/a/check?{}", check_query()), &h, "");
    assert_eq!(status, 302);
    assert!(head.contains(&format!("location: {ARKOS_LATEST}")), "{head}");
    assert_eq!(request(&srv.addr, "POST", "/a", &h, &session()).0, 204);
    assert!(srv.captured.reports.lock().unwrap().is_empty(), "nothing written for an ignored address");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_gate_refuses_an_origin_and_any_agent_but_the_reports_own() {
    let srv = start_apps(programs(), Limiter::new(1000.0, 0.0)).await;
    let check = format!("/a/check?{}", check_query());
    let with_origin = [("User-Agent", ARKOS_UA), ("Origin", "https://example.com")];
    let (status, _, body) = request(&srv.addr, "POST", "/a", &with_origin, &session());
    assert_eq!((status, body.as_str()), (403, r#"{"error":"origin not allowed for reports"}"#));
    let (status, _, body) = request(&srv.addr, "GET", &check, &with_origin, "");
    assert_eq!((status, body.as_str()), (403, r#"{"error":"origin not allowed for reports"}"#));
    for agent in ["other/1.0.0", "arkos", "Mozilla/5.0 (Macintosh)", "zed/1.0.0"] {
        let (status, _, body) = request(&srv.addr, "POST", "/a", &[("User-Agent", agent)], &session());
        assert_eq!((status, body.as_str()), (403, r#"{"error":"app not allowed"}"#), "POST {agent}");
        let (status, _, body) = request(&srv.addr, "GET", &check, &[("User-Agent", agent)], "");
        assert_eq!((status, body.as_str()), (403, r#"{"error":"app not allowed"}"#), "GET {agent}");
    }
    assert_eq!(request(&srv.addr, "POST", "/a", &[], &session()).0, 403, "no agent");
    // A body naming an app the collector does not allow, under an allowed agent.
    let (status, _, body) = request(&srv.addr, "POST", "/a", &[("User-Agent", ARKOS_UA)], &session_with("app", "other".into()));
    assert_eq!((status, body.as_str()), (403, r#"{"error":"app not allowed"}"#));
    assert!(srv.captured.reports.lock().unwrap().is_empty());
    let (status, _, body) = request(&srv.addr, "POST", "/a", &[("User-Agent", ARKOS_UA)], &session_with("ch", "source".into()));
    assert_eq!((status, body.as_str()), (403, r#"{"error":"source channel"}"#));
    let source = check.replace("ch=release", "ch=source");
    let (status, _, body) = request(&srv.addr, "GET", &source, &[("User-Agent", ARKOS_UA)], "");
    assert_eq!((status, body.as_str()), (403, r#"{"error":"source channel"}"#));
    assert!(srv.captured.reports.lock().unwrap().is_empty(), "nothing refused is written");
    let detailed = [("User-Agent", "arkos/0.5.0 (macOS 15; arm64)")];
    assert_eq!(request(&srv.addr, "POST", "/a", &detailed, &session()).0, 204, "the agent may carry more after a space");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_invalid_fixture_answers_its_status_and_writes_nothing() {
    use serde_json::Value;
    let srv = start_apps(programs(), Limiter::new(1000.0, 0.0)).await;
    let entries: Vec<Value> = serde_json::from_str(&app_fixture("invalid.json")).unwrap();
    let h = [("User-Agent", ARKOS_UA)];
    for e in &entries {
        let reason = e["reason"].as_str().unwrap();
        let want = e["status"].as_u64().unwrap() as u16;
        let (status, head, body) = match (e.get("report"), e.get("check").and_then(Value::as_str)) {
            (Some(report), _) => request(&srv.addr, "POST", "/a", &h, &report.to_string()),
            (None, Some(query)) => request(&srv.addr, "GET", &format!("/a/check?{query}"), &h, ""),
            _ => panic!("{reason}: neither report nor check"),
        };
        assert_eq!(status, want, "{reason}");
        assert_eq!(body, serde_json::json!({ "error": reason }).to_string(), "{reason}");
        assert!(head.contains("cache-control: no-store"), "{reason}: {head}");
    }
    let (status, _, body) = request(&srv.addr, "POST", "/a", &h, &format!("[{}]", session()));
    assert_eq!((status, body.as_str()), (400, r#"{"error":"malformed json"}"#), "an array is malformed");
    assert!(srv.captured.reports.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_report_route_takes_64_kb() {
    let srv = start_apps(programs(), Limiter::new(1000.0, 0.0)).await;
    let h = [("User-Agent", ARKOS_UA)];
    let event = serde_json::json!({ "n": "command", "t": 1, "d": { "id": "x".repeat(64) } });
    let big = session_with("ev", serde_json::Value::Array(vec![event; 180]));
    assert!(big.len() > 17 * 1024 && big.len() < 64 * 1024, "{}", big.len());
    assert_eq!(request(&srv.addr, "POST", "/a", &h, &big).0, 204, "above the beacon's 16 KB");
    let huge = format!("{}{}", session(), " ".repeat(65 * 1024));
    let (status, head, _) = request(&srv.addr, "POST", "/a", &h, &huge);
    assert_eq!(status, 413);
    assert!(head.contains("cache-control: no-store"), "{head}");
    assert_eq!(srv.captured.reports.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_report_route_takes_one_method() {
    let srv = start_apps(programs(), Limiter::new(1000.0, 0.0)).await;
    let h = [("User-Agent", ARKOS_UA)];
    let (status, head, _) = request(&srv.addr, "GET", "/a", &h, "");
    assert_eq!(status, 405);
    assert!(head.contains("allow: post") && head.contains("cache-control: no-store"), "{head}");
    let (status, head, _) = request(&srv.addr, "POST", &format!("/a/check?{}", check_query()), &h, "");
    assert_eq!(status, 405);
    assert!(head.contains("allow: get") && head.contains("cache-control: no-store"), "{head}");
    assert_eq!(request(&srv.addr, "PUT", "/a", &h, &session()).0, 405);
    assert!(srv.captured.reports.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn beacons_and_reports_share_one_bucket() {
    let srv = launch(&["https://example.com"], programs(), Limiter::new(3.0, 0.0), None, 1, vec![]).await;
    let beacon = [("Origin", "https://example.com"), ("X-Forwarded-For", "198.51.100.7")];
    let report = [("User-Agent", ARKOS_UA), ("X-Forwarded-For", "198.51.100.7")];
    assert_eq!(request(&srv.addr, "POST", "/v", &beacon, VIEW).0, 204);
    assert_eq!(request(&srv.addr, "POST", "/v", &beacon, VIEW).0, 204);
    assert_eq!(request(&srv.addr, "POST", "/a", &report, &session()).0, 204);
    let (status, head, _) = request(&srv.addr, "POST", "/a", &report, &session());
    assert_eq!(status, 429);
    assert!(head.contains("retry-after: 1"), "{head}");
    let check = format!("/a/check?{}", check_query());
    assert_eq!(request(&srv.addr, "GET", &check, &report, "").0, 429);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_route_is_mounted_only_when_configured() {
    let web = start(None, 0).await;
    let h = [("User-Agent", ARKOS_UA)];
    assert_eq!(request(&web.addr, "POST", "/a", &h, &session()).0, 404, "no apps, no report route");
    assert_eq!(request(&web.addr, "GET", &format!("/a/check?{}", check_query()), &h, "").0, 404);
    let apps = start_apps(programs(), Limiter::new(1000.0, 0.0)).await;
    assert_eq!(request(&apps.addr, "POST", "/v", &[("Origin", "https://example.com")], VIEW).0, 404, "no origins, no beacon route");
    assert_eq!(request(&apps.addr, "GET", "/healthcheck", &[], "").0, 200);
}
