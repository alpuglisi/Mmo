//! `evo-server`: run the authoritative game server.
//!
//! ```text
//! evo-server [--bind ADDR] [--data DIR | --memory] [--size N] [--seed N]
//!            [--no-wild] [--tick-ms MS] [--state-ms MS] [--fast]
//! ```

use std::time::Duration;

use evo_server::{start, ServerConfig};

const USAGE: &str = "usage: evo-server [--bind 127.0.0.1:8080] [--data DIR | --memory] [--size 128] [--seed 1] \
[--no-wild] [--tick-ms 1000] [--state-ms 1000] [--fast]";

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
            "--tick-ms" => cfg.runner.tick = Duration::from_millis(value("--tick-ms")?.parse().map_err(|e| format!("--tick-ms: {e}"))?),
            "--state-ms" => {
                cfg.runner.state_interval = Duration::from_millis(value("--state-ms")?.parse().map_err(|e| format!("--state-ms: {e}"))?)
            }
            "--fast" => cfg = cfg.fast(),
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
    let mut server = match start(cfg).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not start: {e}");
            std::process::exit(1);
        }
    };
    let stop = server.take_shutdown();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down");
        if let Some(tx) = stop {
            let _ = tx.send(());
        }
    });
    server.wait().await;
}
