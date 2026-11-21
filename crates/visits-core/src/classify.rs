//! Browser, system, crawler and referrer, from the request's headers. Small
//! by design: the dozen families that matter, by substring, no regex crate.

fn major_after(ua: &str, token: &str) -> Option<u32> {
    let rest = &ua[ua.find(token)? + token.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Browser family and major version: client hints first, else the User-Agent.
pub fn browser_of(ua: &str, ch_ua: Option<&str>) -> (String, Option<u32>) {
    'hints: {
        let Some(hints) = ch_ua else { break 'hints };
        let mut chromium = None;
        for part in hints.split(',') {
            let part = part.trim();
            let brand = part.split(';').next().unwrap_or("").trim().trim_matches('"').trim().to_string();
            let version = part.split("v=").nth(1).and_then(|v| v.trim().trim_matches('"').trim().parse::<u32>().ok());
            if brand.is_empty() || brand.starts_with("Not") { continue; }
            if brand == "Chromium" { chromium = Some((brand, version)); continue; }
            let brand = match brand.as_str() { "Google Chrome" => "Chrome".to_string(), "Microsoft Edge" => "Edge".to_string(), _ => brand };
            if brand.chars().count() > 32 { break 'hints; }
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
        let p = p.trim().trim_matches('"').trim();
        return match p {
            "Chrome OS" => "ChromeOS".into(),
            "Windows" | "macOS" | "iOS" | "Android" | "Linux" => p.into(),
            _ => "other".into(),
        };
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
    let scheme_end = r.find("://")?;
    let scheme = &r[..scheme_end];
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)) { return None; }
    let after = &r[scheme_end + 3..];
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let host = &after[..end];
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    if host.is_empty() { return None; }
    let rest = &after[end..];
    let path = if rest.starts_with('/') { &rest[..rest.find(['?', '#']).unwrap_or(rest.len())] } else { "/" };
    Some((host_key(host), path.to_string()))
}

/// Networks that are clouds and hosting providers: a browser there is almost always automated.
/// Only providers that run no consumer access network, so their own staff do not land here
/// (Google's 15169 and Microsoft's 8075 carry offices as well as clouds, and are left out).
const CLOUD_ASNS: &[u32] = &[
    16509, 14618, // Amazon AWS
    396982, // Google Cloud
    14061, // DigitalOcean
    24940, 213230, // Hetzner
    16276, // OVH
    63949, // Linode (Akamai Connected Cloud)
    20473, // Vultr (Choopa)
    31898, // Oracle Cloud
    45102, // Alibaba Cloud
    132203, // Tencent Cloud
    51167, // Contabo
    12876, // Scaleway
    36352, // ColoCrossing
    197540, // netcup
    26496, // GoDaddy
    54290, // Hostwinds
    8100, // QuadraNet
    60781, 28753, // Leaseweb
];

/// Networks that mostly carry commercial VPNs: a person, but one whose place is the VPN's.
const VPN_ASNS: &[u32] = &[
    9009, // M247
    60068, 212238, // Datacamp (CDN77)
    136787, // TEFINCOM (NordVPN)
    147049, // PacketHub
];

/// Networks that relay people's own browsing: iCloud Private Relay's egress (Akamai, Cloudflare,
/// Fastly) and Cloudflare WARP. A person, placed near where they are.
const RELAY_ASNS: &[u32] = &[
    13335, // Cloudflare
    36183, 20940, 16625, // Akamai
    54113, // Fastly
];

/// What kind of network an address is on, from its ASN and the Tor exit list: `tor`, `cloud`,
/// `vpn` or `relay`, or none for every other network (an ISP, a company, a university).
pub fn network_kind(asn: Option<u32>, tor_exit: bool) -> Option<&'static str> {
    if tor_exit { return Some("tor"); }
    let asn = asn?;
    if CLOUD_ASNS.contains(&asn) { Some("cloud") }
    else if VPN_ASNS.contains(&asn) { Some("vpn") }
    else if RELAY_ASNS.contains(&asn) { Some("relay") }
    else { None }
}

/// Chromium's network hints, read from the request headers a page asked for with `Accept-CH`:
/// the effective connection type (`ECT`: slow-2g, 2g, 3g or 4g), the round trip in ms (`RTT`,
/// rounded by the browser to 25 ms), the downlink in Mbit/s (`Downlink`, rounded to 25 kbit/s)
/// and Save-Data. Anything else is dropped.
pub fn network_hints(ect: Option<&str>, rtt: Option<&str>, downlink: Option<&str>, save_data: Option<&str>) -> (Option<String>, Option<u32>, Option<f64>, Option<bool>) {
    let ect = ect.map(|v| v.trim().trim_matches('"').to_ascii_lowercase()).filter(|v| ["slow-2g", "2g", "3g", "4g"].contains(&v.as_str()));
    let rtt = rtt.and_then(|v| v.trim().parse::<u32>().ok()).filter(|v| *v <= 10_000);
    let dl = downlink.and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite() && (0.0..=10_000.0).contains(v)).map(|v| (v * 1000.0).round() / 1000.0);
    let sd = save_data.map(|v| v.trim().eq_ignore_ascii_case("on")).filter(|on| *on);
    (ect, rtt, dl, sd)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_brands_are_normalised() {
        assert_eq!(browser_of("", Some("\"Google Chrome\";v=\"130\"")), ("Chrome".into(), Some(130)));
        assert_eq!(browser_of("", Some("\"Microsoft Edge\";v=\"130\"")), ("Edge".into(), Some(130)));
    }

    #[test]
    fn an_overlong_hint_brand_falls_to_the_user_agent() {
        let hints = format!("\"{}\";v=\"1\"", "x".repeat(300));
        let ua = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
        assert_eq!(browser_of(ua, Some(&hints)), ("Firefox".into(), Some(128)));
    }

    #[test]
    fn whitespace_inside_quotes_is_trimmed() {
        let h = "\" Not A;Brand\";v=\"99\", \"Chromium\";v=\"99\", \"Google Chrome\";v=\"99\"";
        assert_eq!(browser_of("", Some(h)), ("Chrome".into(), Some(99)));
    }

    #[test]
    fn referrer_userinfo_and_scheme() {
        assert_eq!(split_referrer("https://user:pw@Host.com/p"), Some(("host.com".into(), "/p".into())));
        assert_eq!(split_referrer("/x?next=https://evil.com/"), None);
        assert_eq!(split_referrer("1x://host/"), None);
    }

    #[test]
    fn unknown_platform_is_other() {
        assert_eq!(os_of("", Some("\"Fuchsia\"")), "other");
        assert_eq!(os_of("", Some("\"Windows\"")), "Windows");
    }
}
