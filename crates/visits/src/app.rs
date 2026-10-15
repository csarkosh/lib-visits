//! The router: the beacon route, the health route, and the checks in order:
//! method, origin, address and limiter, body, then decorate and sink.
use crate::limiter::Limiter;
use crate::sink::Sink;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, RawQuery, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use visits_core::{decorate, validate_json, validate_query, Invalid, RequestFacts};

pub const MAX_BODY: usize = 16 * 1024;

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
}

pub fn router(state: Arc<AppState>) -> Router {
    let path = state.path.clone();
    Router::new()
        .route("/healthcheck", get(|| async { ([(header::CONTENT_TYPE, "text/plain")], "ok") }))
        .route(&path, any(beacon))
        .layer(DefaultBodyLimit::max(MAX_BODY))
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
    let ip = client_ip(h("x-forwarded-for"), peer.ip(), state.trusted_proxies);
    if state.log_ip { eprintln!("visits: request from {}", ip.map(|i| i.to_string()).unwrap_or_else(|| "unknown".into())); }
    let key = ip.map(|i| i.to_string()).unwrap_or_else(|| "unknown".into());
    if !state.limiter.lock().map(|mut l| l.allow(&key)).unwrap_or(true) {
        return (StatusCode::TOO_MANY_REQUESTS, [(header::CACHE_CONTROL, "no-store"), (header::RETRY_AFTER, "1")]).into_response();
    }
    let parsed = if method == Method::GET { validate_query(query.as_deref().unwrap_or("")) } else { validate_json(&body) };
    let events = match parsed {
        Ok(e) => e,
        Err(Invalid::TooMany) => return refuse(StatusCode::PAYLOAD_TOO_LARGE, "too many events"),
        Err(e) => return refuse(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let ip_text = ip.map(|i| i.to_string());
    let country = ip.and_then(|i| country_of(&state, i));
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let facts = RequestFacts {
        unix_secs: now,
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
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&origin) { res.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v); }
    res
}

#[cfg(feature = "geoip")]
fn country_of(state: &AppState, ip: IpAddr) -> Option<String> {
    let reader = state.geoip.as_ref()?;
    let c: maxminddb::geoip2::Country = reader.lookup(ip).ok()?;
    c.country.and_then(|c| c.iso_code).map(String::from)
}

#[cfg(not(feature = "geoip"))]
fn country_of(_state: &AppState, _ip: IpAddr) -> Option<String> { None }
