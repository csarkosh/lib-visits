//! Browser, system, crawler and referrer, from the request's headers. Small
//! by design: the dozen families that matter, by substring, no regex crate.

fn major_after(ua: &str, token: &str) -> Option<u32> {
    let rest = &ua[ua.find(token)? + token.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Browser family and major version: client hints first, else the User-Agent.
pub fn browser_of(ua: &str, ch_ua: Option<&str>) -> (String, Option<u32>) {
    if let Some(hints) = ch_ua {
        let mut chromium = None;
        for part in hints.split(',') {
            let part = part.trim();
            let brand = part.split(';').next().unwrap_or("").trim().trim_matches('"').to_string();
            let version = part.split("v=").nth(1).and_then(|v| v.trim().trim_matches('"').parse::<u32>().ok());
            if brand.is_empty() || brand.starts_with("Not") { continue; }
            if brand == "Chromium" { chromium = Some((brand, version)); continue; }
            return (brand, version);
        }
        if let Some(c) = chromium { return c; }
    }
    const ORDER: &[(&str, &str)] = &[
        ("Edg/", "Edge"), ("OPR/", "Opera"), ("SamsungBrowser/", "Samsung Internet"),
        ("CriOS/", "Chrome"), ("FxiOS/", "Firefox"), ("Firefox/", "Firefox"), ("Chrome/", "Chrome"),
    ];
    for (token, name) in ORDER {
        if ua.contains(token) { return ((*name).into(), major_after(ua, token)); }
    }
    if ua.contains("Safari/") && ua.contains("Version/") { return ("Safari".into(), major_after(ua, "Version/")); }
    ("other".into(), None)
}

/// Operating system: the platform hint first, else the User-Agent.
pub fn os_of(ua: &str, ch_platform: Option<&str>) -> String {
    if let Some(p) = ch_platform {
        let p = p.trim().trim_matches('"');
        return match p { "Chrome OS" => "ChromeOS".into(), "" => "other".into(), other => other.into() };
    }
    const ORDER: &[(&str, &str)] = &[
        ("Windows", "Windows"), ("Android", "Android"), ("iPhone", "iOS"), ("iPad", "iOS"), ("iPod", "iOS"),
        ("Mac OS X", "macOS"), ("CrOS", "ChromeOS"), ("Linux", "Linux"),
    ];
    for (token, name) in ORDER {
        if ua.contains(token) { return (*name).into(); }
    }
    "other".into()
}

const CRAWLER_WORDS: &[&str] = &["bot", "crawl", "spider", "slurp", "headless", "lighthouse", "pagespeed", "gtmetrix", "facebookexternalhit", "preview"];

pub fn is_crawler(ua: &str) -> bool {
    let lower = ua.to_ascii_lowercase();
    CRAWLER_WORDS.iter().any(|w| lower.contains(w))
}

/// A host for comparison: lowercase, no leading `www.`, no port.
pub fn host_key(host: &str) -> String {
    let lower = host.to_ascii_lowercase();
    let no_port = lower.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map(|(h, _)| h).unwrap_or(&lower);
    no_port.strip_prefix("www.").unwrap_or(no_port).to_string()
}

/// `(host_key, path)` of a referrer URL, or `None` when it has no `scheme://host`.
pub fn split_referrer(r: &str) -> Option<(String, String)> {
    let after = &r[r.find("://")? + 3..];
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let host = &after[..end];
    if host.is_empty() { return None; }
    let rest = &after[end..];
    let path = if rest.starts_with('/') { &rest[..rest.find(['?', '#']).unwrap_or(rest.len())] } else { "/" };
    Some((host_key(host), path.to_string()))
}
