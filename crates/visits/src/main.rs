use clap::Parser;
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use visits::app::{router, AppState};
use visits::config::{Cli, Command, Serve};
use visits::limiter::Limiter;
use visits::sink::sink_from;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Serve(args) => serve(args).await,
    }
}

async fn serve(args: Serve) -> anyhow::Result<()> {
    let secret = std::env::var(&args.secret_env).ok().filter(|s| !s.is_empty()).map(String::into_bytes);
    if secret.is_none() { eprintln!("visits: {} is not set, so events carry no visit", args.secret_env); }
    if args.log_ip { eprintln!("visits: --log-ip is on; this prints addresses to stderr and is for setup only"); }
    if !args.path.starts_with('/') || args.path == "/healthcheck" {
        anyhow::bail!("--path must start with / and not be /healthcheck, got {}", args.path);
    }
    if args.origin.is_empty() && args.app.is_empty() {
        anyhow::bail!("give at least one --origin or --app, or there is nothing to accept");
    }
    let mut apps = BTreeMap::new();
    for spec in &args.app {
        let (name, redirect) = visits::config::parse_app(spec).map_err(|e| anyhow::anyhow!(e))?;
        if apps.contains_key(&name) { anyhow::bail!("--app {name} is given twice"); }
        apps.insert(name, redirect);
    }
    if !apps.is_empty() { visits::config::check_paths(&args.path, &args.app_path).map_err(|e| anyhow::anyhow!(e))?; }
    let (burst, per_second) = args.limit_parts()?;
    let ignore = visits::ignore::parse_list(&args.ignore_ips).map_err(|e| anyhow::anyhow!("--ignore-ips: {e}"))?;
    if !ignore.is_empty() { eprintln!("visits: dropping beacons from {} ignored address range(s)", ignore.len()); }
    let geoip = load_geoip(args.geoip.as_deref())?;
    let state = Arc::new(AppState {
        origins: args.origin.iter().map(|o| o.trim_end_matches('/').to_string()).collect::<HashSet<_>>(),
        path: args.path.clone(),
        secret,
        sink: sink_from(&args.sink)?,
        limiter: Mutex::new(Limiter::new(burst, per_second)),
        trusted_proxies: args.trusted_proxies,
        log_ip: args.log_ip,
        geoip,
        ignore,
        apps,
        app_path: args.app_path.clone(),
    });
    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
    if args.origin.is_empty() {
        eprintln!("visits: listening on {}", args.listen);
    } else {
        eprintln!("visits: listening on {} for {} from {}", args.listen, args.path, args.origin.join(", "));
    }
    if !state.apps.is_empty() {
        let app_names = state.apps.keys().cloned().collect::<Vec<_>>().join(", ");
        eprintln!("visits: apps on {}: {}", args.app_path, app_names);
    }
    axum::serve(listener, router(state).into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

#[cfg(feature = "geoip")]
fn load_geoip(path: Option<&std::path::Path>) -> anyhow::Result<Option<visits::app::GeoIp>> {
    Ok(match path { Some(p) => Some(maxminddb::Reader::open_readfile(p)?), None => None })
}

#[cfg(not(feature = "geoip"))]
fn load_geoip(path: Option<&std::path::Path>) -> anyhow::Result<Option<visits::app::GeoIp>> {
    if path.is_some() { anyhow::bail!("--geoip needs a build with the geoip feature: cargo build --release --features geoip"); }
    Ok(None)
}

/// Resolves on ctrl-c, or on SIGTERM where there is one (containers, systemd).
async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            },
            Err(_) => { let _ = tokio::signal::ctrl_c().await; }
        }
    }
    #[cfg(not(unix))]
    { let _ = tokio::signal::ctrl_c().await; }
}
