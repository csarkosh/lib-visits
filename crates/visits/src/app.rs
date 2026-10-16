//! The router: the beacon route, the report and check routes for installed
//! programs, and the health route. A beacon is checked in order: method, origin,
//! address and limiter, body, then decorate and sink. A report or a check is
//! checked: method, address and limiter, agent, body, app and channel, then
//! decorate and sink; nothing about its request is written or printed.
use crate::ignore::IpNet;
use crate::limiter::Limiter;
use crate::sink::Sink;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, RawQuery, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use std::collections::{BTreeMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};

/// The proxy-depth warning is printed once per process.
static PROXY_WARNED: AtomicBool = AtomicBool::new(false);
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use visits_core::{
    app_of, channel_of, decorate, decorate_report, validate_check, validate_json, validate_query, validate_report, AppReport, Invalid,
    RequestFacts,
};

pub const MAX_BODY: usize = 16 * 1024;
/// The report route's own cap (section 6.1), set on its route inside the router's.
pub const MAX_REPORT: usize = 64 * 1024;

#[cfg(feature = "geoip")]
pub type GeoIp = maxminddb::Reader<Vec<u8>>;
#[cfg(not(feature = "geoip"))]
pub struct GeoIp;

pub struct AppState {
    pub origins: HashSet<String>,
    pub path: String,
    pub secret: Option<Vec<u8>>,
    pub sink: Box<dyn Sink>,
    pub limiter: Mutex<Limiter>,
    pub trusted_proxies: usize,
    pub log_ip: bool,
    pub geoip: Option<GeoIp>,
    /// Beacons from these ranges are answered 204 and never written.
    pub ignore: Vec<IpNet>,
    /// The programs allowed to report, each with the page its check redirects
    /// to, if any. Empty: no report route.
    pub apps: BTreeMap<String, Option<String>>,
    /// The report route; the check route is this followed by `/check`.
    pub app_path: String,
}

/// The beacon route is mounted when there is an origin to allow, the report
/// and check routes when there is an app to allow.
pub fn router(state: Arc<AppState>) -> Router {
    let mut r = Router::new().route("/healthcheck", get(|| async { ([(header::CONTENT_TYPE, "text/plain")], "ok") }));
    if !state.origins.is_empty() {
        r = r.route(&state.path, any(beacon));
    }
    if !state.apps.is_empty() {
        r = r
            .route(&state.app_path, any(report).layer(DefaultBodyLimit::max(MAX_REPORT)))
            .route(&format!("{}/check", state.app_path), any(check));
    }
    r.layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
        .layer(axum::middleware::map_response(no_store))
}

/// Every answer, the framework's own included, is `no-store`.
async fn no_store(mut res: Response) -> Response {
    res.headers_mut().entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("no-store"));
    res
}

/// The client address: the peer at 0 proxies, else that many entries from the
/// right of X-Forwarded-For; `None` when the header cannot answer.
pub fn client_ip(forwarded_for: Option<&str>, peer: IpAddr, trusted_proxies: usize) -> Option<IpAddr> {
    if trusted_proxies == 0 { return Some(peer); }
    let entries: Vec<&str> = forwarded_for?.split(',').map(str::trim).collect();
    if entries.len() < trusted_proxies { return None; }
    entries[entries.len() - trusted_proxies].parse().ok()
}

fn origin_of(headers: &HeaderMap, origins: &HashSet<String>) -> Option<String> {
    if let Some(o) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        return origins.contains(o).then(|| o.to_string());
    }
    let referer = headers.get(header::REFERER)?.to_str().ok()?;
    origins.iter().find(|o| referer.starts_with(o.as_str()) && referer[o.len()..].starts_with(['/', '?', '#']) || referer == *o).cloned()
}

fn host_of(origin: &str) -> &str {
    origin.split("://").nth(1).unwrap_or(origin)
}

/// The beacon's answer: 204, no-store, the origin allowed.
fn taken(origin: &str) -> Response {
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(origin) { res.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v); }
    res
}

fn refuse(status: StatusCode, reason: &str) -> Response {
    let body = serde_json::json!({ "error": reason }).to_string();
    (status, [(header::CACHE_CONTROL, "no-store"), (header::CONTENT_TYPE, "application/json")], body).into_response()
}

