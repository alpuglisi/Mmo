//! `evo-server`: run the authoritative game server.
//!
//! ```text
//! evo-server [--bind ADDR] [--data DIR | --memory] [--size N] [--seed N]
//!            [--no-wild] [--days-per-year N] [--tick-ms MS] [--state-ms MS] [--fast] [--static DIR]
//!            [--session-idle-min N] [--session-max-hours N] [--no-rate-limits] [--lod] [--no-catastrophes]
//!            [--fresh]
//! ```
//!
//! SIGINT (Ctrl-C) or SIGTERM stops it gracefully, saving the world first.

use std::time::Duration;

use evo_server::{start, ServerConfig};

const USAGE: &str = "usage: evo-server [--bind 127.0.0.1:8080] [--data DIR | --memory] [--size 128] [--seed 1] \
[--no-wild] [--days-per-year 120] [--tick-ms 1000] [--state-ms 1000] [--fast] [--static client/dist] \
[--session-idle-min 60] [--session-max-hours 24] [--no-rate-limits] [--lod] [--no-catastrophes] [--fresh]";

fn parse_args() -> Result<ServerConfig, String> {
    let mut cfg = ServerConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--bind" => cfg.bind = value("--bind")?.parse().map_err(|e| format!("--bind: {e}"))?,
            "--data" => cfg.data_dir = Some(value("--data")?.into()),
            "--memory" => cfg.data_dir = None,
            "--size" => {
                let n: usize = value("--size")?.parse().map_err(|e| format!("--size: {e}"))?;
                if !(32..=1024).contains(&n) {
                    return Err("--size must be in 32..=1024".into());
                }
                cfg.world.width = n;
                cfg.world.height = n;
            }
            "--seed" => cfg.world.seed = value("--seed")?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--no-wild" => cfg.world.wild = false,
            "--days-per-year" => {
                let n: u32 = value("--days-per-year")?.parse().map_err(|e| format!("--days-per-year: {e}"))?;
                if n < 4 {
                    return Err("--days-per-year must be at least 4".into());
                }
                cfg.world.days_per_year = n;
            }
            "--tick-ms" => cfg.runner.tick = Duration::from_millis(value("--tick-ms")?.parse().map_err(|e| format!("--tick-ms: {e}"))?),
            "--state-ms" => {
                cfg.runner.state_interval = Duration::from_millis(value("--state-ms")?.parse().map_err(|e| format!("--state-ms: {e}"))?)
            }
            "--fast" => cfg = cfg.fast(),
            "--static" => cfg.static_dir = Some(value("--static")?.into()),
            "--session-idle-min" => {
                let m: f64 = value("--session-idle-min")?.parse().map_err(|e| format!("--session-idle-min: {e}"))?;
                cfg.sessions.idle = Duration::from_secs_f64((m * 60.0).max(1.0));
            }
            "--session-max-hours" => {
                let h: f64 = value("--session-max-hours")?.parse().map_err(|e| format!("--session-max-hours: {e}"))?;
                cfg.sessions.max_age = Duration::from_secs_f64((h * 3600.0).max(1.0));
            }
            "--no-rate-limits" => cfg.limits = evo_server::limits::Limits::unlimited(),
            "--lod" => cfg.world.lod = true,
            "--no-catastrophes" => cfg.world.catastrophes = false,
            "--fresh" => cfg.fresh = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    Ok(cfg)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let server = match start(cfg).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not start: {e}");
            std::process::exit(1);
        }
    };
    shutdown_signal().await;
    tracing::info!("shutting down: saving the world");
    server.shutdown().await;
    tracing::info!("stopped");
}

/// SIGINT or (on Unix) SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
