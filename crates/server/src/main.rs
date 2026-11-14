//! `evo-server`: run the authoritative game server.
//!
//! ```text
//! evo-server [--bind ADDR] [--data DIR | --memory | --database-url URL] [--size N] [--seed N]
//!            [--no-wild] [--days-per-year N] [--tick-ms MS] [--state-ms MS] [--fast] [--static DIR]
//!            [--session-idle-min N] [--session-max-hours N] [--no-rate-limits] [--lod] [--no-catastrophes]
//!            [--season-days N] [--offline-protection-hours H] [--fresh] [--world ID[,seed=N][,size=N][,name=TEXT]]... [--worlds-file FILE]
//!            [--no-push] [--vapid-private-key KEY] [--vapid-subject mailto:...] [--push-per-hour N]
//!            [--push-allow-any-endpoint] [--fcm-service-account FILE] [--apns-key FILE --apns-key-id ID
//!            --apns-team-id ID [--apns-topic BUNDLE_ID] [--apns-sandbox]] [--hash-concurrency N] [--hash-queue N]
//! ```
//!
//! `--database-url` (or the `DATABASE_URL` environment variable) keeps
//! everything in PostgreSQL instead of the data directory.
//!
//! Without `--world` the process hosts one world, `main`. Each `--world`
//! adds one (the first is the default); `--worlds-file` reads a JSON list
//! (`[{"id": "main"}, {"id": "isle", "seed": 2, "size": 96, "name": "Isle"}]`)
//! and the `EVO_WORLDS` environment variable takes `--world` values
//! separated by `;`. Command-line worlds replace the variable's. A world's
//! missing seed and size come from `--seed` and `--size`.
//!
//! Push notifications are on unless `--no-push`. The VAPID key comes from
//! `--vapid-private-key` (or `EVO_VAPID_PRIVATE_KEY`: the raw P-256 private
//! key, base64url), else from the store, where the server keeps the one it
//! generates on first start. `--vapid-subject` (or `EVO_VAPID_SUBJECT`) is
//! the contact push services see. `--push-allow-any-endpoint` accepts
//! subscription endpoints outside the browsers' push services, plain http
//! included (tests, private push servers).
//!
//! Native push for the apps is off until its credentials are given:
//! `--fcm-service-account FILE` (or `EVO_FCM_SERVICE_ACCOUNT`: a path, or the
//! key file's JSON itself) turns on FCM for Android; `--apns-key FILE` (or
//! `EVO_APNS_KEY`: a path to the `.p8` file, or its PEM text) with
//! `--apns-key-id` and `--apns-team-id` (`EVO_APNS_KEY_ID`,
//! `EVO_APNS_TEAM_ID`) turns on APNs for iOS, for the bundle id
//! `--apns-topic` (`EVO_APNS_TOPIC`, default `org.evolutionmmo.client`),
//! against Apple's development servers with `--apns-sandbox`
//! (`EVO_APNS_SANDBOX=1`).
//!
//! `--hash-concurrency` (or `EVO_HASH_CONCURRENCY`) sets how many password
//! hashes (registrations and logins) run at once, by default half the cores
//! (at least 1); `--hash-queue` (or `EVO_HASH_QUEUE`) how many more may
//! wait, by default 16 per concurrent hash. Past that a login or
//! registration is refused at once with 503 `busy` and `retry_after_ms`.
//!
//! SIGINT (Ctrl-C) or SIGTERM stops it gracefully, saving the world first.

use std::time::Duration;

use evo_server::accounts::HashPolicy;
use evo_server::webpush::apns::{self, ApnsConfig};
use evo_server::webpush::fcm::{FcmConfig, ServiceAccount};
use evo_server::worlds::WorldSpec;
use evo_server::{start, ServerConfig};

const USAGE: &str =
    "usage: evo-server [--bind 127.0.0.1:8080] [--data DIR | --memory | --database-url URL] [--size 128] [--seed 1] \
[--no-wild] [--days-per-year 120] [--tick-ms 1000] [--state-ms 1000] [--fast] [--static client/dist] \
[--session-idle-min 60] [--session-max-hours 24] [--no-rate-limits] [--lod] [--no-catastrophes] \
[--season-days 60] [--offline-protection-hours 12] [--fresh] [--world ID[,seed=N][,size=N][,name=TEXT]]... [--worlds-file FILE] \
[--no-push] [--vapid-private-key KEY] [--vapid-subject mailto:you@example.org] [--push-per-hour 4] \
[--push-allow-any-endpoint] [--fcm-service-account FILE] [--apns-key FILE --apns-key-id ID --apns-team-id ID \
[--apns-topic org.evolutionmmo.client] [--apns-sandbox]] [--hash-concurrency N] [--hash-queue N]";

