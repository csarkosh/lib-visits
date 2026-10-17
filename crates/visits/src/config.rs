use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "visits", version, about = "Cookieless web analytics: take beacons, emit JSON lines.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Listen for beacons and write decorated events to the sink.
    Serve(Serve),
}

#[derive(Args, Debug, Clone)]
pub struct Serve {
    /// Address to listen on.
    #[arg(long, env = "VISITS_LISTEN", default_value = "0.0.0.0:8080")]
    pub listen: String,
    /// The beacon route.
    #[arg(long, env = "VISITS_PATH", default_value = "/v")]
    pub path: String,
    /// A site allowed to post, e.g. https://example.com (repeatable; env: comma-separated).
    #[arg(long, env = "VISITS_ORIGINS", value_delimiter = ',')]
    pub origin: Vec<String>,
    /// An app allowed to report, as NAME or NAME=https://redirect (repeatable; env: comma-separated).
    #[arg(long = "app", env = "VISITS_APPS", value_delimiter = ',')]
    pub app: Vec<String>,
    /// The app report route; its check lives at <app-path>/check.
    #[arg(long, env = "VISITS_APP_PATH", default_value = "/a")]
    pub app_path: String,
    /// The environment variable holding the hash secret.
    #[arg(long, default_value = "VISITS_SECRET")]
    pub secret_env: String,
    /// stdout, or file:<path>.
    #[arg(long, env = "VISITS_SINK", default_value = "stdout")]
    pub sink: String,
    /// An .mmdb database for countries (needs the geoip feature).
    #[arg(long, env = "VISITS_GEOIP")]
    pub geoip: Option<PathBuf>,
    /// Proxies in front: the client is that many entries from the right of X-Forwarded-For.
    #[arg(long, env = "VISITS_TRUSTED_PROXIES", default_value_t = 0)]
    pub trusted_proxies: usize,
    /// Per-address token bucket as burst/per_second.
    #[arg(long, env = "VISITS_LIMIT", default_value = "60/1")]
    pub limit: String,
    /// Setup only: print each request's resolved address to stderr.
    #[arg(long)]
    pub log_ip: bool,
    /// Addresses whose beacons are dropped and whose reports are marked `own`, as addresses or CIDR prefixes (repeatable; env: comma-separated).
    #[arg(long = "ignore-ips", env = "VISITS_IGNORE_IPS", value_delimiter = ',')]
    pub ignore_ips: Vec<String>,
}

impl Serve {
    pub fn limit_parts(&self) -> anyhow::Result<(f64, f64)> {
        let (b, r) = self.limit.split_once('/').ok_or_else(|| anyhow::anyhow!("--limit wants burst/per_second, e.g. 60/1"))?;
        let burst: f64 = b.trim().parse().map_err(|_| anyhow::anyhow!("--limit burst must be a positive number, got {b}"))?;
        let rate: f64 = r.trim().parse().map_err(|_| anyhow::anyhow!("--limit per_second must be a non-negative number, got {r}"))?;
        if !burst.is_finite() || burst <= 0.0 { anyhow::bail!("--limit burst must be a positive number, got {b}"); }
        if !rate.is_finite() || rate < 0.0 { anyhow::bail!("--limit per_second must be a non-negative number, got {r}"); }
        Ok((burst, rate))
    }
}

/// Parses `NAME` or `NAME=https://redirect` into the name and the optional redirect.
pub fn parse_app(s: &str) -> Result<(String, Option<String>), String> {
    let (name, redirect) = match s.split_once('=') {
        Some((n, r)) => (n, Some(r)),
        None => (s, None),
    };
    let mut chars = name.chars();
    let ok = name.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !ok { return Err(format!("--app name must match [a-z][a-z0-9_-]{{0,31}}, got {name:?}")); }
    if let Some(r) = redirect {
        if !r.starts_with("https://") { return Err(format!("--app {name}: the redirect must start with https://, got {r:?}")); }
        // A value the check handler could not send as a Location header is refused here, once.
        if axum::http::HeaderValue::from_str(r).is_err() { return Err(format!("--app {name}: the redirect is not a valid header value")); }
    }
    Ok((name.to_string(), redirect.map(str::to_string)))
}

