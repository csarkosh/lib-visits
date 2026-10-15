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
    #[arg(long, env = "VISITS_ORIGINS", value_delimiter = ',', required = true)]
    pub origin: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    fn serve(limit: &str) -> Serve {
        Serve { listen: String::new(), path: "/v".into(), origin: vec![], secret_env: String::new(), sink: "stdout".into(), geoip: None, trusted_proxies: 0, limit: limit.into(), log_ip: false }
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
}