/// A credential given as a file path, or inline (text containing `marker`).
fn read_credential(name: &str, value: &str, marker: &str) -> Result<String, String> {
    if value.contains(marker) {
        return Ok(value.to_string());
    }
    std::fs::read_to_string(value.trim()).map_err(|e| format!("{name} {value}: {e}"))
}

/// Native push settings as given; turned into `PushConfig::fcm` and
/// `PushConfig::apns` once every argument is read.
#[derive(Default)]
struct NativeArgs {
    fcm: Option<String>,
    apns_key: Option<String>,
    apns_key_id: Option<String>,
    apns_team_id: Option<String>,
    apns_topic: Option<String>,
    apns_sandbox: bool,
}

impl NativeArgs {
    fn from_env() -> Self {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        NativeArgs {
            fcm: env("EVO_FCM_SERVICE_ACCOUNT"),
            apns_key: env("EVO_APNS_KEY"),
            apns_key_id: env("EVO_APNS_KEY_ID"),
            apns_team_id: env("EVO_APNS_TEAM_ID"),
            apns_topic: env("EVO_APNS_TOPIC"),
            apns_sandbox: env("EVO_APNS_SANDBOX").is_some_and(|v| v.trim() == "1" || v.trim() == "true"),
        }
    }

    fn fcm(&self) -> Result<Option<FcmConfig>, String> {
        let Some(v) = &self.fcm else { return Ok(None) };
        let text = read_credential("--fcm-service-account", v, "{")?;
        Ok(Some(FcmConfig::new(ServiceAccount::from_json(&text)?)))
    }

    fn apns(&self) -> Result<Option<ApnsConfig>, String> {
        let Some(key) = &self.apns_key else {
            if self.apns_key_id.is_some() || self.apns_team_id.is_some() {
                return Err("--apns-key-id and --apns-team-id need --apns-key".into());
            }
            return Ok(None);
        };
        let (Some(key_id), Some(team_id)) = (&self.apns_key_id, &self.apns_team_id) else {
            return Err("--apns-key needs --apns-key-id and --apns-team-id".into());
        };
        let key_pem = read_credential("--apns-key", key, "-----BEGIN")?;
        apns::signing_key(&key_pem)?;
        Ok(Some(ApnsConfig {
            key_pem,
            key_id: key_id.trim().to_string(),
            team_id: team_id.trim().to_string(),
            topic: self.apns_topic.clone().unwrap_or_else(|| apns::DEFAULT_TOPIC.into()).trim().to_string(),
            sandbox: self.apns_sandbox,
            api: None,
        }))
    }
}