/// Refuses a route path in axum 0.8's capture syntax, so the flag names a
/// literal path: a `{` or `}` anywhere (a capture), or a segment that starts
/// with `:` or `*` (the old spellings, which axum panics on). A `:` or `*`
/// inside a segment is literal, so `/v:1` stays a valid path.
pub fn check_route(flag: &str, path: &str) -> Result<(), String> {
    if path.contains(['{', '}']) || path.split('/').any(|seg| seg.starts_with([':', '*'])) {
        return Err(format!("{flag} must not contain {{ or }} or a segment starting with : or *, got {path}"));
    }
    Ok(())
}

/// Checks the beacon and report routes so that no two of them overlap (axum panics on that).
pub fn check_paths(path: &str, app_path: &str) -> Result<(), String> {
    check_route("--path", path)?;
    check_route("--app-path", app_path)?;
    if !app_path.starts_with('/') || app_path == "/healthcheck" {
        return Err(format!("--app-path must start with / and not be /healthcheck, got {app_path}"));
    }
    if app_path.len() > 1 && app_path.ends_with('/') || app_path == "/" {
        return Err(format!("--app-path must not end with /, got {app_path}"));
    }
    if app_path == path || path == format!("{app_path}/check") {
        return Err(format!("--path {path} overlaps --app-path {app_path} and its /check route"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn serve(limit: &str) -> Serve {
        Serve { listen: String::new(), path: "/v".into(), origin: vec![], app: vec![], app_path: "/a".into(), secret_env: String::new(), sink: "stdout".into(), geoip: None, trusted_proxies: 0, limit: limit.into(), log_ip: false, ignore_ips: vec![] }
    }
    #[test]
    fn limit_parts_validates() {
        assert_eq!(serve("60/1").limit_parts().unwrap(), (60.0, 1.0));
        assert!(serve("60").limit_parts().is_err());
        assert!(serve("a/b").limit_parts().is_err());
        assert!(serve("0/1").limit_parts().is_err());
        assert!(serve("60/-1").limit_parts().is_err());
        assert!(serve("inf/1").limit_parts().is_err());
    }
    #[test]
    fn parse_app_accepts_name_and_redirect() {
        assert_eq!(parse_app("arkos=https://x").unwrap(), ("arkos".to_string(), Some("https://x".to_string())));
        assert_eq!(parse_app("arkos").unwrap(), ("arkos".to_string(), None));
        // The redirect is kept exactly as written, trailing slash included.
        assert_eq!(parse_app("a=https://x/").unwrap().1.as_deref(), Some("https://x/"));
        assert_eq!(parse_app("my-app_2=https://a.b/c?d=e").unwrap().0, "my-app_2");
    }
    #[test]
    fn parse_app_refuses_bad_input() {
        for bad in ["", "Arkos", "1app", "-a", "a b", "a.b", "arkos=", "arkos=http://x", "arkos=ftp://x", "arkos=x", "arkos=https://x\n", "arkos=https://x\u{7f}", "=https://x"] {
            assert!(parse_app(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(parse_app(&format!("a{}", "b".repeat(31))).is_ok());
        assert!(parse_app(&format!("a{}", "b".repeat(32))).is_err());
    }
    #[test]
    fn check_paths_refuses_overlaps() {
        assert!(check_paths("/v", "/a").is_ok());
        assert!(check_paths("/v", "a").is_err());
        assert!(check_paths("/v", "/").is_err());
        assert!(check_paths("/v", "/a/").is_err());
        assert!(check_paths("/v", "/v").is_err());
        assert!(check_paths("/v", "/healthcheck").is_err());
        assert!(check_paths("/a", "/a").is_err());
        assert!(check_paths("/a/check", "/a").is_err());
        assert!(check_paths("/v", "/v/check").is_ok());
    }
    #[test]
    fn route_paths_refuse_axum_capture_syntax() {
        for bad in ["/{x}", "/a{", "/a}", "/:x", "/a/:x", "/*x", "/a/*rest"] {
            assert!(check_route("--path", bad).is_err(), "{bad:?}");
            assert!(check_paths("/v", bad).is_err(), "{bad:?}");
            assert!(check_paths(bad, "/a").is_err(), "{bad:?}");
        }
        // A colon or a star inside a segment is literal to axum 0.8.
        for good in ["/v", "/my-app_2/v.1", "/v:1", "/a/b:c", "/a*b"] {
            assert!(check_route("--path", good).is_ok(), "{good:?}");
        }
    }
}
