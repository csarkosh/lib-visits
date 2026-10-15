use crate::classify::{browser_of, host_key, is_crawler, os_of, split_referrer};
use crate::clock::rfc3339;
use crate::event::{Decoration, Event};
use crate::visit::visit_hash;

/// What the binary knows about a request and the core must not fetch itself.
pub struct RequestFacts<'a> {
    pub unix_secs: u64,
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
    let host = host_key(facts.origin_host);
    let (rk, rp, rh) = match event.fields.get("r").and_then(|v| v.as_str()).filter(|r| !r.is_empty()) {
        None => ("direct", None, None),
        Some(r) => match split_referrer(r) {
            Some((h, p)) if h == host => ("internal", Some(p), None),
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
    event.s = Some(Decoration {
        sv: 1,
        at: rfc3339(facts.unix_secs),
        visit,
        host,
        bn,
        bv,
        os: os_of(ua, facts.sec_ch_ua_platform),
        bot: client_bot || is_crawler(ua),
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
        let facts = RequestFacts { unix_secs: 0, ip: None, user_agent: None, origin_host: "x.com", accept_language: Some("*"), sec_ch_ua: None, sec_ch_ua_platform: None, country: None, secret: None };
        assert_eq!(decorate(event, &facts).s.unwrap().lg, None);
    }

    #[test]
    fn an_overlong_language_gives_no_lg() {
        let event: Event = serde_json::from_str(r#"{"v":1,"n":"view","id":"a","p":"/"}"#).unwrap();
        let long = "a".repeat(300);
        let facts = RequestFacts { unix_secs: 0, ip: None, user_agent: None, origin_host: "x.com", accept_language: Some(&long), sec_ch_ua: None, sec_ch_ua_platform: None, country: None, secret: None };
        assert_eq!(decorate(event, &facts).s.unwrap().lg, None);
    }
}