fn parse_args() -> Result<ServerConfig, String> {
    let mut cfg = ServerConfig {
        database_url: std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty()),
        ..Default::default()
    };
    if let Ok(k) = std::env::var("EVO_VAPID_PRIVATE_KEY") {
        cfg.push.vapid_private_key = Some(k).filter(|k| !k.trim().is_empty());
    }
    if let Ok(s) = std::env::var("EVO_VAPID_SUBJECT") {
        if !s.trim().is_empty() {
            cfg.push.subject = s;
        }
    }
    let mut worlds = match std::env::var("EVO_WORLDS") {
        Ok(v) if !v.trim().is_empty() => WorldSpec::parse_list(&v).map_err(|e| format!("EVO_WORLDS: {e}"))?,
        _ => Vec::new(),
    };
    let mut cli_worlds: Vec<WorldSpec> = Vec::new();
    let positive = |name: &str, v: &str| -> Result<usize, String> {
        match v.trim().parse::<usize>() {
            Ok(n) if n >= 1 => Ok(n),
            _ => Err(format!("{name} must be a whole number of at least 1")),
        }
    };
    let count = |name: &str, v: &str| v.trim().parse::<usize>().map_err(|e| format!("{name}: {e}"));
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let mut hash_concurrency = env("EVO_HASH_CONCURRENCY").map(|v| positive("EVO_HASH_CONCURRENCY", &v)).transpose()?;
    let mut hash_queue = env("EVO_HASH_QUEUE").map(|v| count("EVO_HASH_QUEUE", &v)).transpose()?;
    let mut offline_hours: Option<f64> = None;
    let mut native = NativeArgs::from_env();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--bind" => cfg.bind = value("--bind")?.parse().map_err(|e| format!("--bind: {e}"))?,
            "--data" => {
                cfg.data_dir = Some(value("--data")?.into());
                cfg.database_url = None;
            }
            "--memory" => {
                cfg.data_dir = None;
                cfg.database_url = None;
            }
            "--database-url" => cfg.database_url = Some(value("--database-url")?),
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
            "--tick-ms" => {
                cfg.runner.tick =
                    Duration::from_millis(value("--tick-ms")?.parse().map_err(|e| format!("--tick-ms: {e}"))?)
            }
            "--state-ms" => {
                cfg.runner.state_interval =
                    Duration::from_millis(value("--state-ms")?.parse().map_err(|e| format!("--state-ms: {e}"))?)
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
            "--season-days" => {
                let n: u64 = value("--season-days")?.parse().map_err(|e| format!("--season-days: {e}"))?;
                if !(1..=100_000).contains(&n) {
                    return Err("--season-days must be in 1..=100000".into());
                }
                cfg.game.season_days = n;
            }
            "--offline-protection-hours" => {
                let v = value("--offline-protection-hours")?;
                match v.trim().parse::<f64>() {
                    Ok(h) if (0.0..=24.0 * 365.0).contains(&h) => offline_hours = Some(h),
                    _ => return Err("--offline-protection-hours must be a number of hours in 0..=8760".into()),
                }
            }
            "--fresh" => cfg.fresh = true,
            "--no-push" => cfg.push.enabled = false,
            "--vapid-private-key" => cfg.push.vapid_private_key = Some(value("--vapid-private-key")?),
            "--vapid-subject" => {
                let s = value("--vapid-subject")?;
                if !(s.starts_with("mailto:") || s.starts_with("https://")) {
                    return Err("--vapid-subject must be a mailto: or https: URL".into());
                }
                cfg.push.subject = s;
            }
            "--push-per-hour" => {
                let n: u32 = value("--push-per-hour")?.parse().map_err(|e| format!("--push-per-hour: {e}"))?;
                if !(1..=60).contains(&n) {
                    return Err("--push-per-hour must be in 1..=60".into());
                }
                cfg.push.limits.per_hour = n;
                cfg.push.limits.min_gap = cfg.push.limits.min_gap.min(Duration::from_secs(3600 / u64::from(n)));
            }
            "--push-allow-any-endpoint" => cfg.push.allow_any_endpoint = true,
            "--fcm-service-account" => native.fcm = Some(value("--fcm-service-account")?),
            "--apns-key" => native.apns_key = Some(value("--apns-key")?),
            "--apns-key-id" => native.apns_key_id = Some(value("--apns-key-id")?),
            "--apns-team-id" => native.apns_team_id = Some(value("--apns-team-id")?),
            "--apns-topic" => native.apns_topic = Some(value("--apns-topic")?),
            "--apns-sandbox" => native.apns_sandbox = true,
            "--hash-concurrency" => {
                hash_concurrency = Some(positive("--hash-concurrency", &value("--hash-concurrency")?)?)
            }
            "--hash-queue" => hash_queue = Some(count("--hash-queue", &value("--hash-queue")?)?),
            "--world" => cli_worlds.push(WorldSpec::parse(&value("--world")?).map_err(|e| format!("--world: {e}"))?),
            "--worlds-file" => {
                let path = value("--worlds-file")?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("--worlds-file {path}: {e}"))?;
                cli_worlds.extend(WorldSpec::parse_file(&text).map_err(|e| format!("--worlds-file {path}: {e}"))?);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    // Offline protection is set in hours of wall-clock time: ticks at the
    // configured clock (counted as 1 s ticks when it runs as fast as it can).
    let tick_ms = if cfg.runner.tick.is_zero() { 1000.0 } else { cfg.runner.tick.as_secs_f64() * 1000.0 };
    let hours = offline_hours.unwrap_or(evo_server::game::DEFAULT_PROTECTION_HOURS);
    cfg.game.offline_protection_ticks = (hours * 3_600_000.0 / tick_ms).round() as u64;
    cfg.push.fcm = native.fcm()?;
    cfg.push.apns = native.apns()?;
    if let Some(n) = hash_concurrency {
        cfg.hashing = HashPolicy::with_concurrency(n);
    }
    if let Some(q) = hash_queue {
        cfg.hashing.queue = q;
    }
    if !cli_worlds.is_empty() {
        worlds = cli_worlds;
    }
    if !worlds.is_empty() {
        evo_server::worlds::validate(&worlds)?;
    }
    cfg.worlds = worlds;
    Ok(cfg)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        // A log line that can't be written (stdout on a full disk, a closed
        // pipe) is dropped. By default the subscriber reports it with
        // `eprintln!`, which panics when stderr fails too: the soak test
        // filled the disk and that killed the saver thread, stopping saves.
        .log_internal_errors(false)
        .init();
    // Every world's parallel decide and apply run on rayon's global pool
    // (one thread per core, shared by all worlds). Name its threads so
    // `top -H` and per-thread CPU tell them from the Tokio workers.
    if let Err(e) = rayon::ThreadPoolBuilder::new().thread_name(|i| format!("sim-{i}")).build_global() {
        tracing::warn!("could not set up the sim thread pool: {e}");
    }
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
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
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
