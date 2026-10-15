//! The server end to end: a real listener, a raw HTTP client, a capturing sink.
use std::future::IntoFuture;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use visits::app::{router, AppState};
use visits::limiter::Limiter;
use visits::sink::Sink;
use visits_core::Event;

#[derive(Default)]
struct Capture(Mutex<Vec<Event>>);
impl Sink for Capture {
    fn write(&self, events: &[Event]) -> std::io::Result<()> { self.0.lock().unwrap().extend_from_slice(events); Ok(()) }
}

struct Server { addr: String, captured: Arc<Capture> }

async fn start(secret: Option<&str>, trusted_proxies: usize) -> Server {
    let captured = Arc::new(Capture::default());
    let state = Arc::new(AppState {
        origins: ["https://example.com".to_string()].into_iter().collect(),
        path: "/v".into(),
        secret: secret.map(|s| s.as_bytes().to_vec()),
        sink: Box::new(SharedSink(captured.clone())),
        limiter: Mutex::new(Limiter::new(3.0, 0.0)),
        trusted_proxies,
        log_ip: false,
        geoip: None,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(axum::serve(listener, router(state).into_make_service_with_connect_info::<std::net::SocketAddr>()).into_future());
    Server { addr, captured }
}

struct SharedSink(Arc<Capture>);
impl Sink for SharedSink {
    fn write(&self, events: &[Event]) -> std::io::Result<()> { self.0.write(events) }
}

fn request(addr: &str, method: &str, target: &str, headers: &[(&str, &str)], body: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    let mut req = format!("{method} {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers { req.push_str(&format!("{k}: {v}\r\n")); }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
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
    let got = srv.captured.0.lock().unwrap();
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
    let got = srv.captured.0.lock().unwrap();
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
    assert_eq!(request(&srv.addr, "POST", "/v", &[origin], &big).0, 413);
    let fifty_one = format!("[{}]", vec![VIEW; 51].join(","));
    assert_eq!(request(&srv.addr, "POST", "/v", &[origin], &fifty_one).0, 413);
    let (status, head, _) = request(&srv.addr, "GET", "/healthcheck", &[], "");
    assert_eq!(status, 200);
    assert!(head.contains("content-type: text/plain"));
    assert_eq!(request(&srv.addr, "GET", "/nope", &[], "").0, 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_limiter_answers_429_after_the_burst_and_keys_unknown_addresses_together() {
    let srv = start(None, 1).await;
    let h = [("Origin", "https://example.com"), ("X-Forwarded-For", "  , garbage")];
    for _ in 0..3 { assert_eq!(request(&srv.addr, "POST", "/v", &h, VIEW).0, 204); }
    let (status, head, _) = request(&srv.addr, "POST", "/v", &h, VIEW);
    assert_eq!(status, 429);
    assert!(head.contains("retry-after: 1"));
    let other = [("Origin", "https://example.com"), ("X-Forwarded-For", "198.51.100.1")];
    assert_eq!(request(&srv.addr, "POST", "/v", &other, VIEW).0, 204, "a real address has its own bucket");
    assert!(srv.captured.0.lock().unwrap()[0].s.as_ref().unwrap().visit.is_none());
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