async fn beacon(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    if method != Method::POST && method != Method::GET {
        return (StatusCode::METHOD_NOT_ALLOWED, [(header::CACHE_CONTROL, "no-store"), (header::ALLOW, "GET, POST")]).into_response();
    }
    let Some(origin) = origin_of(&headers, &state.origins) else { return refuse(StatusCode::FORBIDDEN, "origin not allowed") };
    let h = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let ip = resolve_ip(&state, &headers, peer);
    if state.log_ip { eprintln!("visits: request from {}", ip.map(|i| i.to_string()).unwrap_or_else(|| "unknown".into())); }
    if ignored(&state, ip) { return taken(&origin); }
    if let Some(res) = limited(&state, ip) { return res; }
    let parsed = if method == Method::GET { validate_query(query.as_deref().unwrap_or("")) } else { validate_json(&body) };
    let events = match parsed {
        Ok(e) => e,
        Err(Invalid::TooMany) => return refuse(StatusCode::PAYLOAD_TOO_LARGE, "too many events"),
        Err(e) => return refuse(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let ip_text = ip.map(|i| i.to_string());
    let country = ip.and_then(|i| country_of(&state, i));
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let facts = RequestFacts {
        unix_secs: now_ms / 1000,
        unix_millis: now_ms,
        ip: ip_text.as_deref(),
        user_agent: h("user-agent"),
        origin_host: host_of(&origin),
        accept_language: h("accept-language"),
        sec_ch_ua: h("sec-ch-ua"),
        sec_ch_ua_platform: h("sec-ch-ua-platform"),
        country,
        secret: state.secret.as_deref(),
    };
    let decorated: Vec<_> = events.into_iter().map(|e| decorate(e, &facts)).collect();
    if let Err(err) = state.sink.write(&decorated) { eprintln!("visits: sink failed: {err}"); }
    taken(&origin)
}

/// The client address under `--trusted-proxies`, warning once per process
/// when `X-Forwarded-For` is too short to answer.
fn resolve_ip(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> Option<IpAddr> {
    let forwarded_for = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    let ip = client_ip(forwarded_for, peer.ip(), state.trusted_proxies);
    if ip.is_none() && state.trusted_proxies > 0 && !PROXY_WARNED.swap(true, Ordering::Relaxed) {
        eprintln!("visits: X-Forwarded-For has fewer entries than --trusted-proxies; every visitor is sharing one bucket and no visit is computed. Check the proxy depth with --log-ip.");
    }
    ip
}

fn ignored(state: &AppState, ip: Option<IpAddr>) -> bool {
    ip.is_some_and(|i| state.ignore.iter().any(|n| n.contains(i)))
}

/// The `429` when the address has spent its bucket. Every route draws on the
/// same bucket, keyed by address; unknown addresses share one.
fn limited(state: &AppState, ip: Option<IpAddr>) -> Option<Response> {
    let key = ip.map(|i| i.to_string()).unwrap_or_else(|| "unknown".into());
    if state.limiter.lock().map(|mut l| l.allow(&key)).unwrap_or(true) { return None; }
    Some((StatusCode::TOO_MANY_REQUESTS, [(header::CACHE_CONTROL, "no-store"), (header::RETRY_AFTER, "1")]).into_response())
}

fn only(allow: &'static str) -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::CACHE_CONTROL, "no-store"), (header::ALLOW, allow)]).into_response()
}

/// A program's request carries no `Origin`, and its agent is `<app>/<rest>`
/// with `<app>` one the collector allows. Answers that app, else the refusal
/// (boxed, as a `Response` is large for an `Err`).
fn gate(headers: &HeaderMap, apps: &BTreeMap<String, Option<String>>) -> Result<String, Box<Response>> {
    if headers.contains_key(header::ORIGIN) { return Err(Box::new(refuse(StatusCode::FORBIDDEN, "origin not allowed for reports"))); }
    let agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    match agent.split_once('/') {
        Some((app, _)) if apps.contains_key(app) => Ok(app.to_string()),
        _ => Err(Box::new(refuse(StatusCode::FORBIDDEN, "app not allowed"))),
    }
}

/// A validated report or check from the agent's own app, not from a source build.
fn admit(parsed: Result<AppReport, Invalid>, app: &str) -> Result<AppReport, Box<Response>> {
    let refusal = match parsed {
        Err(Invalid::Channel) => refuse(StatusCode::FORBIDDEN, "source channel"),
        Err(e) => refuse(StatusCode::BAD_REQUEST, &e.to_string()),
        Ok(r) if app_of(&r) != app => refuse(StatusCode::FORBIDDEN, "app not allowed"),
        Ok(r) if channel_of(&r) == "source" => refuse(StatusCode::FORBIDDEN, "source channel"),
        Ok(r) => return Ok(r),
    };
    Err(Box::new(refusal))
}

/// Decorates and writes a report; a sink's failure is printed, not answered.
fn write_report(state: &AppState, r: AppReport) {
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    if let Err(err) = state.sink.write_report(&decorate_report(r, now_ms)) { eprintln!("visits: sink failed: {err}"); }
}

fn no_content() -> Response {
    (StatusCode::NO_CONTENT, [(header::CACHE_CONTROL, "no-store")]).into_response()
}

/// `POST <app_path>`: one report. Never printed under `--log-ip`.
async fn report(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if method != Method::POST { return only("POST"); }
    let ip = resolve_ip(&state, &headers, peer);
    if ignored(&state, ip) { return no_content(); }
    if let Some(res) = limited(&state, ip) { return res; }
    let app = match gate(&headers, &state.apps) { Ok(a) => a, Err(res) => return *res };
    match admit(validate_report(&body), &app) {
        Ok(r) => write_report(&state, r),
        Err(res) => return *res,
    }
    no_content()
}

/// `GET <app_path>/check`: written before it is answered, so a client that does
/// not follow the redirect still counts; an ignored address is answered unwritten.
async fn check(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    if method != Method::GET { return only("GET"); }
    let ip = resolve_ip(&state, &headers, peer);
    if let Some(res) = limited(&state, ip) { return res; }
    let app = match gate(&headers, &state.apps) { Ok(a) => a, Err(res) => return *res };
    let r = match admit(validate_check(query.as_deref().unwrap_or("")), &app) { Ok(r) => r, Err(res) => return *res };
    if !ignored(&state, ip) { write_report(&state, r); }
    match state.apps.get(&app).cloned().flatten().and_then(|to| HeaderValue::from_str(&to).ok()) {
        Some(to) => (StatusCode::FOUND, [(header::CACHE_CONTROL, HeaderValue::from_static("no-store")), (header::LOCATION, to)]).into_response(),
        None => no_content(),
    }
}

#[cfg(feature = "geoip")]
fn country_of(state: &AppState, ip: IpAddr) -> Option<String> {
    let reader = state.geoip.as_ref()?;
    let c: maxminddb::geoip2::Country = reader.lookup(ip).ok()?;
    c.country.and_then(|c| c.iso_code).map(String::from)
}

#[cfg(not(feature = "geoip"))]
fn country_of(_state: &AppState, _ip: IpAddr) -> Option<String> { None }
