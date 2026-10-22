use crate::classify::{browser_of, host_key, is_crawler, os_of, split_referrer};
use crate::clock::{rfc3339, rfc3339_ms};
use crate::event::{Decoration, Event};
use crate::visit::visit_hash;

/// What the binary knows about a request and the core must not fetch itself.
pub struct RequestFacts<'a> {
    pub unix_secs: u64,
    /// The time of receipt in milliseconds, which the event's own `ts` is checked against.
    pub unix_millis: u64,
    pub ip: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub origin_host: &'a str,
    pub accept_language: Option<&'a str>,
    pub sec_ch_ua: Option<&'a str>,
    pub sec_ch_ua_platform: Option<&'a str>,
    pub country: Option<String>,
    pub secret: Option<&'a [u8]>,
}

pub fn decorate(mut event: Event, facts: &RequestFacts) -> Event {
    let ua = facts.user_agent.unwrap_or("");
    let (bn, bv) = browser_of(ua, facts.sec_ch_ua);
    let client_bot = event.fields.get("b").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
    // The brand too: HeadlessChrome names itself in Sec-CH-UA under a User-Agent that claims a phone.
    let bot = client_bot || is_crawler(ua) || is_crawler(&bn);
    let host = host_key(facts.origin_host);
    let (rk, rp, rh) = match event.fields.get("r").and_then(|v| v.as_str()).filter(|r| !r.is_empty()) {
        None => ("direct", None, None),
        Some(r) => match split_referrer(r) {
            Some((h, p)) if h == host || h.ends_with(&format!(".{host}")) => ("internal", Some(p), None),
            Some((h, _)) => ("external", None, Some(h)),
            None => ("external", None, None),
        },
    };
    let lg = if event.fields.contains_key("lg") { None } else {
        facts.accept_language.and_then(|al| al.split(',').next()).map(|t| t.split(';').next().unwrap_or("").trim().to_string()).filter(|t| (1..=35).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    };
    let visit = match (facts.secret, facts.ip) {
        (Some(secret), Some(ip)) => Some(visit_hash(secret, facts.unix_secs, ip, ua)),
        _ => None,
    };
    // A client clock within five minutes of receipt is believed; beyond that the
    // clock is wrong, and the time of receipt stands in.
    let event_ms = match event.fields.get("ts").and_then(|v| v.as_u64()) {
        Some(ts) if ts.abs_diff(facts.unix_millis) <= 300_000 => ts,
        _ => facts.unix_millis,
    };
    event.time = Some(rfc3339_ms(event_ms));
    event.s = Some(Decoration {
        sv: 1,
        at: rfc3339(facts.unix_secs),
        visit,
        host,
        bn,
        bv,
        os: os_of(ua, facts.sec_ch_ua_platform),
        bot,
        rk: rk.into(),
        rp,
        rh,
        lg,
        cc: facts.country.clone(),
    });
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_language_gives_no_lg() {
        let event: Event = serde_json::from_str(r#"{"v":1,"n":"view","id":"a","p":"/"}"#).unwrap();
        let facts = RequestFacts { unix_secs: 0, unix_millis: 0, ip: None, user_agent: None, origin_host: "x.com", accept_language: Some("*"), sec_ch_ua: None, sec_ch_ua_platform: None, country: None, secret: None };
        assert_eq!(decorate(event, &facts).s.unwrap().lg, None);
    }

    #[test]
    fn an_overlong_language_gives_no_lg() {
        let event: Event = serde_json::from_str(r#"{"v":1,"n":"view","id":"a","p":"/"}"#).unwrap();
        let long = "a".repeat(300);
        let facts = RequestFacts { unix_secs: 0, unix_millis: 0, ip: None, user_agent: None, origin_host: "x.com", accept_language: Some(&long), sec_ch_ua: None, sec_ch_ua_platform: None, country: None, secret: None };
        assert_eq!(decorate(event, &facts).s.unwrap().lg, None);
    }

    fn at(unix_millis: u64) -> RequestFacts<'static> {
        RequestFacts { unix_secs: unix_millis / 1000, unix_millis, ip: None, user_agent: None, origin_host: "x.com", accept_language: None, sec_ch_ua: None, sec_ch_ua_platform: None, country: None, secret: None }
    }

    #[test]
    fn the_line_time_is_the_client_clock_when_near_else_receipt() {
        let near: Event = serde_json::from_str(r#"{"v":1,"n":"view","id":"a","p":"/","ts":1792032027500}"#).unwrap();
        assert_eq!(decorate(near, &at(1_792_032_030_000)).time.as_deref(), Some("2026-10-15T02:40:27.500Z"));
        let far: Event = serde_json::from_str(r#"{"v":1,"n":"view","id":"a","p":"/","ts":1792000000000}"#).unwrap();
        assert_eq!(decorate(far, &at(1_792_032_030_000)).time.as_deref(), Some("2026-10-15T02:40:30.000Z"));
        let none: Event = serde_json::from_str(r#"{"v":1,"n":"view","id":"a","p":"/"}"#).unwrap();
        assert_eq!(decorate(none, &at(1_792_032_030_250)).time.as_deref(), Some("2026-10-15T02:40:30.250Z"));
    }

    #[test]
    fn a_subdomain_of_the_host_is_internal() {
        let event: Event = serde_json::from_str(r#"{"v":1,"n":"view","id":"a","p":"/","r":"https://apps.x.com/lib"}"#).unwrap();
        let s = decorate(event, &at(0)).s.unwrap();
        assert_eq!((s.rk.as_str(), s.rp.as_deref()), ("internal", Some("/lib")));
        let event: Event = serde_json::from_str(r#"{"v":1,"n":"view","id":"a","p":"/","r":"https://notx.com/"}"#).unwrap();
        assert_eq!(decorate(event, &at(0)).s.unwrap().rk, "external");
    }
}
